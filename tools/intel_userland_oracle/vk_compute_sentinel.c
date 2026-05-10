#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <vulkan/vulkan.h>

#define CHECK_VK(call)                                                           \
    do {                                                                         \
        VkResult result__ = (call);                                              \
        if (result__ != VK_SUCCESS) {                                            \
            fprintf(stderr, "%s failed: %d\n", #call, (int)result__);           \
            exit(1);                                                             \
        }                                                                        \
    } while (0)

typedef struct Spirv {
    uint32_t *words;
    size_t size_bytes;
} Spirv;

static Spirv read_spirv(const char *path) {
    FILE *file = fopen(path, "rb");
    if (!file) {
        fprintf(stderr, "open failed: %s\n", path);
        exit(1);
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        fprintf(stderr, "seek failed: %s\n", path);
        exit(1);
    }
    long size = ftell(file);
    if (size <= 0 || (size % 4) != 0) {
        fprintf(stderr, "invalid SPIR-V size %ld: %s\n", size, path);
        exit(1);
    }
    rewind(file);
    uint32_t *words = malloc((size_t)size);
    if (!words) {
        fprintf(stderr, "oom reading SPIR-V\n");
        exit(1);
    }
    if (fread(words, 1, (size_t)size, file) != (size_t)size) {
        fprintf(stderr, "read failed: %s\n", path);
        exit(1);
    }
    fclose(file);
    return (Spirv){ .words = words, .size_bytes = (size_t)size };
}

static const char *device_type_name(VkPhysicalDeviceType type) {
    switch (type) {
        case VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU:
            return "integrated";
        case VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU:
            return "discrete";
        case VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU:
            return "virtual";
        case VK_PHYSICAL_DEVICE_TYPE_CPU:
            return "cpu";
        default:
            return "other";
    }
}

static uint32_t env_u32_or(const char *name, uint32_t fallback) {
    const char *value = getenv(name);
    if (!value || !value[0]) {
        return fallback;
    }
    char *end = NULL;
    unsigned long parsed = strtoul(value, &end, 0);
    if (end == value || *end != '\0' || parsed > UINT32_MAX) {
        fprintf(stderr, "invalid %s=%s\n", name, value);
        exit(1);
    }
    return (uint32_t)parsed;
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
    fprintf(stderr, "no memory type for flags=0x%x type_bits=0x%x\n", wanted, type_bits);
    exit(1);
}

static void init_t5_small_live4(uint32_t *words) {
    words[0] = 0x3F800000u;  // x0 = 1.0
    words[1] = 0x40000000u;  // x1 = 2.0
    words[2] = 0x40400000u;  // x2 = 3.0
    words[3] = 0x40800000u;  // x3 = 4.0
    words[8] = 0x00003F80u;  // w0 = bf16(1.0)
    words[9] = 0x00004000u;  // w1 = bf16(2.0)
    words[10] = 0x00004040u; // w2 = bf16(3.0)
    words[11] = 0x00004080u; // w3 = bf16(4.0)
}

static void init_t5_small_live4_trueos_arena(uint32_t *words) {
    init_t5_small_live4(words);
    words[8] = 0;
    words[9] = 0;
    words[10] = 0;
    words[11] = 0;
    words[2048] = 0x00003F80u; // w0 = bf16(1.0)
    words[2049] = 0x00004000u; // w1 = bf16(2.0)
    words[2050] = 0x00004040u; // w2 = bf16(3.0)
    words[2051] = 0x00004080u; // w3 = bf16(4.0)
}

static int verify_sentinel(const uint32_t *words) {
    const uint32_t expected_lanes = 8;
    printf(
        "oracle-app: header words[0..7]=0x%08X 0x%08X 0x%08X 0x%08X 0x%08X 0x%08X 0x%08X 0x%08X\n",
        words[0], words[1], words[2], words[3], words[4], words[5], words[6], words[7]
    );
    int ok = words[0] == 0xC0DE7733u && words[1] == expected_lanes;
    for (uint32_t lane = 0; lane < expected_lanes; ++lane) {
        const uint32_t base = 8u + lane * 4u;
        const int lane_ok =
            words[base + 0] == 0xC0DE7800u + lane &&
            words[base + 1] == lane &&
            words[base + 2] == lane &&
            words[base + 3] == 0xE0F00000u + lane;
        ok = ok && lane_ok;
        printf(
            "oracle-app: lane[%u] ok=%d words=0x%08X 0x%08X 0x%08X 0x%08X\n",
            lane,
            lane_ok,
            words[base + 0],
            words[base + 1],
            words[base + 2],
            words[base + 3]
        );
    }
    printf(
        "oracle-app: verified=%d expected_header=0xC0DE7733 observed_header=0x%08X lanes=%u\n",
        ok,
        words[0],
        expected_lanes
    );
    return ok;
}

static int verify_t5_small_live4(const uint32_t *words) {
    const uint32_t expected_bits = 0x41F00000u; // 1*1 + 2*2 + 3*3 + 4*4 = 30.0
    const int ok =
        words[16] == expected_bits &&
        words[17] == 4u &&
        words[18] == 0xC0DE7504u &&
        words[19] == 0u;
    printf(
        "oracle-app: t5-small-live4 input_x_bits=0x%08X 0x%08X 0x%08X 0x%08X input_w_bf16=0x%04X 0x%04X 0x%04X 0x%04X\n",
        words[0], words[1], words[2], words[3],
        words[8] & 0xFFFFu, words[9] & 0xFFFFu, words[10] & 0xFFFFu, words[11] & 0xFFFFu
    );
    printf(
        "oracle-app: t5-small-live4 verified=%d expected_bits=0x%08X observed_bits=0x%08X live_k=%u sentinel=0x%08X workgroup=%u\n",
        ok,
        expected_bits,
        words[16],
        words[17],
        words[18],
        words[19]
    );
    return ok;
}

static int verify_t5_small_live4_trueos_arena(const uint32_t *words) {
    const uint32_t out = 264192u;
    const uint32_t expected_bits = 0x41F00000u; // 1*1 + 2*2 + 3*3 + 4*4 = 30.0
    const int ok =
        words[out + 0] == expected_bits &&
        words[out + 1] == 4u &&
        words[out + 2] == 0xC0DE7505u &&
        words[out + 3] == 0u;
    printf(
        "oracle-app: t5-small-live4-trueos-arena input_x_bits=0x%08X 0x%08X 0x%08X 0x%08X input_w_bf16=0x%04X 0x%04X 0x%04X 0x%04X out_dword=%u\n",
        words[0], words[1], words[2], words[3],
        words[2048] & 0xFFFFu, words[2049] & 0xFFFFu, words[2050] & 0xFFFFu, words[2051] & 0xFFFFu,
        out
    );
    printf(
        "oracle-app: t5-small-live4-trueos-arena verified=%d expected_bits=0x%08X observed_bits=0x%08X live_k=%u sentinel=0x%08X workgroup=%u\n",
        ok,
        expected_bits,
        words[out + 0],
        words[out + 1],
        words[out + 2],
        words[out + 3]
    );
    return ok;
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);

    if (argc < 2 || argc > 3) {
        fprintf(stderr, "usage: %s shader.comp.spv [sentinel|t5-small-live4]\n", argv[0]);
        return 1;
    }
    const char *workload = argc == 3 ? argv[2] : "sentinel";
    const int is_sentinel = strcmp(workload, "sentinel") == 0;
    const int is_t5_small_live4 = strcmp(workload, "t5-small-live4") == 0;
    const int is_t5_small_live4_trueos_arena =
        strcmp(workload, "t5-small-live4-trueos-arena") == 0;
    if (!is_sentinel && !is_t5_small_live4 && !is_t5_small_live4_trueos_arena) {
        fprintf(stderr, "unsupported workload: %s\n", workload);
        return 1;
    }

    const uint32_t wanted_vendor = env_u32_or("TRUEOS_ORACLE_VK_VENDOR_ID", 0x8086);
    const uint32_t wanted_device = env_u32_or("TRUEOS_ORACLE_VK_DEVICE_ID", 0xA780);
    printf("oracle-app: wanted vendor=0x%04X device=0x%04X\n", wanted_vendor, wanted_device);
    printf("oracle-app: lens macro=begin workload=%s\n", workload);

    const VkApplicationInfo app_info = {
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "trueos-intel-userland-oracle",
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
    VkPhysicalDevice *physical_devices = calloc(physical_count, sizeof(*physical_devices));
    CHECK_VK(vkEnumeratePhysicalDevices(instance, &physical_count, physical_devices));

    VkPhysicalDevice physical_device = VK_NULL_HANDLE;
    uint32_t queue_family = UINT32_MAX;
    VkPhysicalDeviceProperties selected_props = { 0 };

    for (uint32_t i = 0; i < physical_count; ++i) {
        VkPhysicalDeviceProperties props;
        vkGetPhysicalDeviceProperties(physical_devices[i], &props);
        printf(
            "oracle-app: physical[%u] vendor=0x%04X device=0x%04X type=%s api=0x%08X driver=0x%08X name=\"%s\"\n",
            i,
            props.vendorID,
            props.deviceID,
            device_type_name(props.deviceType),
            props.apiVersion,
            props.driverVersion,
            props.deviceName
        );
        if (props.vendorID != wanted_vendor || props.deviceID != wanted_device) {
            continue;
        }
        uint32_t queue_count = 0;
        vkGetPhysicalDeviceQueueFamilyProperties(physical_devices[i], &queue_count, NULL);
        VkQueueFamilyProperties *queues = calloc(queue_count, sizeof(*queues));
        vkGetPhysicalDeviceQueueFamilyProperties(physical_devices[i], &queue_count, queues);
        for (uint32_t q = 0; q < queue_count; ++q) {
            if (queues[q].queueFlags & VK_QUEUE_COMPUTE_BIT) {
                physical_device = physical_devices[i];
                queue_family = q;
                selected_props = props;
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
        fprintf(stderr, "oracle-app: no matching compute-capable Intel Vulkan device\n");
        return 1;
    }

    printf(
        "oracle-app: selected vendor=0x%04X device=0x%04X queue_family=%u name=\"%s\"\n",
        selected_props.vendorID,
        selected_props.deviceID,
        queue_family,
        selected_props.deviceName
    );
    printf("oracle-app: lens macro=selected-device\n");

    const float priority = 1.0f;
    const VkDeviceQueueCreateInfo queue_info = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = queue_family,
        .queueCount = 1,
        .pQueuePriorities = &priority,
    };
    const VkDeviceCreateInfo device_info = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        .queueCreateInfoCount = 1,
        .pQueueCreateInfos = &queue_info,
    };
    VkDevice device;
    CHECK_VK(vkCreateDevice(physical_device, &device_info, NULL, &device));

    VkQueue queue;
    vkGetDeviceQueue(device, queue_family, 0, &queue);

    const VkDeviceSize buffer_size = is_t5_small_live4_trueos_arena ? 0x103000u : 4096u;
    const VkBufferCreateInfo buffer_info = {
        .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        .size = buffer_size,
        .usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT | VK_BUFFER_USAGE_TRANSFER_SRC_BIT,
        .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
    };
    VkBuffer buffer;
    CHECK_VK(vkCreateBuffer(device, &buffer_info, NULL, &buffer));
    VkMemoryRequirements mem_reqs;
    vkGetBufferMemoryRequirements(device, buffer, &mem_reqs);
    const VkMemoryAllocateInfo mem_alloc = {
        .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize = mem_reqs.size,
        .memoryTypeIndex = find_memory_type(
            physical_device,
            mem_reqs.memoryTypeBits,
            VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT
        ),
    };
    VkDeviceMemory memory;
    CHECK_VK(vkAllocateMemory(device, &mem_alloc, NULL, &memory));
    CHECK_VK(vkBindBufferMemory(device, buffer, memory, 0));
    void *mapped = NULL;
    CHECK_VK(vkMapMemory(device, memory, 0, buffer_size, 0, &mapped));
    memset(mapped, 0, (size_t)buffer_size);
    if (is_t5_small_live4) {
        init_t5_small_live4((uint32_t *)mapped);
    } else if (is_t5_small_live4_trueos_arena) {
        init_t5_small_live4_trueos_arena((uint32_t *)mapped);
    }

    Spirv spv = read_spirv(argv[1]);
    const VkShaderModuleCreateInfo shader_info = {
        .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = spv.size_bytes,
        .pCode = spv.words,
    };
    VkShaderModule shader;
    CHECK_VK(vkCreateShaderModule(device, &shader_info, NULL, &shader));

    const VkDescriptorSetLayoutBinding binding = {
        .binding = 0,
        .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
        .descriptorCount = 1,
        .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT,
    };
    const VkDescriptorSetLayoutCreateInfo set_layout_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 1,
        .pBindings = &binding,
    };
    VkDescriptorSetLayout set_layout;
    CHECK_VK(vkCreateDescriptorSetLayout(device, &set_layout_info, NULL, &set_layout));

    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 1,
        .pSetLayouts = &set_layout,
    };
    VkPipelineLayout pipeline_layout;
    CHECK_VK(vkCreatePipelineLayout(device, &pipeline_layout_info, NULL, &pipeline_layout));

    const VkComputePipelineCreateInfo pipeline_info = {
        .sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
        .stage = {
            .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            .stage = VK_SHADER_STAGE_COMPUTE_BIT,
            .module = shader,
            .pName = "main",
        },
        .layout = pipeline_layout,
    };
    VkPipeline pipeline;
    CHECK_VK(vkCreateComputePipelines(device, VK_NULL_HANDLE, 1, &pipeline_info, NULL, &pipeline));
    printf("oracle-app: lens macro=pipeline-created\n");

    const VkDescriptorPoolSize pool_size = {
        .type = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
        .descriptorCount = 1,
    };
    const VkDescriptorPoolCreateInfo pool_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
        .maxSets = 1,
        .poolSizeCount = 1,
        .pPoolSizes = &pool_size,
    };
    VkDescriptorPool descriptor_pool;
    CHECK_VK(vkCreateDescriptorPool(device, &pool_info, NULL, &descriptor_pool));
    const VkDescriptorSetAllocateInfo set_alloc = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
        .descriptorPool = descriptor_pool,
        .descriptorSetCount = 1,
        .pSetLayouts = &set_layout,
    };
    VkDescriptorSet descriptor_set;
    CHECK_VK(vkAllocateDescriptorSets(device, &set_alloc, &descriptor_set));
    const VkDescriptorBufferInfo descriptor_buffer = {
        .buffer = buffer,
        .offset = 0,
        .range = buffer_size,
    };
    const VkWriteDescriptorSet write_set = {
        .sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
        .dstSet = descriptor_set,
        .dstBinding = 0,
        .descriptorCount = 1,
        .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
        .pBufferInfo = &descriptor_buffer,
    };
    vkUpdateDescriptorSets(device, 1, &write_set, 0, NULL);

    const VkCommandPoolCreateInfo command_pool_info = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
        .queueFamilyIndex = queue_family,
    };
    VkCommandPool command_pool;
    CHECK_VK(vkCreateCommandPool(device, &command_pool_info, NULL, &command_pool));
    const VkCommandBufferAllocateInfo command_alloc = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = command_pool,
        .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        .commandBufferCount = 1,
    };
    VkCommandBuffer command_buffer;
    CHECK_VK(vkAllocateCommandBuffers(device, &command_alloc, &command_buffer));

    const VkCommandBufferBeginInfo begin_info = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
    };
    CHECK_VK(vkBeginCommandBuffer(command_buffer, &begin_info));
    printf("oracle-app: lens macro=record-begin\n");
    vkCmdBindPipeline(command_buffer, VK_PIPELINE_BIND_POINT_COMPUTE, pipeline);
    vkCmdBindDescriptorSets(
        command_buffer,
        VK_PIPELINE_BIND_POINT_COMPUTE,
        pipeline_layout,
        0,
        1,
        &descriptor_set,
        0,
        NULL
    );
    vkCmdDispatch(command_buffer, 1, 1, 1);
    const VkBufferMemoryBarrier barrier = {
        .sType = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,
        .srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT,
        .dstAccessMask = VK_ACCESS_HOST_READ_BIT,
        .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
        .buffer = buffer,
        .offset = 0,
        .size = buffer_size,
    };
    vkCmdPipelineBarrier(
        command_buffer,
        VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
        VK_PIPELINE_STAGE_HOST_BIT,
        0,
        0, NULL,
        1, &barrier,
        0, NULL
    );
    CHECK_VK(vkEndCommandBuffer(command_buffer));
    printf("oracle-app: lens macro=record-end pre-submit\n");

    const VkSubmitInfo submit_info = {
        .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
        .commandBufferCount = 1,
        .pCommandBuffers = &command_buffer,
    };
    printf("oracle-app: lens macro=submit-enter\n");
    CHECK_VK(vkQueueSubmit(queue, 1, &submit_info, VK_NULL_HANDLE));
    CHECK_VK(vkQueueWaitIdle(queue));
    CHECK_VK(vkDeviceWaitIdle(device));
    printf("oracle-app: lens macro=submit-complete wait-idle-done\n");

    const uint32_t *words = (const uint32_t *)mapped;
    const int ok = is_t5_small_live4_trueos_arena
        ? verify_t5_small_live4_trueos_arena(words)
        : is_t5_small_live4 ? verify_t5_small_live4(words) : verify_sentinel(words);

    vkUnmapMemory(device, memory);
    vkDestroyCommandPool(device, command_pool, NULL);
    vkDestroyDescriptorPool(device, descriptor_pool, NULL);
    vkDestroyPipeline(device, pipeline, NULL);
    vkDestroyPipelineLayout(device, pipeline_layout, NULL);
    vkDestroyDescriptorSetLayout(device, set_layout, NULL);
    vkDestroyShaderModule(device, shader, NULL);
    vkDestroyBuffer(device, buffer, NULL);
    vkFreeMemory(device, memory, NULL);
    vkDestroyDevice(device, NULL);
    vkDestroyInstance(instance, NULL);
    free(spv.words);

    return ok ? 0 : 2;
}
