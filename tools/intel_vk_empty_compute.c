#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vulkan/vulkan.h>

#define VK_CHECK(expr) do { \
    VkResult _r = (expr); \
    if (_r != VK_SUCCESS) { \
        fprintf(stderr, "%s failed: %d\n", #expr, _r); \
        return 1; \
    } \
} while (0)

static uint32_t *read_file_u32(const char *path, size_t *byte_len) {
    FILE *f = fopen(path, "rb");
    if (!f) {
        perror(path);
        return NULL;
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return NULL;
    }
    long len = ftell(f);
    if (len <= 0 || (len & 3) != 0) {
        fclose(f);
        return NULL;
    }
    rewind(f);
    uint32_t *data = malloc((size_t)len);
    if (!data) {
        fclose(f);
        return NULL;
    }
    if (fread(data, 1, (size_t)len, f) != (size_t)len) {
        free(data);
        fclose(f);
        return NULL;
    }
    fclose(f);
    *byte_len = (size_t)len;
    return data;
}

int main(int argc, char **argv) {
    const char *spv_path = argc > 1 ? argv[1] : "tools/empty_compute.spv";
    size_t spv_bytes = 0;
    uint32_t *spv = read_file_u32(spv_path, &spv_bytes);
    if (!spv) {
        fprintf(stderr, "could not read SPIR-V: %s\n", spv_path);
        return 1;
    }

    VkApplicationInfo app = {
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "trueos-empty-compute-probe",
        .apiVersion = VK_API_VERSION_1_2,
    };
    VkInstanceCreateInfo ici = {
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &app,
    };
    VkInstance instance;
    VK_CHECK(vkCreateInstance(&ici, NULL, &instance));

    uint32_t gpu_count = 0;
    VK_CHECK(vkEnumeratePhysicalDevices(instance, &gpu_count, NULL));
    VkPhysicalDevice *gpus = calloc(gpu_count, sizeof(*gpus));
    VK_CHECK(vkEnumeratePhysicalDevices(instance, &gpu_count, gpus));
    VkPhysicalDevice gpu = VK_NULL_HANDLE;
    VkPhysicalDeviceProperties props;
    for (uint32_t i = 0; i < gpu_count; ++i) {
        vkGetPhysicalDeviceProperties(gpus[i], &props);
        if (props.vendorID == 0x8086) {
            gpu = gpus[i];
            break;
        }
    }
    if (gpu == VK_NULL_HANDLE) {
        fprintf(stderr, "no Intel Vulkan device found\n");
        return 2;
    }
    vkGetPhysicalDeviceProperties(gpu, &props);
    fprintf(stderr, "selected Intel device: vendor=0x%04x device=0x%04x name=%s\n",
            props.vendorID, props.deviceID, props.deviceName);

    uint32_t qf_count = 0;
    vkGetPhysicalDeviceQueueFamilyProperties(gpu, &qf_count, NULL);
    VkQueueFamilyProperties *qfs = calloc(qf_count, sizeof(*qfs));
    vkGetPhysicalDeviceQueueFamilyProperties(gpu, &qf_count, qfs);
    uint32_t qf = UINT32_MAX;
    for (uint32_t i = 0; i < qf_count; ++i) {
        if (qfs[i].queueFlags & VK_QUEUE_COMPUTE_BIT) {
            qf = i;
            break;
        }
    }
    if (qf == UINT32_MAX) {
        fprintf(stderr, "no compute queue family found\n");
        return 3;
    }

    float prio = 1.0f;
    VkDeviceQueueCreateInfo qci = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = qf,
        .queueCount = 1,
        .pQueuePriorities = &prio,
    };
    VkDeviceCreateInfo dci = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        .queueCreateInfoCount = 1,
        .pQueueCreateInfos = &qci,
    };
    VkDevice dev;
    VK_CHECK(vkCreateDevice(gpu, &dci, NULL, &dev));
    VkQueue queue;
    vkGetDeviceQueue(dev, qf, 0, &queue);

    VkShaderModuleCreateInfo smci = {
        .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = spv_bytes,
        .pCode = spv,
    };
    VkShaderModule shader;
    VK_CHECK(vkCreateShaderModule(dev, &smci, NULL, &shader));

    VkPipelineLayoutCreateInfo plci = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
    };
    VkPipelineLayout layout;
    VK_CHECK(vkCreatePipelineLayout(dev, &plci, NULL, &layout));

    VkComputePipelineCreateInfo cpci = {
        .sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
        .stage = {
            .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            .stage = VK_SHADER_STAGE_COMPUTE_BIT,
            .module = shader,
            .pName = "main",
        },
        .layout = layout,
    };
    VkPipeline pipe;
    VK_CHECK(vkCreateComputePipelines(dev, VK_NULL_HANDLE, 1, &cpci, NULL, &pipe));

    VkCommandPoolCreateInfo pool_ci = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
        .queueFamilyIndex = qf,
    };
    VkCommandPool pool;
    VK_CHECK(vkCreateCommandPool(dev, &pool_ci, NULL, &pool));

    VkCommandBufferAllocateInfo cbai = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
        .commandPool = pool,
        .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
        .commandBufferCount = 1,
    };
    VkCommandBuffer cb;
    VK_CHECK(vkAllocateCommandBuffers(dev, &cbai, &cb));

    VkCommandBufferBeginInfo begin = {
        .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
    };
    VK_CHECK(vkBeginCommandBuffer(cb, &begin));
    vkCmdBindPipeline(cb, VK_PIPELINE_BIND_POINT_COMPUTE, pipe);
    vkCmdDispatch(cb, 1, 1, 1);
    VK_CHECK(vkEndCommandBuffer(cb));

    VkFenceCreateInfo fci = { .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
    VkFence fence;
    VK_CHECK(vkCreateFence(dev, &fci, NULL, &fence));
    VkSubmitInfo si = {
        .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
        .commandBufferCount = 1,
        .pCommandBuffers = &cb,
    };
    VK_CHECK(vkQueueSubmit(queue, 1, &si, fence));
    VK_CHECK(vkWaitForFences(dev, 1, &fence, VK_TRUE, 5000000000ull));
    VK_CHECK(vkDeviceWaitIdle(dev));

    vkDestroyFence(dev, fence, NULL);
    vkDestroyCommandPool(dev, pool, NULL);
    vkDestroyPipeline(dev, pipe, NULL);
    vkDestroyPipelineLayout(dev, layout, NULL);
    vkDestroyShaderModule(dev, shader, NULL);
    vkDestroyDevice(dev, NULL);
    vkDestroyInstance(instance, NULL);
    free(qfs);
    free(gpus);
    free(spv);
    return 0;
}
