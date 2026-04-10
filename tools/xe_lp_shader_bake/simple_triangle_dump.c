#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <vulkan/vulkan.h>

#define CHECK_VK(call)                                                             \
    do {                                                                           \
        VkResult result__ = (call);                                                \
        if (result__ != VK_SUCCESS) {                                              \
            fprintf(stderr, "%s failed: %d\n", #call, (int)result__);             \
            exit(1);                                                               \
        }                                                                          \
    } while (0)

typedef struct FileData {
    uint32_t *words;
    size_t word_count;
} FileData;

static FileData read_spirv(const char *path) {
    FILE *file = fopen(path, "rb");
    if (!file) {
        fprintf(stderr, "failed to open %s\n", path);
        exit(1);
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        fprintf(stderr, "failed to seek %s\n", path);
        exit(1);
    }
    long size = ftell(file);
    if (size <= 0 || (size % 4) != 0) {
        fprintf(stderr, "invalid spirv size %ld for %s\n", size, path);
        exit(1);
    }
    rewind(file);
    uint32_t *words = malloc((size_t)size);
    if (!words) {
        fprintf(stderr, "oom reading %s\n", path);
        exit(1);
    }
    if (fread(words, 1, (size_t)size, file) != (size_t)size) {
        fprintf(stderr, "failed to read %s\n", path);
        exit(1);
    }
    fclose(file);
    FileData data = { .words = words, .word_count = (size_t)size / 4 };
    return data;
}

static uint32_t find_memory_type(
    VkPhysicalDevice physical_device,
    uint32_t type_bits,
    VkMemoryPropertyFlags wanted
) {
    VkPhysicalDeviceMemoryProperties props;
    vkGetPhysicalDeviceMemoryProperties(physical_device, &props);
    for (uint32_t i = 0; i < props.memoryTypeCount; ++i) {
        if ((type_bits & (1u << i)) && (props.memoryTypes[i].propertyFlags & wanted) == wanted) {
            return i;
        }
    }
    fprintf(stderr, "no suitable memory type for flags=0x%x\n", wanted);
    exit(1);
}

static int is_expected_green(uint32_t pixel) {
    const uint8_t r = (uint8_t)((pixel >> 16) & 0xFF);
    const uint8_t g = (uint8_t)((pixel >> 8) & 0xFF);
    const uint8_t b = (uint8_t)(pixel & 0xFF);
    return r <= 8 && g >= 0xF0 && b <= 8;
}

static void dump_pixel(const char *label, uint32_t pixel) {
    const uint8_t b0 = (uint8_t)(pixel & 0xFF);
    const uint8_t b1 = (uint8_t)((pixel >> 8) & 0xFF);
    const uint8_t b2 = (uint8_t)((pixel >> 16) & 0xFF);
    const uint8_t b3 = (uint8_t)((pixel >> 24) & 0xFF);
    printf(
        "simple_triangle_dump: %s=0x%08X bytes=[%02X %02X %02X %02X]\n",
        label,
        pixel,
        b0,
        b1,
        b2,
        b3
    );
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s simple_triangle.vert.spv simple_triangle.frag.spv\n", argv[0]);
        return 1;
    }

    const VkApplicationInfo app_info = {
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "trueos-simple-triangle-dump",
        .applicationVersion = VK_MAKE_VERSION(1, 0, 0),
        .pEngineName = "none",
        .engineVersion = VK_MAKE_VERSION(1, 0, 0),
        .apiVersion = VK_API_VERSION_1_0,
    };
    const VkInstanceCreateInfo instance_info = {
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &app_info,
    };

    VkInstance instance;
    CHECK_VK(vkCreateInstance(&instance_info, NULL, &instance));

    uint32_t physical_count = 0;
    CHECK_VK(vkEnumeratePhysicalDevices(instance, &physical_count, NULL));
    if (physical_count == 0) {
        fprintf(stderr, "no vulkan physical devices\n");
        return 1;
    }
    VkPhysicalDevice *physical_devices = calloc(physical_count, sizeof(*physical_devices));
    CHECK_VK(vkEnumeratePhysicalDevices(instance, &physical_count, physical_devices));

    VkPhysicalDevice physical_device = VK_NULL_HANDLE;
    uint32_t graphics_family = UINT32_MAX;
    for (uint32_t i = 0; i < physical_count; ++i) {
        VkPhysicalDeviceProperties props;
        vkGetPhysicalDeviceProperties(physical_devices[i], &props);
        if (props.vendorID != 0x8086) {
            continue;
        }
        uint32_t queue_count = 0;
        vkGetPhysicalDeviceQueueFamilyProperties(physical_devices[i], &queue_count, NULL);
        VkQueueFamilyProperties *queues = calloc(queue_count, sizeof(*queues));
        vkGetPhysicalDeviceQueueFamilyProperties(physical_devices[i], &queue_count, queues);
        for (uint32_t q = 0; q < queue_count; ++q) {
            if (queues[q].queueFlags & VK_QUEUE_GRAPHICS_BIT) {
                physical_device = physical_devices[i];
                graphics_family = q;
                break;
            }
        }
        free(queues);
        if (physical_device != VK_NULL_HANDLE) {
            break;
        }
    }
    free(physical_devices);
    if (physical_device == VK_NULL_HANDLE) {
        fprintf(stderr, "failed to find intel graphics queue\n");
        return 1;
    }

    const float queue_priority = 1.0f;
    const VkDeviceQueueCreateInfo queue_info = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = graphics_family,
        .queueCount = 1,
        .pQueuePriorities = &queue_priority,
    };
    const VkDeviceCreateInfo device_info = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        .queueCreateInfoCount = 1,
        .pQueueCreateInfos = &queue_info,
    };

    VkDevice device;
    CHECK_VK(vkCreateDevice(physical_device, &device_info, NULL, &device));

    VkQueue queue;
    vkGetDeviceQueue(device, graphics_family, 0, &queue);

    const VkCommandPoolCreateInfo pool_info = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        .queueFamilyIndex = graphics_family,
    };
    VkCommandPool command_pool;
    CHECK_VK(vkCreateCommandPool(device, &pool_info, NULL, &command_pool));

    const VkCommandBufferAllocateInfo command_alloc = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = command_pool,
        .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        .commandBufferCount = 1,
    };
    VkCommandBuffer command_buffer;
    CHECK_VK(vkAllocateCommandBuffers(device, &command_alloc, &command_buffer));

    const VkImageCreateInfo image_info = {
        .sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
        .imageType = VK_IMAGE_TYPE_2D,
        .format = VK_FORMAT_R8G8B8A8_UNORM,
        .extent = { 64, 64, 1 },
        .mipLevels = 1,
        .arrayLayers = 1,
        .samples = VK_SAMPLE_COUNT_1_BIT,
        .tiling = VK_IMAGE_TILING_OPTIMAL,
        .usage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
        .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
        .initialLayout = VK_IMAGE_LAYOUT_UNDEFINED,
    };
    VkImage image;
    CHECK_VK(vkCreateImage(device, &image_info, NULL, &image));

    VkMemoryRequirements image_mem_reqs;
    vkGetImageMemoryRequirements(device, image, &image_mem_reqs);
    const VkMemoryAllocateInfo image_mem_alloc = {
        .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize = image_mem_reqs.size,
        .memoryTypeIndex = find_memory_type(
            physical_device,
            image_mem_reqs.memoryTypeBits,
            VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT
        ),
    };
    VkDeviceMemory image_memory;
    CHECK_VK(vkAllocateMemory(device, &image_mem_alloc, NULL, &image_memory));
    CHECK_VK(vkBindImageMemory(device, image, image_memory, 0));

    const VkImageViewCreateInfo image_view_info = {
        .sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
        .image = image,
        .viewType = VK_IMAGE_VIEW_TYPE_2D,
        .format = VK_FORMAT_R8G8B8A8_UNORM,
        .subresourceRange = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .baseMipLevel = 0,
            .levelCount = 1,
            .baseArrayLayer = 0,
            .layerCount = 1,
        },
    };
    VkImageView image_view;
    CHECK_VK(vkCreateImageView(device, &image_view_info, NULL, &image_view));

    const VkAttachmentDescription attachment = {
        .format = VK_FORMAT_R8G8B8A8_UNORM,
        .samples = VK_SAMPLE_COUNT_1_BIT,
        .loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR,
        .storeOp = VK_ATTACHMENT_STORE_OP_STORE,
        .stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE,
        .stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE,
        .initialLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        .finalLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
    };
    const VkAttachmentReference color_ref = {
        .attachment = 0,
        .layout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
    };
    const VkSubpassDescription subpass = {
        .pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS,
        .colorAttachmentCount = 1,
        .pColorAttachments = &color_ref,
    };
    const VkRenderPassCreateInfo render_pass_info = {
        .sType = VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
        .attachmentCount = 1,
        .pAttachments = &attachment,
        .subpassCount = 1,
        .pSubpasses = &subpass,
    };
    VkRenderPass render_pass;
    CHECK_VK(vkCreateRenderPass(device, &render_pass_info, NULL, &render_pass));

    const VkFramebufferCreateInfo framebuffer_info = {
        .sType = VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
        .renderPass = render_pass,
        .attachmentCount = 1,
        .pAttachments = &image_view,
        .width = 64,
        .height = 64,
        .layers = 1,
    };
    VkFramebuffer framebuffer;
    CHECK_VK(vkCreateFramebuffer(device, &framebuffer_info, NULL, &framebuffer));

    FileData vs_spirv = read_spirv(argv[1]);
    FileData fs_spirv = read_spirv(argv[2]);
    const VkShaderModuleCreateInfo vs_info = {
        .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = vs_spirv.word_count * sizeof(uint32_t),
        .pCode = vs_spirv.words,
    };
    const VkShaderModuleCreateInfo fs_info = {
        .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = fs_spirv.word_count * sizeof(uint32_t),
        .pCode = fs_spirv.words,
    };
    VkShaderModule vs_module;
    VkShaderModule fs_module;
    CHECK_VK(vkCreateShaderModule(device, &vs_info, NULL, &vs_module));
    CHECK_VK(vkCreateShaderModule(device, &fs_info, NULL, &fs_module));

    const VkPipelineShaderStageCreateInfo stages[2] = {
        {
            .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            .stage = VK_SHADER_STAGE_VERTEX_BIT,
            .module = vs_module,
            .pName = "main",
        },
        {
            .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            .stage = VK_SHADER_STAGE_FRAGMENT_BIT,
            .module = fs_module,
            .pName = "main",
        },
    };

    const VkVertexInputBindingDescription binding = {
        .binding = 0,
        .stride = 12,
        .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription attribute = {
        .location = 0,
        .binding = 0,
        .format = VK_FORMAT_R32G32B32_SFLOAT,
        .offset = 0,
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1,
        .pVertexBindingDescriptions = &binding,
        .vertexAttributeDescriptionCount = 1,
        .pVertexAttributeDescriptions = &attribute,
    };
    const VkPipelineInputAssemblyStateCreateInfo input_assembly = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
        .topology = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
    };
    const VkViewport viewport = {
        .x = 0.0f,
        .y = 0.0f,
        .width = 64.0f,
        .height = 64.0f,
        .minDepth = 0.0f,
        .maxDepth = 1.0f,
    };
    const VkRect2D scissor = {
        .offset = { 0, 0 },
        .extent = { 64, 64 },
    };
    const VkPipelineViewportStateCreateInfo viewport_state = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
        .viewportCount = 1,
        .pViewports = &viewport,
        .scissorCount = 1,
        .pScissors = &scissor,
    };
    const VkPipelineRasterizationStateCreateInfo raster = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
        .polygonMode = VK_POLYGON_MODE_FILL,
        .cullMode = VK_CULL_MODE_NONE,
        .frontFace = VK_FRONT_FACE_COUNTER_CLOCKWISE,
        .lineWidth = 1.0f,
    };
    const VkPipelineMultisampleStateCreateInfo multisample = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
        .rasterizationSamples = VK_SAMPLE_COUNT_1_BIT,
    };
    const VkPipelineColorBlendAttachmentState blend_attachment = {
        .blendEnable = VK_FALSE,
        .colorWriteMask = VK_COLOR_COMPONENT_R_BIT | VK_COLOR_COMPONENT_G_BIT |
                          VK_COLOR_COMPONENT_B_BIT | VK_COLOR_COMPONENT_A_BIT,
    };
    const VkPipelineColorBlendStateCreateInfo blend = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
        .attachmentCount = 1,
        .pAttachments = &blend_attachment,
    };
    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
    };
    VkPipelineLayout pipeline_layout;
    CHECK_VK(vkCreatePipelineLayout(device, &pipeline_layout_info, NULL, &pipeline_layout));

    const VkGraphicsPipelineCreateInfo pipeline_info = {
        .sType = VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
        .stageCount = 2,
        .pStages = stages,
        .pVertexInputState = &vertex_input,
        .pInputAssemblyState = &input_assembly,
        .pViewportState = &viewport_state,
        .pRasterizationState = &raster,
        .pMultisampleState = &multisample,
        .pColorBlendState = &blend,
        .layout = pipeline_layout,
        .renderPass = render_pass,
        .subpass = 0,
    };
    VkPipeline pipeline;
    CHECK_VK(vkCreateGraphicsPipelines(device, VK_NULL_HANDLE, 1, &pipeline_info, NULL, &pipeline));

    const float vertices[9] = {
        0.0f, 0.72f, 0.0f,
        -0.72f, -0.58f, 0.0f,
        0.72f, -0.58f, 0.0f,
    };
    const VkBufferCreateInfo buffer_info = {
        .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        .size = sizeof(vertices),
        .usage = VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
        .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
    };
    VkBuffer vertex_buffer;
    CHECK_VK(vkCreateBuffer(device, &buffer_info, NULL, &vertex_buffer));
    VkMemoryRequirements vertex_mem_reqs;
    vkGetBufferMemoryRequirements(device, vertex_buffer, &vertex_mem_reqs);
    const VkMemoryAllocateInfo vertex_alloc = {
        .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize = vertex_mem_reqs.size,
        .memoryTypeIndex = find_memory_type(
            physical_device,
            vertex_mem_reqs.memoryTypeBits,
            VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT
        ),
    };
    VkDeviceMemory vertex_memory;
    CHECK_VK(vkAllocateMemory(device, &vertex_alloc, NULL, &vertex_memory));
    CHECK_VK(vkBindBufferMemory(device, vertex_buffer, vertex_memory, 0));
    void *mapped = NULL;
    CHECK_VK(vkMapMemory(device, vertex_memory, 0, sizeof(vertices), 0, &mapped));
    memcpy(mapped, vertices, sizeof(vertices));
    vkUnmapMemory(device, vertex_memory);

    const VkDeviceSize readback_size = 64u * 64u * 4u;
    const VkBufferCreateInfo readback_info = {
        .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        .size = readback_size,
        .usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT,
        .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
    };
    VkBuffer readback_buffer;
    CHECK_VK(vkCreateBuffer(device, &readback_info, NULL, &readback_buffer));
    VkMemoryRequirements readback_mem_reqs;
    vkGetBufferMemoryRequirements(device, readback_buffer, &readback_mem_reqs);
    const VkMemoryAllocateInfo readback_alloc = {
        .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize = readback_mem_reqs.size,
        .memoryTypeIndex = find_memory_type(
            physical_device,
            readback_mem_reqs.memoryTypeBits,
            VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT
        ),
    };
    VkDeviceMemory readback_memory;
    CHECK_VK(vkAllocateMemory(device, &readback_alloc, NULL, &readback_memory));
    CHECK_VK(vkBindBufferMemory(device, readback_buffer, readback_memory, 0));

    const VkCommandBufferBeginInfo begin_info = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
    };
    CHECK_VK(vkBeginCommandBuffer(command_buffer, &begin_info));

    const VkImageMemoryBarrier barrier = {
        .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        .srcAccessMask = 0,
        .dstAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
        .oldLayout = VK_IMAGE_LAYOUT_UNDEFINED,
        .newLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .image = image,
        .subresourceRange = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .baseMipLevel = 0,
            .levelCount = 1,
            .baseArrayLayer = 0,
            .layerCount = 1,
        },
    };
    vkCmdPipelineBarrier(
        command_buffer,
        VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
        VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
        0,
        0, NULL,
        0, NULL,
        1, &barrier
    );

    const VkClearValue clear_value = { .color = { .float32 = { 0.0f, 0.0f, 0.0f, 1.0f } } };
    const VkRenderPassBeginInfo rp_begin = {
        .sType = VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
        .renderPass = render_pass,
        .framebuffer = framebuffer,
        .renderArea = { .offset = { 0, 0 }, .extent = { 64, 64 } },
        .clearValueCount = 1,
        .pClearValues = &clear_value,
    };
    vkCmdBeginRenderPass(command_buffer, &rp_begin, VK_SUBPASS_CONTENTS_INLINE);
    vkCmdBindPipeline(command_buffer, VK_PIPELINE_BIND_POINT_GRAPHICS, pipeline);
    VkDeviceSize vertex_offset = 0;
    vkCmdBindVertexBuffers(command_buffer, 0, 1, &vertex_buffer, &vertex_offset);
    vkCmdDraw(command_buffer, 3, 1, 0, 0);
    vkCmdEndRenderPass(command_buffer);

    const VkImageMemoryBarrier transfer_barrier = {
        .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
        .dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT,
        .oldLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        .newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .image = image,
        .subresourceRange = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .baseMipLevel = 0,
            .levelCount = 1,
            .baseArrayLayer = 0,
            .layerCount = 1,
        },
    };
    vkCmdPipelineBarrier(
        command_buffer,
        VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
        VK_PIPELINE_STAGE_TRANSFER_BIT,
        0,
        0, NULL,
        0, NULL,
        1, &transfer_barrier
    );

    const VkBufferImageCopy readback_region = {
        .bufferOffset = 0,
        .bufferRowLength = 0,
        .bufferImageHeight = 0,
        .imageSubresource = {
            .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
            .mipLevel = 0,
            .baseArrayLayer = 0,
            .layerCount = 1,
        },
        .imageOffset = { 0, 0, 0 },
        .imageExtent = { 64, 64, 1 },
    };
    vkCmdCopyImageToBuffer(
        command_buffer,
        image,
        VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
        readback_buffer,
        1,
        &readback_region
    );
    CHECK_VK(vkEndCommandBuffer(command_buffer));

    const VkSubmitInfo submit_info = {
        .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
        .commandBufferCount = 1,
        .pCommandBuffers = &command_buffer,
    };
    CHECK_VK(vkQueueSubmit(queue, 1, &submit_info, VK_NULL_HANDLE));
    CHECK_VK(vkQueueWaitIdle(queue));
    CHECK_VK(vkDeviceWaitIdle(device));

    void *readback_map = NULL;
    CHECK_VK(vkMapMemory(device, readback_memory, 0, readback_size, 0, &readback_map));
    const uint32_t *pixels = (const uint32_t *)readback_map;
    const uint32_t center = pixels[32 * 64 + 32];
    const uint32_t up = pixels[24 * 64 + 32];
    const uint32_t down = pixels[40 * 64 + 32];
    const uint32_t left = pixels[32 * 64 + 24];
    const uint32_t right = pixels[32 * 64 + 40];
    const uint32_t corner = pixels[0];
    dump_pixel("center", center);
    dump_pixel("up", up);
    dump_pixel("down", down);
    dump_pixel("left", left);
    dump_pixel("right", right);
    dump_pixel("corner", corner);
    printf("simple_triangle_dump: verified=%d\n", is_expected_green(center));
    vkUnmapMemory(device, readback_memory);

    if (!is_expected_green(center)) {
        fprintf(
            stderr,
            "simple_triangle_dump: verification failed, expected center pixel to be green\n"
        );
        return 2;
    }

    vkDestroyBuffer(device, vertex_buffer, NULL);
    vkFreeMemory(device, vertex_memory, NULL);
    vkDestroyBuffer(device, readback_buffer, NULL);
    vkFreeMemory(device, readback_memory, NULL);
    vkDestroyPipeline(device, pipeline, NULL);
    vkDestroyPipelineLayout(device, pipeline_layout, NULL);
    vkDestroyShaderModule(device, vs_module, NULL);
    vkDestroyShaderModule(device, fs_module, NULL);
    vkDestroyFramebuffer(device, framebuffer, NULL);
    vkDestroyRenderPass(device, render_pass, NULL);
    vkDestroyImageView(device, image_view, NULL);
    vkDestroyImage(device, image, NULL);
    vkFreeMemory(device, image_memory, NULL);
    vkFreeCommandBuffers(device, command_pool, 1, &command_buffer);
    vkDestroyCommandPool(device, command_pool, NULL);
    vkDestroyDevice(device, NULL);
    vkDestroyInstance(instance, NULL);
    free(vs_spirv.words);
    free(fs_spirv.words);
    return 0;
}
