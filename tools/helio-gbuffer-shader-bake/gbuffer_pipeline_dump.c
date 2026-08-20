#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

#include <vulkan/vulkan.h>

#define MATERIAL_TEXTURE_COUNT 256u
#define GBUFFER_COLOR_TARGET_COUNT 8u

#define CHECK_VK(call)                                                            \
    do {                                                                          \
        VkResult result__ = (call);                                               \
        if (result__ != VK_SUCCESS) {                                             \
            fprintf(stderr, "%s failed: %d\n", #call, (int)result__);           \
            exit(1);                                                              \
        }                                                                         \
    } while (0)

typedef struct FileData {
    uint32_t *words;
    size_t word_count;
} FileData;

static FileData read_spirv(const char *path) {
    FILE *file = fopen(path, "rb");
    if (!file) {
        fprintf(stderr, "failed to open %s: %s\n", path, strerror(errno));
        exit(1);
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        fprintf(stderr, "failed to seek %s\n", path);
        exit(1);
    }
    const long size = ftell(file);
    if (size <= 0 || (size % 4) != 0) {
        fprintf(stderr, "invalid SPIR-V size %ld for %s\n", size, path);
        exit(1);
    }
    rewind(file);
    uint32_t *words = malloc((size_t)size);
    if (!words) {
        fprintf(stderr, "out of memory reading %s\n", path);
        exit(1);
    }
    if (fread(words, 1, (size_t)size, file) != (size_t)size) {
        fprintf(stderr, "failed to read %s\n", path);
        exit(1);
    }
    fclose(file);
    return (FileData){ .words = words, .word_count = (size_t)size / 4u };
}

static const char *stage_name(VkShaderStageFlags stage) {
    switch (stage) {
        case VK_SHADER_STAGE_VERTEX_BIT:
            return "vertex";
        case VK_SHADER_STAGE_FRAGMENT_BIT:
            return "fragment";
        default:
            return "unknown";
    }
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

static int parse_u32_env(const char *name, uint32_t *out) {
    const char *value = getenv(name);
    if (!value || !value[0]) {
        return 0;
    }
    char *end = NULL;
    const unsigned long parsed = strtoul(value, &end, 0);
    if (end == value || *end != '\0' || parsed > UINT32_MAX) {
        fprintf(stderr, "invalid %s=%s\n", name, value);
        exit(1);
    }
    *out = (uint32_t)parsed;
    return 1;
}

static int physical_device_matches_selector(const VkPhysicalDeviceProperties *props) {
    uint32_t wanted_device_id = 0;
    if (parse_u32_env("TRUEOS_VK_DEVICE_ID", &wanted_device_id) &&
        props->deviceID != wanted_device_id) {
        return 0;
    }
    const char *wanted_name = getenv("TRUEOS_VK_DEVICE_NAME");
    return !(wanted_name && wanted_name[0] && strstr(props->deviceName, wanted_name) == NULL);
}

static int physical_device_has_extension(VkPhysicalDevice physical_device, const char *wanted) {
    uint32_t count = 0;
    CHECK_VK(vkEnumerateDeviceExtensionProperties(physical_device, NULL, &count, NULL));
    VkExtensionProperties *extensions = calloc(count, sizeof(*extensions));
    if (!extensions) {
        fprintf(stderr, "out of memory enumerating device extensions\n");
        exit(1);
    }
    CHECK_VK(vkEnumerateDeviceExtensionProperties(physical_device, NULL, &count, extensions));
    int found = 0;
    for (uint32_t i = 0; i < count; ++i) {
        if (strcmp(extensions[i].extensionName, wanted) == 0) {
            found = 1;
            break;
        }
    }
    free(extensions);
    return found;
}

static void sanitize_name(const char *src, char *dst, size_t dst_size) {
    size_t cursor = 0;
    if (!dst_size) {
        return;
    }
    for (size_t i = 0; src[i] && cursor + 1 < dst_size; ++i) {
        const char c = src[i];
        dst[cursor++] = ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                         (c >= '0' && c <= '9')) ? c : '_';
    }
    dst[cursor] = '\0';
}

static void ensure_dir(const char *path) {
    if (!path || !path[0]) {
        return;
    }
    if (mkdir(path, 0755) == 0 || errno == EEXIST) {
        return;
    }
    fprintf(stderr, "failed to create %s: %s\n", path, strerror(errno));
    exit(1);
}

static void write_blob_file(const char *dir, const char *name, const void *data, size_t size) {
    if (!dir || !dir[0]) {
        return;
    }
    char path[1024];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    FILE *file = fopen(path, "wb");
    if (!file) {
        fprintf(stderr, "failed to open %s: %s\n", path, strerror(errno));
        exit(1);
    }
    if (size && fwrite(data, 1, size, file) != size) {
        fprintf(stderr, "failed to write %s\n", path);
        exit(1);
    }
    fclose(file);
}

static void dump_pipeline_cache_blob(VkDevice device, VkPipelineCache cache) {
    const char *out_dir = getenv("TRUEOS_EXECUTABLE_DUMP_DIR");
    if (!out_dir || !out_dir[0]) {
        return;
    }
    size_t size = 0;
    CHECK_VK(vkGetPipelineCacheData(device, cache, &size, NULL));
    if (!size) {
        fprintf(stderr, "helio_gbuffer_dump: empty pipeline cache\n");
        exit(1);
    }
    void *data = malloc(size);
    if (!data) {
        fprintf(stderr, "out of memory reading pipeline cache\n");
        exit(1);
    }
    CHECK_VK(vkGetPipelineCacheData(device, cache, &size, data));
    printf("helio_gbuffer_dump: pipeline_cache_size=%zu\n", size);
    write_blob_file(out_dir, "pipeline_cache.bin", data, size);
    free(data);
}

static void dump_pipeline_executables(VkDevice device, VkPipeline pipeline) {
    PFN_vkGetPipelineExecutablePropertiesKHR get_props =
        (PFN_vkGetPipelineExecutablePropertiesKHR)vkGetDeviceProcAddr(
            device, "vkGetPipelineExecutablePropertiesKHR");
    PFN_vkGetPipelineExecutableStatisticsKHR get_stats =
        (PFN_vkGetPipelineExecutableStatisticsKHR)vkGetDeviceProcAddr(
            device, "vkGetPipelineExecutableStatisticsKHR");
    PFN_vkGetPipelineExecutableInternalRepresentationsKHR get_reps =
        (PFN_vkGetPipelineExecutableInternalRepresentationsKHR)vkGetDeviceProcAddr(
            device, "vkGetPipelineExecutableInternalRepresentationsKHR");
    if (!get_props || !get_stats || !get_reps) {
        fprintf(stderr, "pipeline executable introspection unavailable\n");
        exit(1);
    }

    const char *out_dir = getenv("TRUEOS_EXECUTABLE_DUMP_DIR");
    ensure_dir(out_dir);
    const VkPipelineInfoKHR pipeline_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_INFO_KHR,
        .pipeline = pipeline,
    };
    uint32_t executable_count = 0;
    CHECK_VK(get_props(device, &pipeline_info, &executable_count, NULL));
    if (!executable_count) {
        fprintf(stderr, "pipeline exposes no executables\n");
        exit(1);
    }
    VkPipelineExecutablePropertiesKHR *props = calloc(executable_count, sizeof(*props));
    if (!props) {
        fprintf(stderr, "out of memory reading pipeline executables\n");
        exit(1);
    }
    for (uint32_t i = 0; i < executable_count; ++i) {
        props[i].sType = VK_STRUCTURE_TYPE_PIPELINE_EXECUTABLE_PROPERTIES_KHR;
    }
    CHECK_VK(get_props(device, &pipeline_info, &executable_count, props));
    printf("helio_gbuffer_dump: executable_count=%u\n", executable_count);

    for (uint32_t exec = 0; exec < executable_count; ++exec) {
        const VkPipelineExecutableInfoKHR info = {
            .sType = VK_STRUCTURE_TYPE_PIPELINE_EXECUTABLE_INFO_KHR,
            .pipeline = pipeline,
            .executableIndex = exec,
        };
        char stage_tag[64];
        sanitize_name(stage_name(props[exec].stages), stage_tag, sizeof(stage_tag));
        printf(
            "helio_gbuffer_dump: executable[%u] stage=%s name=\"%s\" desc=\"%s\" subgroup=%u\n",
            exec,
            stage_name(props[exec].stages),
            props[exec].name,
            props[exec].description,
            props[exec].subgroupSize);

        uint32_t stat_count = 0;
        CHECK_VK(get_stats(device, &info, &stat_count, NULL));
        VkPipelineExecutableStatisticKHR *stats = calloc(stat_count, sizeof(*stats));
        for (uint32_t i = 0; i < stat_count; ++i) {
            stats[i].sType = VK_STRUCTURE_TYPE_PIPELINE_EXECUTABLE_STATISTIC_KHR;
        }
        if (stat_count) {
            CHECK_VK(get_stats(device, &info, &stat_count, stats));
        }
        for (uint32_t i = 0; i < stat_count; ++i) {
            char value[128];
            switch (stats[i].format) {
                case VK_PIPELINE_EXECUTABLE_STATISTIC_FORMAT_BOOL32_KHR:
                    snprintf(value, sizeof(value), "%u", stats[i].value.b32);
                    break;
                case VK_PIPELINE_EXECUTABLE_STATISTIC_FORMAT_INT64_KHR:
                    snprintf(value, sizeof(value), "%lld", (long long)stats[i].value.i64);
                    break;
                case VK_PIPELINE_EXECUTABLE_STATISTIC_FORMAT_UINT64_KHR:
                    snprintf(value, sizeof(value), "%llu", (unsigned long long)stats[i].value.u64);
                    break;
                case VK_PIPELINE_EXECUTABLE_STATISTIC_FORMAT_FLOAT64_KHR:
                    snprintf(value, sizeof(value), "%.3f", stats[i].value.f64);
                    break;
                default:
                    snprintf(value, sizeof(value), "unknown");
                    break;
            }
            printf("helio_gbuffer_dump: stat[%u][%u] name=\"%s\" value=%s\n",
                   exec, i, stats[i].name, value);
        }
        free(stats);

        uint32_t rep_count = 0;
        CHECK_VK(get_reps(device, &info, &rep_count, NULL));
        VkPipelineExecutableInternalRepresentationKHR *reps = calloc(rep_count, sizeof(*reps));
        for (uint32_t i = 0; i < rep_count; ++i) {
            reps[i].sType = VK_STRUCTURE_TYPE_PIPELINE_EXECUTABLE_INTERNAL_REPRESENTATION_KHR;
        }
        if (rep_count) {
            CHECK_VK(get_reps(device, &info, &rep_count, reps));
        }
        for (uint32_t i = 0; i < rep_count; ++i) {
            if (reps[i].dataSize) {
                reps[i].pData = malloc(reps[i].dataSize);
                if (!reps[i].pData) {
                    fprintf(stderr, "out of memory reading internal representation\n");
                    exit(1);
                }
            }
        }
        if (rep_count) {
            CHECK_VK(get_reps(device, &info, &rep_count, reps));
        }
        for (uint32_t i = 0; i < rep_count; ++i) {
            char rep_tag[128];
            sanitize_name(reps[i].name, rep_tag, sizeof(rep_tag));
            printf("helio_gbuffer_dump: rep[%u][%u] name=\"%s\" isText=%u size=%zu\n",
                   exec, i, reps[i].name, reps[i].isText, reps[i].dataSize);
            if (reps[i].pData && reps[i].dataSize) {
                char file_name[256];
                snprintf(file_name, sizeof(file_name), "%02u_%s_%02u_%s.%s",
                         exec, stage_tag, i, rep_tag, reps[i].isText ? "txt" : "bin");
                write_blob_file(out_dir, file_name, reps[i].pData, reps[i].dataSize);
            }
            free(reps[i].pData);
        }
        free(reps);
    }
    free(props);
}

static void require_descriptor_indexing(
    VkPhysicalDevice physical_device,
    const VkPhysicalDeviceProperties *properties,
    VkPhysicalDeviceDescriptorIndexingFeatures *enabled_features
) {
    VkPhysicalDeviceDescriptorIndexingFeatures supported = {
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_INDEXING_FEATURES,
    };
    VkPhysicalDeviceFeatures2 features = {
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2,
        .pNext = &supported,
    };
    vkGetPhysicalDeviceFeatures2(physical_device, &features);
    if (!supported.shaderSampledImageArrayNonUniformIndexing ||
        !supported.descriptorBindingSampledImageUpdateAfterBind) {
        fprintf(stderr,
                "device 0x%04X lacks G-buffer descriptor indexing "
                "(non_uniform=%u update_after_bind=%u)\n",
                properties->deviceID,
                supported.shaderSampledImageArrayNonUniformIndexing,
                supported.descriptorBindingSampledImageUpdateAfterBind);
        exit(1);
    }

    VkPhysicalDeviceDescriptorIndexingProperties indexing = {
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_INDEXING_PROPERTIES,
    };
    VkPhysicalDeviceProperties2 properties2 = {
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PROPERTIES_2,
        .pNext = &indexing,
    };
    vkGetPhysicalDeviceProperties2(physical_device, &properties2);
    if (properties->limits.maxColorAttachments < GBUFFER_COLOR_TARGET_COUNT ||
        indexing.maxPerStageDescriptorUpdateAfterBindSamplers < MATERIAL_TEXTURE_COUNT ||
        indexing.maxPerStageDescriptorUpdateAfterBindSampledImages < MATERIAL_TEXTURE_COUNT) {
        fprintf(stderr,
                "device 0x%04X cannot represent Helio G-buffer ABI "
                "(color_targets=%u update_after_bind_samplers=%u images=%u)\n",
                properties->deviceID,
                properties->limits.maxColorAttachments,
                indexing.maxPerStageDescriptorUpdateAfterBindSamplers,
                indexing.maxPerStageDescriptorUpdateAfterBindSampledImages);
        exit(1);
    }
    printf(
        "helio_gbuffer_dump: limits color_targets=%u "
        "update_after_bind_samplers=%u update_after_bind_images=%u\n",
        properties->limits.maxColorAttachments,
        indexing.maxPerStageDescriptorUpdateAfterBindSamplers,
        indexing.maxPerStageDescriptorUpdateAfterBindSampledImages);

    *enabled_features = (VkPhysicalDeviceDescriptorIndexingFeatures){
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_INDEXING_FEATURES,
        .shaderSampledImageArrayNonUniformIndexing = VK_TRUE,
        .descriptorBindingSampledImageUpdateAfterBind = VK_TRUE,
    };
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s gbuffer.vs.spv gbuffer.fs.spv\n", argv[0]);
        return 2;
    }

    const VkApplicationInfo app_info = {
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "trueos-helio-gbuffer-dump",
        .applicationVersion = VK_MAKE_VERSION(1, 0, 0),
        .pEngineName = "Helio",
        .engineVersion = VK_MAKE_VERSION(1, 0, 0),
        .apiVersion = VK_API_VERSION_1_2,
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
    if (!physical_devices) {
        fprintf(stderr, "no Vulkan physical devices\n");
        return 1;
    }
    CHECK_VK(vkEnumeratePhysicalDevices(instance, &physical_count, physical_devices));

    VkPhysicalDevice physical_device = VK_NULL_HANDLE;
    VkPhysicalDeviceProperties selected = { 0 };
    uint32_t graphics_family = UINT32_MAX;
    int enable_descriptor_indexing_extension = 0;
    for (uint32_t i = 0; i < physical_count; ++i) {
        VkPhysicalDeviceProperties properties;
        vkGetPhysicalDeviceProperties(physical_devices[i], &properties);
        printf(
            "helio_gbuffer_dump: physical[%u] vendor=0x%04X device=0x%04X "
            "type=%s api=0x%08X driver=0x%08X name=\"%s\"\n",
            i, properties.vendorID, properties.deviceID,
            device_type_name(properties.deviceType), properties.apiVersion,
            properties.driverVersion, properties.deviceName);
        if (properties.vendorID != 0x8086 || !physical_device_matches_selector(&properties) ||
            !physical_device_has_extension(
                physical_devices[i], VK_KHR_PIPELINE_EXECUTABLE_PROPERTIES_EXTENSION_NAME)) {
            continue;
        }
        const int descriptor_indexing_is_core = properties.apiVersion >= VK_API_VERSION_1_2;
        const int has_descriptor_indexing_extension = physical_device_has_extension(
            physical_devices[i], VK_EXT_DESCRIPTOR_INDEXING_EXTENSION_NAME);
        if (!descriptor_indexing_is_core && !has_descriptor_indexing_extension) {
            continue;
        }
        uint32_t queue_count = 0;
        vkGetPhysicalDeviceQueueFamilyProperties(physical_devices[i], &queue_count, NULL);
        VkQueueFamilyProperties *queues = calloc(queue_count, sizeof(*queues));
        vkGetPhysicalDeviceQueueFamilyProperties(physical_devices[i], &queue_count, queues);
        for (uint32_t queue = 0; queue < queue_count; ++queue) {
            if (queues[queue].queueFlags & VK_QUEUE_GRAPHICS_BIT) {
                physical_device = physical_devices[i];
                selected = properties;
                graphics_family = queue;
                enable_descriptor_indexing_extension = !descriptor_indexing_is_core;
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
        fprintf(stderr, "failed to find an Intel Vulkan 1.2 descriptor-indexing graphics queue\n");
        return 1;
    }
    printf(
        "helio_gbuffer_dump: selected vendor=0x%04X device=0x%04X "
        "queue_family=%u name=\"%s\"\n",
        selected.vendorID, selected.deviceID, graphics_family, selected.deviceName);

    VkPhysicalDeviceDescriptorIndexingFeatures descriptor_indexing;
    require_descriptor_indexing(physical_device, &selected, &descriptor_indexing);
    const float queue_priority = 1.0f;
    const VkDeviceQueueCreateInfo queue_info = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = graphics_family,
        .queueCount = 1,
        .pQueuePriorities = &queue_priority,
    };
    const char *extensions[2] = {
        VK_KHR_PIPELINE_EXECUTABLE_PROPERTIES_EXTENSION_NAME,
        VK_EXT_DESCRIPTOR_INDEXING_EXTENSION_NAME,
    };
    const VkPhysicalDevicePipelineExecutablePropertiesFeaturesKHR executable_features = {
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PIPELINE_EXECUTABLE_PROPERTIES_FEATURES_KHR,
        .pNext = &descriptor_indexing,
        .pipelineExecutableInfo = VK_TRUE,
    };
    const VkDeviceCreateInfo device_info = {
        .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        .pNext = &executable_features,
        .queueCreateInfoCount = 1,
        .pQueueCreateInfos = &queue_info,
        .enabledExtensionCount = enable_descriptor_indexing_extension ? 2u : 1u,
        .ppEnabledExtensionNames = extensions,
    };
    VkDevice device;
    CHECK_VK(vkCreateDevice(physical_device, &device_info, NULL, &device));

    const VkFormat color_formats[GBUFFER_COLOR_TARGET_COUNT] = {
        VK_FORMAT_R8G8B8A8_UNORM,
        VK_FORMAT_R16G16B16A16_SFLOAT,
        VK_FORMAT_R8G8B8A8_UNORM,
        VK_FORMAT_R16G16B16A16_SFLOAT,
        VK_FORMAT_R16G16_SFLOAT,
        VK_FORMAT_R16G16B16A16_SFLOAT,
        VK_FORMAT_R16G16B16A16_SFLOAT,
        VK_FORMAT_R16G16_SFLOAT,
    };
    VkAttachmentDescription attachments[GBUFFER_COLOR_TARGET_COUNT + 1] = { 0 };
    VkAttachmentReference color_references[GBUFFER_COLOR_TARGET_COUNT];
    for (uint32_t i = 0; i < GBUFFER_COLOR_TARGET_COUNT; ++i) {
        attachments[i] = (VkAttachmentDescription){
            .format = color_formats[i],
            .samples = VK_SAMPLE_COUNT_1_BIT,
            .loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR,
            .storeOp = VK_ATTACHMENT_STORE_OP_STORE,
            .stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE,
            .stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE,
            .initialLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
            .finalLayout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
        color_references[i] = (VkAttachmentReference){
            .attachment = i,
            .layout = VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
    }
    attachments[GBUFFER_COLOR_TARGET_COUNT] = (VkAttachmentDescription){
        .format = VK_FORMAT_D32_SFLOAT,
        .samples = VK_SAMPLE_COUNT_1_BIT,
        .loadOp = VK_ATTACHMENT_LOAD_OP_CLEAR,
        .storeOp = VK_ATTACHMENT_STORE_OP_STORE,
        .stencilLoadOp = VK_ATTACHMENT_LOAD_OP_DONT_CARE,
        .stencilStoreOp = VK_ATTACHMENT_STORE_OP_DONT_CARE,
        .initialLayout = VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        .finalLayout = VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
    };
    const VkAttachmentReference depth_reference = {
        .attachment = GBUFFER_COLOR_TARGET_COUNT,
        .layout = VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
    };
    const VkSubpassDescription subpass = {
        .pipelineBindPoint = VK_PIPELINE_BIND_POINT_GRAPHICS,
        .colorAttachmentCount = GBUFFER_COLOR_TARGET_COUNT,
        .pColorAttachments = color_references,
        .pDepthStencilAttachment = &depth_reference,
    };
    const VkRenderPassCreateInfo render_pass_info = {
        .sType = VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
        .attachmentCount = GBUFFER_COLOR_TARGET_COUNT + 1,
        .pAttachments = attachments,
        .subpassCount = 1,
        .pSubpasses = &subpass,
    };
    VkRenderPass render_pass;
    CHECK_VK(vkCreateRenderPass(device, &render_pass_info, NULL, &render_pass));

    FileData vertex_spirv = read_spirv(argv[1]);
    FileData fragment_spirv = read_spirv(argv[2]);
    const VkShaderModuleCreateInfo vertex_module_info = {
        .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = vertex_spirv.word_count * sizeof(uint32_t),
        .pCode = vertex_spirv.words,
    };
    const VkShaderModuleCreateInfo fragment_module_info = {
        .sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
        .codeSize = fragment_spirv.word_count * sizeof(uint32_t),
        .pCode = fragment_spirv.words,
    };
    VkShaderModule vertex_module;
    VkShaderModule fragment_module;
    CHECK_VK(vkCreateShaderModule(device, &vertex_module_info, NULL, &vertex_module));
    CHECK_VK(vkCreateShaderModule(device, &fragment_module_info, NULL, &fragment_module));
    const VkPipelineShaderStageCreateInfo stages[2] = {
        {
            .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            .stage = VK_SHADER_STAGE_VERTEX_BIT,
            .module = vertex_module,
            .pName = "vs_main",
        },
        {
            .sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
            .stage = VK_SHADER_STAGE_FRAGMENT_BIT,
            .module = fragment_module,
            .pName = "fs_main",
        },
    };

    const VkVertexInputBindingDescription vertex_binding = {
        .binding = 0,
        .stride = 40,
        .inputRate = VK_VERTEX_INPUT_RATE_VERTEX,
    };
    const VkVertexInputAttributeDescription vertex_attributes[6] = {
        { .location = 0, .binding = 0, .format = VK_FORMAT_R32G32B32_SFLOAT, .offset = 0 },
        { .location = 1, .binding = 0, .format = VK_FORMAT_R32_SFLOAT, .offset = 12 },
        { .location = 2, .binding = 0, .format = VK_FORMAT_R32G32_SFLOAT, .offset = 16 },
        { .location = 5, .binding = 0, .format = VK_FORMAT_R32G32_SFLOAT, .offset = 24 },
        { .location = 3, .binding = 0, .format = VK_FORMAT_R32_UINT, .offset = 32 },
        { .location = 4, .binding = 0, .format = VK_FORMAT_R32_UINT, .offset = 36 },
    };
    const VkPipelineVertexInputStateCreateInfo vertex_input = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
        .vertexBindingDescriptionCount = 1,
        .pVertexBindingDescriptions = &vertex_binding,
        .vertexAttributeDescriptionCount = 6,
        .pVertexAttributeDescriptions = vertex_attributes,
    };
    const VkPipelineInputAssemblyStateCreateInfo input_assembly = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
        .topology = VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
    };
    const VkViewport viewport = {
        .width = 64.0f,
        .height = 64.0f,
        .minDepth = 0.0f,
        .maxDepth = 1.0f,
    };
    const VkRect2D scissor = { .extent = { 64, 64 } };
    const VkPipelineViewportStateCreateInfo viewport_state = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
        .viewportCount = 1,
        .pViewports = &viewport,
        .scissorCount = 1,
        .pScissors = &scissor,
    };
    const VkPipelineRasterizationStateCreateInfo rasterization = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
        .polygonMode = VK_POLYGON_MODE_FILL,
        .cullMode = VK_CULL_MODE_BACK_BIT,
        .frontFace = VK_FRONT_FACE_COUNTER_CLOCKWISE,
        .lineWidth = 1.0f,
    };
    const VkPipelineMultisampleStateCreateInfo multisample = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
        .rasterizationSamples = VK_SAMPLE_COUNT_1_BIT,
    };
    VkPipelineColorBlendAttachmentState blend_attachments[GBUFFER_COLOR_TARGET_COUNT] = { 0 };
    for (uint32_t i = 0; i < GBUFFER_COLOR_TARGET_COUNT; ++i) {
        blend_attachments[i].colorWriteMask =
            VK_COLOR_COMPONENT_R_BIT | VK_COLOR_COMPONENT_G_BIT |
            VK_COLOR_COMPONENT_B_BIT | VK_COLOR_COMPONENT_A_BIT;
    }
    const VkPipelineColorBlendStateCreateInfo color_blend = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
        .attachmentCount = GBUFFER_COLOR_TARGET_COUNT,
        .pAttachments = blend_attachments,
    };
    const VkPipelineDepthStencilStateCreateInfo depth_stencil = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO,
        .depthTestEnable = VK_TRUE,
        .depthWriteEnable = VK_TRUE,
        .depthCompareOp = VK_COMPARE_OP_LESS_OR_EQUAL,
    };

    const VkDescriptorSetLayoutBinding group0_bindings[9] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 3, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 4, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 5, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 6, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 7, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
        { .binding = 8, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_VERTEX_BIT },
    };
    const VkDescriptorSetLayoutCreateInfo group0_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .bindingCount = 9,
        .pBindings = group0_bindings,
    };
    VkDescriptorSetLayout set_layouts[2];
    CHECK_VK(vkCreateDescriptorSetLayout(device, &group0_info, NULL, &set_layouts[0]));

    const VkDescriptorSetLayoutBinding group1_bindings[4] = {
        { .binding = 0, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 1, .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .descriptorCount = 1,
          .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 2, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLED_IMAGE,
          .descriptorCount = MATERIAL_TEXTURE_COUNT, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
        { .binding = 3, .descriptorType = VK_DESCRIPTOR_TYPE_SAMPLER,
          .descriptorCount = MATERIAL_TEXTURE_COUNT, .stageFlags = VK_SHADER_STAGE_FRAGMENT_BIT },
    };
    const VkDescriptorBindingFlags group1_binding_flags[4] = {
        0,
        0,
        VK_DESCRIPTOR_BINDING_UPDATE_AFTER_BIND_BIT,
        VK_DESCRIPTOR_BINDING_UPDATE_AFTER_BIND_BIT,
    };
    const VkDescriptorSetLayoutBindingFlagsCreateInfo group1_flags_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_BINDING_FLAGS_CREATE_INFO,
        .bindingCount = 4,
        .pBindingFlags = group1_binding_flags,
    };
    const VkDescriptorSetLayoutCreateInfo group1_info = {
        .sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
        .pNext = &group1_flags_info,
        .flags = VK_DESCRIPTOR_SET_LAYOUT_CREATE_UPDATE_AFTER_BIND_POOL_BIT,
        .bindingCount = 4,
        .pBindings = group1_bindings,
    };
    CHECK_VK(vkCreateDescriptorSetLayout(device, &group1_info, NULL, &set_layouts[1]));
    const VkPipelineLayoutCreateInfo pipeline_layout_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
        .setLayoutCount = 2,
        .pSetLayouts = set_layouts,
    };
    VkPipelineLayout pipeline_layout;
    CHECK_VK(vkCreatePipelineLayout(device, &pipeline_layout_info, NULL, &pipeline_layout));

    const VkPipelineCacheCreateInfo cache_info = {
        .sType = VK_STRUCTURE_TYPE_PIPELINE_CACHE_CREATE_INFO,
    };
    VkPipelineCache pipeline_cache;
    CHECK_VK(vkCreatePipelineCache(device, &cache_info, NULL, &pipeline_cache));
    const VkGraphicsPipelineCreateInfo pipeline_info = {
        .sType = VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
        .flags = VK_PIPELINE_CREATE_CAPTURE_STATISTICS_BIT_KHR |
                 VK_PIPELINE_CREATE_CAPTURE_INTERNAL_REPRESENTATIONS_BIT_KHR,
        .stageCount = 2,
        .pStages = stages,
        .pVertexInputState = &vertex_input,
        .pInputAssemblyState = &input_assembly,
        .pViewportState = &viewport_state,
        .pRasterizationState = &rasterization,
        .pMultisampleState = &multisample,
        .pDepthStencilState = &depth_stencil,
        .pColorBlendState = &color_blend,
        .layout = pipeline_layout,
        .renderPass = render_pass,
        .subpass = 0,
    };
    VkPipeline pipeline;
    CHECK_VK(vkCreateGraphicsPipelines(
        device, pipeline_cache, 1, &pipeline_info, NULL, &pipeline));
    dump_pipeline_cache_blob(device, pipeline_cache);
    dump_pipeline_executables(device, pipeline);
    printf(
        "helio_gbuffer_dump: compiled_only=1 vertex_stride=40 "
        "sets=2 textures=256 color_targets=8 depth=VK_FORMAT_D32_SFLOAT\n");

    vkDestroyPipeline(device, pipeline, NULL);
    vkDestroyPipelineCache(device, pipeline_cache, NULL);
    vkDestroyPipelineLayout(device, pipeline_layout, NULL);
    vkDestroyDescriptorSetLayout(device, set_layouts[1], NULL);
    vkDestroyDescriptorSetLayout(device, set_layouts[0], NULL);
    vkDestroyShaderModule(device, fragment_module, NULL);
    vkDestroyShaderModule(device, vertex_module, NULL);
    vkDestroyRenderPass(device, render_pass, NULL);
    vkDestroyDevice(device, NULL);
    vkDestroyInstance(instance, NULL);
    free(fragment_spirv.words);
    free(vertex_spirv.words);
    return 0;
}
