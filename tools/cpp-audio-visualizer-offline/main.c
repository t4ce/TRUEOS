#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <math.h>
#include <png.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*
 * Minimal OpenCL declarations keep the replay independent of an OpenCL
 * development-header package.
 */
typedef int32_t cl_int;
typedef uint32_t cl_uint;
typedef uint64_t cl_ulong;
typedef cl_ulong cl_bitfield;
typedef cl_bitfield cl_device_type;
typedef cl_bitfield cl_mem_flags;
typedef cl_bitfield cl_command_queue_properties;
typedef intptr_t cl_context_properties;
typedef struct _cl_platform_id *cl_platform_id;
typedef struct _cl_device_id *cl_device_id;
typedef struct _cl_context *cl_context;
typedef struct _cl_command_queue *cl_command_queue;
typedef struct _cl_mem *cl_mem;
typedef struct _cl_program *cl_program;
typedef struct _cl_kernel *cl_kernel;
typedef struct _cl_event *cl_event;

#define CL_SUCCESS 0
#define CL_DEVICE_NOT_FOUND -1
#define CL_TRUE 1
#define CL_DEVICE_TYPE_GPU (1ULL << 2)
#define CL_MEM_READ_WRITE (1ULL << 0)
#define CL_MEM_READ_ONLY (1ULL << 2)
#define CL_MEM_COPY_HOST_PTR (1ULL << 5)
#define CL_QUEUE_PROFILING_ENABLE (1ULL << 1)
#define CL_PLATFORM_NAME 0x0902
#define CL_DEVICE_NAME 0x102B
#define CL_CONTEXT_PLATFORM 0x1084
#define CL_PROGRAM_BUILD_LOG 0x1183
#define CL_PROFILING_COMMAND_START 0x1282
#define CL_PROFILING_COMMAND_END 0x1283

extern cl_int clGetPlatformIDs(cl_uint, cl_platform_id *, cl_uint *);
extern cl_int clGetPlatformInfo(cl_platform_id, cl_uint, size_t, void *, size_t *);
extern cl_int clGetDeviceIDs(cl_platform_id, cl_device_type, cl_uint, cl_device_id *, cl_uint *);
extern cl_int clGetDeviceInfo(cl_device_id, cl_uint, size_t, void *, size_t *);
extern cl_context clCreateContext(const cl_context_properties *, cl_uint, const cl_device_id *,
                                  void (*)(const char *, const void *, size_t, void *), void *,
                                  cl_int *);
extern cl_command_queue clCreateCommandQueue(cl_context, cl_device_id,
                                             cl_command_queue_properties, cl_int *);
extern cl_program clCreateProgramWithIL(cl_context, const void *, size_t, cl_int *);
extern cl_int clBuildProgram(cl_program, cl_uint, const cl_device_id *, const char *,
                             void (*)(cl_program, void *), void *);
extern cl_int clGetProgramBuildInfo(cl_program, cl_device_id, cl_uint, size_t, void *, size_t *);
extern cl_kernel clCreateKernel(cl_program, const char *, cl_int *);
extern cl_mem clCreateBuffer(cl_context, cl_mem_flags, size_t, void *, cl_int *);
extern cl_int clSetKernelArg(cl_kernel, cl_uint, size_t, const void *);
extern cl_int clEnqueueNDRangeKernel(cl_command_queue, cl_kernel, cl_uint, const size_t *,
                                    const size_t *, const size_t *, cl_uint, const cl_event *,
                                    cl_event *);
extern cl_int clEnqueueReadBuffer(cl_command_queue, cl_mem, cl_uint, size_t, size_t, void *,
                                  cl_uint, const cl_event *, cl_event *);
extern cl_int clFinish(cl_command_queue);
extern cl_int clGetEventProfilingInfo(cl_event, cl_uint, size_t, void *, size_t *);
extern cl_int clReleaseEvent(cl_event);
extern cl_int clReleaseMemObject(cl_mem);
extern cl_int clReleaseKernel(cl_kernel);
extern cl_int clReleaseProgram(cl_program);
extern cl_int clReleaseCommandQueue(cl_command_queue);
extern cl_int clReleaseContext(cl_context);

enum {
    SNAPSHOT_DWORDS = 1024,
    FEATURE_BASE = 8,
    WAVEFORM_BASE = 32,
    WAVEFORM_COUNT = 128,
    SPECTRUM_BASE = 320,
    SPECTRUM_COUNT = 64,
    PROFILE_RUNS = 5,
};

static const char *const SPIRV_PATH =
    "crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/"
    "cpp_audio_visualizer_rgba8.spv";

static void die(const char *message) {
    fprintf(stderr, "cpp-audio-visualizer-offline: %s\n", message);
    exit(EXIT_FAILURE);
}

static void check_cl(cl_int error, const char *operation) {
    if (error == CL_SUCCESS) {
        return;
    }
    fprintf(stderr, "cpp-audio-visualizer-offline: %s failed with OpenCL error %d\n",
            operation, error);
    exit(EXIT_FAILURE);
}

static uint8_t *read_file(const char *path, size_t *length_out) {
    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        fprintf(stderr, "cpp-audio-visualizer-offline: cannot open %s: %s\n",
                path, strerror(errno));
        exit(EXIT_FAILURE);
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        die("failed to seek SPIR-V");
    }
    long length = ftell(file);
    if (length <= 0 || fseek(file, 0, SEEK_SET) != 0) {
        die("failed to size SPIR-V");
    }
    uint8_t *bytes = malloc((size_t)length);
    if (bytes == NULL) {
        die("out of host memory for SPIR-V");
    }
    if (fread(bytes, 1, (size_t)length, file) != (size_t)length) {
        die("failed to read SPIR-V");
    }
    fclose(file);
    *length_out = (size_t)length;
    return bytes;
}

static void info_string_platform(cl_platform_id platform, cl_uint key,
                                 char *out, size_t out_bytes) {
    if (clGetPlatformInfo(platform, key, out_bytes, out, NULL) != CL_SUCCESS) {
        snprintf(out, out_bytes, "unknown-platform");
    }
    out[out_bytes - 1] = '\0';
}

static void info_string_device(cl_device_id device, cl_uint key,
                               char *out, size_t out_bytes) {
    if (clGetDeviceInfo(device, key, out_bytes, out, NULL) != CL_SUCCESS) {
        snprintf(out, out_bytes, "unknown-device");
    }
    out[out_bytes - 1] = '\0';
}

static cl_device_id pick_device(cl_platform_id *platform_out) {
    cl_uint count = 0;
    cl_int platform_error = clGetPlatformIDs(0, NULL, &count);
    if (platform_error != CL_SUCCESS || count == 0) {
        *platform_out = NULL;
        return NULL;
    }
    cl_platform_id *platforms = calloc(count, sizeof(*platforms));
    if (platforms == NULL) {
        die("out of host memory for OpenCL platforms");
    }
    check_cl(clGetPlatformIDs(count, platforms, NULL), "clGetPlatformIDs(list)");

    cl_device_id fallback = NULL;
    cl_platform_id fallback_platform = NULL;
    for (cl_uint index = 0; index < count; ++index) {
        cl_device_id device = NULL;
        cl_int error =
            clGetDeviceIDs(platforms[index], CL_DEVICE_TYPE_GPU, 1, &device, NULL);
        if (error == CL_DEVICE_NOT_FOUND) {
            continue;
        }
        check_cl(error, "clGetDeviceIDs");
        if (fallback == NULL) {
            fallback = device;
            fallback_platform = platforms[index];
        }
        char name[256];
        info_string_platform(platforms[index], CL_PLATFORM_NAME, name, sizeof(name));
        if (strstr(name, "Intel") != NULL) {
            *platform_out = platforms[index];
            free(platforms);
            return device;
        }
    }
    free(platforms);
    if (fallback != NULL) {
        *platform_out = fallback_platform;
        return fallback;
    }
    die("no GPU OpenCL device found");
    return NULL;
}

static uint32_t float_bits(float value) {
    uint32_t bits = 0;
    memcpy(&bits, &value, sizeof(bits));
    return bits;
}

static void fill_snapshot(uint32_t words[SNAPSHOT_DWORDS]) {
    memset(words, 0, SNAPSHOT_DWORDS * sizeof(*words));
    words[0] = 0x315A5641U; /* AVZ1 */
    words[1] = 1;
    words[2] = 684000;
    words[3] = 1;
    words[4] = 48000;
    words[5] = SPECTRUM_COUNT;
    words[6] = WAVEFORM_COUNT;

    const float features[] = {
        0.72f, /* rms L */
        0.66f, /* rms R */
        0.91f, /* peak */
        0.58f, /* stereo width */
        0.79f, /* low */
        0.53f, /* mid */
        0.41f, /* high */
        0.82f, /* beat */
        0.47f, /* centroid */
        0.69f, /* flux */
        0.31f, /* tempo phase */
        0.88f, /* signal */
    };
    for (size_t index = 0; index < sizeof(features) / sizeof(features[0]); ++index) {
        words[FEATURE_BASE + index] = float_bits(features[index]);
    }

    const float tau = 6.28318530718f;
    for (unsigned index = 0; index < WAVEFORM_COUNT; ++index) {
        float x = (float)index / (float)(WAVEFORM_COUNT - 1);
        float envelope = 0.74f + 0.26f * sinf(tau * x * 2.0f);
        float left = envelope
            * (0.42f * sinf(tau * (x * 3.0f + 0.08f))
               + 0.22f * sinf(tau * (x * 11.0f + 0.31f))
               + 0.10f * sinf(tau * (x * 29.0f + 0.17f)));
        float right = envelope
            * (0.38f * sinf(tau * (x * 3.0f + 0.19f))
               + 0.19f * sinf(tau * (x * 13.0f + 0.47f))
               + 0.12f * sinf(tau * (x * 23.0f + 0.03f)));
        words[WAVEFORM_BASE + index * 2] = float_bits(left);
        words[WAVEFORM_BASE + index * 2 + 1] = float_bits(right);
    }

    for (unsigned index = 0; index < SPECTRUM_COUNT; ++index) {
        float x = (float)index / (float)(SPECTRUM_COUNT - 1);
        float bass = 0.82f * expf(-3.3f * x);
        float kick = 0.46f * expf(-180.0f * (x - 0.10f) * (x - 0.10f));
        float vocal = 0.52f * expf(-54.0f * (x - 0.43f) * (x - 0.43f));
        float hat = 0.30f * expf(-80.0f * (x - 0.78f) * (x - 0.78f));
        float ripple = 0.06f + 0.09f * (0.5f + 0.5f * sinf(tau * x * 9.0f));
        float magnitude = fminf(1.0f, bass + kick + vocal + hat + ripple);
        words[SPECTRUM_BASE + index] = float_bits(magnitude);
    }
}

typedef struct {
    float x;
    float y;
} Float2;

typedef struct {
    float x;
    float y;
    float z;
} Float3;

static float saturate(float value) {
    return fminf(1.0f, fmaxf(0.0f, value));
}

static float mix_float(float a, float b, float blend) {
    return a + (b - a) * blend;
}

static float smoothstep_float(float edge0, float edge1, float value) {
    float t = saturate((value - edge0) / (edge1 - edge0));
    return t * t * (3.0f - 2.0f * t);
}

static float snapshot_float(const uint32_t snapshot[SNAPSHOT_DWORDS],
                            unsigned word) {
    float value = 0.0f;
    memcpy(&value, &snapshot[word], sizeof(value));
    return isfinite(value) ? value : 0.0f;
}

static float snapshot_feature(const uint32_t snapshot[SNAPSHOT_DWORDS],
                              unsigned index) {
    return saturate(snapshot_float(snapshot, FEATURE_BASE + index));
}

static float snapshot_waveform(const uint32_t snapshot[SNAPSHOT_DWORDS],
                               float position, unsigned channel) {
    float scaled = saturate(position) * (float)(WAVEFORM_COUNT - 1);
    unsigned left = (unsigned)scaled;
    unsigned right = left + 1 < WAVEFORM_COUNT ? left + 1 : left;
    unsigned lane = channel > 1 ? 1 : channel;
    float value =
        mix_float(snapshot_float(snapshot, WAVEFORM_BASE + left * 2 + lane),
                  snapshot_float(snapshot, WAVEFORM_BASE + right * 2 + lane),
                  scaled - (float)left);
    return fminf(1.0f, fmaxf(-1.0f, value));
}

static float snapshot_spectrum(const uint32_t snapshot[SNAPSHOT_DWORDS],
                               float position) {
    float scaled = saturate(position) * (float)(SPECTRUM_COUNT - 1);
    unsigned left = (unsigned)scaled;
    unsigned right = left + 1 < SPECTRUM_COUNT ? left + 1 : left;
    return saturate(mix_float(snapshot_float(snapshot, SPECTRUM_BASE + left),
                              snapshot_float(snapshot, SPECTRUM_BASE + right),
                              scaled - (float)left));
}

static uint32_t hash_u32(uint32_t value) {
    value ^= value >> 16;
    value *= 0x7FEB352DU;
    value ^= value >> 15;
    value *= 0x846CA68BU;
    value ^= value >> 16;
    return value;
}

static float hash_unit(int x, int y, uint32_t seed) {
    uint32_t mixed = hash_u32((uint32_t)x * 0x9E3779B9U
                              ^ (uint32_t)y * 0x85EBCA6BU
                              ^ seed);
    return (float)(mixed & 0x00FFFFFFU) * (1.0f / 16777216.0f);
}

static Float3 palette(float phase) {
    const float tau = 6.28318530718f;
    return (Float3){
        0.5f + 0.5f * cosf(tau * (phase + 0.00f)),
        0.5f + 0.5f * cosf(tau * (phase + 0.32f)),
        0.5f + 0.5f * cosf(tau * (phase + 0.67f)),
    };
}

static float line_glow(float distance, float sharpness) {
    return expf(-sharpness * fabsf(distance));
}

static void add_scaled(Float3 *color, Float3 source, float scale) {
    color->x += source.x * scale;
    color->y += source.y * scale;
    color->z += source.z * scale;
}

static Float3 render_reference_pixel(
    const uint32_t snapshot[SNAPSHOT_DWORDS],
    Float2 normalized, float aspect, float time_seconds, uint32_t frame) {
    const float tau = 6.28318530718f;
    Float2 centered = {normalized.x * 2.0f - 1.0f,
                       normalized.y * 2.0f - 1.0f};
    Float2 uv = {centered.x * aspect, centered.y};
    float radius = sqrtf(uv.x * uv.x + uv.y * uv.y);
    float angle = atan2f(uv.y, uv.x);
    float angle01 = (angle + 3.14159265359f) / tau;

    float rms_left = snapshot_feature(snapshot, 0);
    float rms_right = snapshot_feature(snapshot, 1);
    float peak = snapshot_feature(snapshot, 2);
    float stereo = snapshot_feature(snapshot, 3);
    float low = snapshot_feature(snapshot, 4);
    float mid = snapshot_feature(snapshot, 5);
    float high = snapshot_feature(snapshot, 6);
    float beat = snapshot_feature(snapshot, 7);
    float centroid = snapshot_feature(snapshot, 8);
    float flux = snapshot_feature(snapshot, 9);
    float tempo = snapshot_feature(snapshot, 10);
    float signal = snapshot_feature(snapshot, 11);

    float band_x = snapshot_spectrum(snapshot, normalized.x);
    float angular_band = snapshot_spectrum(snapshot, angle01);
    float wave_left = snapshot_waveform(snapshot, normalized.x, 0);
    float wave_right = snapshot_waveform(snapshot, normalized.x, 1);
    float circular_wave =
        0.5f * (snapshot_waveform(snapshot, angle01, 0)
                + snapshot_waveform(snapshot, angle01, 1));

    float flow = sinf(uv.x * 2.7f + time_seconds * 0.23f + low * 4.0f)
        + cosf(uv.y * 3.1f - time_seconds * 0.19f + mid * 3.0f)
        + sinf((uv.x + uv.y) * 1.9f + centroid * tau);
    Float3 color = {0.0035f, 0.006f, 0.018f};
    add_scaled(&color, palette(0.62f + centroid * 0.28f + flow * 0.025f),
               (0.012f + 0.026f * signal + 0.020f * low)
                   * (1.0f - saturate(radius * 0.52f)));

    float lower_height = 0.08f + band_x * (0.18f + 0.42f * signal);
    float upper_height = 0.04f + band_x * (0.10f + 0.25f * high);
    float lower_edge =
        line_glow(normalized.y - (1.0f - lower_height), 170.0f);
    float upper_edge = line_glow(normalized.y - upper_height, 190.0f);
    float lower_fill =
        smoothstep_float(1.0f - lower_height,
                         1.0f - lower_height - 0.018f, normalized.y);
    float upper_fill =
        smoothstep_float(upper_height, upper_height + 0.014f, normalized.y);
    Float3 spectral = palette(0.88f - normalized.x * 0.52f + centroid * 0.18f);
    add_scaled(&color, spectral,
               lower_fill * (0.006f + band_x * 0.070f) + lower_edge * 0.72f);
    add_scaled(&color, (Float3){spectral.z, spectral.x, spectral.y},
               upper_fill * (0.004f + band_x * 0.040f) + upper_edge * 0.35f);

    float left_path = -0.19f + wave_left * (0.08f + rms_left * 0.19f);
    float right_path = 0.19f + wave_right * (0.08f + rms_right * 0.19f);
    float left_ribbon = line_glow(uv.y - left_path, 135.0f);
    float right_ribbon = line_glow(uv.y - right_path, 135.0f);
    add_scaled(&color, (Float3){0.10f, 0.82f, 1.00f},
               left_ribbon * (0.25f + signal * 0.85f));
    add_scaled(&color, (Float3){1.00f, 0.16f, 0.68f},
               right_ribbon * (0.25f + signal * 0.85f));
    add_scaled(&color, (Float3){0.55f, 0.75f, 1.00f},
               line_glow(uv.y - 0.5f * (left_path + right_path), 42.0f)
                   * stereo * 0.20f);

    float prism_radius =
        0.40f + circular_wave * (0.035f + 0.11f * signal)
        + angular_band * 0.095f;
    float prism = line_glow(radius - prism_radius, 115.0f);
    float prism_halo = line_glow(radius - prism_radius, 24.0f);
    float spokes =
        0.52f + 0.48f * sinf(angle * 64.0f + time_seconds * 0.7f
                             + angular_band * 5.0f);
    Float3 prism_color =
        palette(angle01 + centroid * 0.35f + time_seconds * 0.012f);
    add_scaled(&color, prism_color, prism * (0.42f + angular_band * 1.35f));
    add_scaled(&color, prism_color,
               prism_halo * spokes * (0.04f + high * 0.16f));

    float phase_left = snapshot_waveform(snapshot, angle01, 0);
    float phase_right = snapshot_waveform(snapshot, angle01, 1);
    Float2 phase_point = {
        (phase_left + phase_right) * (0.20f + stereo * 0.16f),
        (phase_left - phase_right) * (0.20f + stereo * 0.16f),
    };
    float phase_dx = uv.x - phase_point.x;
    float phase_dy = uv.y - phase_point.y;
    float phase_glow =
        expf(-34.0f * sqrtf(phase_dx * phase_dx + phase_dy * phase_dy));
    add_scaled(&color, (Float3){0.72f, 0.46f, 1.00f},
               phase_glow * (0.05f + stereo * 0.26f));

    float beat_radius = 0.18f + 0.88f * tempo;
    float beat_ring = line_glow(radius - beat_radius, 95.0f);
    float bass_bloom = expf(-3.6f * radius * radius);
    add_scaled(&color, palette(0.02f + centroid * 0.22f),
               beat_ring * beat * (0.35f + flux * 1.4f));
    add_scaled(&color, (Float3){0.16f, 0.035f, 0.25f},
               bass_bloom * low * (0.28f + beat * 0.85f));

    Float2 particle_grid = {normalized.x * 72.0f, normalized.y * 40.5f};
    int cell_x = (int)floorf(particle_grid.x);
    int cell_y = (int)floorf(particle_grid.y);
    float particle_x = particle_grid.x - floorf(particle_grid.x) - 0.5f;
    float particle_y = particle_grid.y - floorf(particle_grid.y) - 0.5f;
    float particle_seed = hash_unit(cell_x, cell_y, frame >> 2);
    float particle_gate =
        particle_seed >= 0.985f - high * 0.045f - beat * 0.025f ? 1.0f : 0.0f;
    float particle =
        expf(-32.0f * (particle_x * particle_x + particle_y * particle_y))
        * particle_gate;
    add_scaled(&color, palette(particle_seed + centroid),
               particle * (0.18f + high * 0.72f));

    add_scaled(&color, (Float3){0.35f, 0.42f, 0.65f},
               expf(-5.0f * radius) * peak * peak * 0.16f);
    float texture =
        0.975f + 0.025f * sinf(normalized.y * 1440.0f * 0.42f);
    float vignette =
        saturate(1.13f - 0.28f
                 * (centered.x * centered.x + centered.y * centered.y));
    color.x *= texture * vignette;
    color.y *= texture * vignette;
    color.z *= texture * vignette;
    color.x = sqrtf(saturate(color.x / (color.x + 1.0f)));
    color.y = sqrtf(saturate(color.y / (color.y + 1.0f)));
    color.z = sqrtf(saturate(color.z / (color.z + 1.0f)));
    return color;
}

static void render_cpu_reference(
    uint8_t *rgba, uint32_t width, uint32_t height,
    float time_seconds, uint32_t frame,
    const uint32_t snapshot[SNAPSHOT_DWORDS]) {
    float aspect = (float)width / (float)height;
    for (uint32_t y = 0; y < height; ++y) {
        for (uint32_t pair = 0; pair < (width + 1) / 2; ++pair) {
            uint32_t x = pair * 2;
            uint32_t center_x = x + 1 < width ? x + 1 : width - 1;
            Float2 normalized = {
                ((float)center_x + 0.5f) / (float)width,
                ((float)y + 0.5f) / (float)height,
            };
            Float3 color = render_reference_pixel(
                snapshot, normalized, aspect, time_seconds, frame);
            uint8_t pixel[4] = {
                (uint8_t)(saturate(color.x) * 255.0f + 0.5f),
                (uint8_t)(saturate(color.y) * 255.0f + 0.5f),
                (uint8_t)(saturate(color.z) * 255.0f + 0.5f),
                255,
            };
            memcpy(rgba + ((size_t)y * width + x) * 4, pixel, sizeof(pixel));
            if (x + 1 < width) {
                memcpy(rgba + ((size_t)y * width + x + 1) * 4,
                       pixel, sizeof(pixel));
            }
        }
    }
}

static cl_program build_program(cl_context context, cl_device_id device) {
    size_t spirv_bytes = 0;
    uint8_t *spirv = read_file(SPIRV_PATH, &spirv_bytes);
    cl_int error = CL_SUCCESS;
    cl_program program = clCreateProgramWithIL(context, spirv, spirv_bytes, &error);
    check_cl(error, "clCreateProgramWithIL");
    free(spirv);
    error = clBuildProgram(program, 1, &device, NULL, NULL, NULL);
    if (error != CL_SUCCESS) {
        size_t log_bytes = 0;
        clGetProgramBuildInfo(program, device, CL_PROGRAM_BUILD_LOG, 0, NULL, &log_bytes);
        char *log = calloc(log_bytes + 1, 1);
        if (log != NULL) {
            clGetProgramBuildInfo(program, device, CL_PROGRAM_BUILD_LOG,
                                  log_bytes, log, NULL);
            fprintf(stderr, "OpenCL build log:\n%s\n", log);
            free(log);
        }
        check_cl(error, "clBuildProgram");
    }
    return program;
}

static double dispatch_profile_ms(cl_command_queue queue, cl_kernel kernel,
                                  const size_t global[2], const size_t local[2]) {
    cl_event event = NULL;
    check_cl(clEnqueueNDRangeKernel(queue, kernel, 2, NULL, global, local,
                                    0, NULL, &event),
             "clEnqueueNDRangeKernel");
    check_cl(clFinish(queue), "clFinish");
    cl_ulong start = 0;
    cl_ulong end = 0;
    check_cl(clGetEventProfilingInfo(event, CL_PROFILING_COMMAND_START,
                                     sizeof(start), &start, NULL),
             "clGetEventProfilingInfo(start)");
    check_cl(clGetEventProfilingInfo(event, CL_PROFILING_COMMAND_END,
                                     sizeof(end), &end, NULL),
             "clGetEventProfilingInfo(end)");
    check_cl(clReleaseEvent(event), "clReleaseEvent");
    return (double)(end - start) / 1000000.0;
}

static void write_png(const char *path, const uint8_t *rgba,
                      uint32_t width, uint32_t height) {
    png_image image;
    memset(&image, 0, sizeof(image));
    image.version = PNG_IMAGE_VERSION;
    image.width = width;
    image.height = height;
    image.format = PNG_FORMAT_RGBA;
    if (!png_image_write_to_file(&image, path, 0, rgba, 0, NULL)) {
        die("libpng failed to write output");
    }
}

static uint32_t parse_extent(const char *raw, const char *label) {
    char *end = NULL;
    unsigned long value = strtoul(raw, &end, 10);
    if (raw[0] == '\0' || *end != '\0' || value == 0 || value > 8192) {
        fprintf(stderr, "invalid %s: %s\n", label, raw);
        exit(EXIT_FAILURE);
    }
    return (uint32_t)value;
}

int main(int argc, char **argv) {
    const char *output = argc > 1 ? argv[1] : "bld/cpp-audio-visualizer-1440p.png";
    uint32_t width = argc > 2 ? parse_extent(argv[2], "width") : 2560;
    uint32_t height = argc > 3 ? parse_extent(argv[3], "height") : 1440;
    float time_seconds = argc > 4 ? strtof(argv[4], NULL) : 14.25f;
    uint32_t pitch = width * 4;
    size_t output_bytes = (size_t)pitch * height;
    uint8_t *rgba = calloc(output_bytes, 1);
    if (rgba == NULL) {
        die("out of host memory for output");
    }

    cl_platform_id platform = NULL;
    cl_device_id device = pick_device(&platform);
    uint32_t snapshot[SNAPSHOT_DWORDS];
    fill_snapshot(snapshot);
    uint32_t frame = 855;
    size_t pair_width = ((size_t)width + 1) / 2;
    if (device == NULL) {
        render_cpu_reference(rgba, width, height, time_seconds, frame, snapshot);
        write_png(output, rgba, width, height);
        printf("cpp-audio-visualizer replay: ok=1 renderer=cpu-reference "
               "hardware_replay=0 reason=no-opencl-gpu extent=%ux%u lanes=%zu "
               "full_pixel_lanes=%zu lane_ratio=0.5000 output=%s\n",
               width, height, pair_width * height, (size_t)width * height, output);
        free(rgba);
        return EXIT_SUCCESS;
    }
    char platform_name[256];
    char device_name[256];
    info_string_platform(platform, CL_PLATFORM_NAME, platform_name, sizeof(platform_name));
    info_string_device(device, CL_DEVICE_NAME, device_name, sizeof(device_name));

    const cl_context_properties properties[] = {
        CL_CONTEXT_PLATFORM, (cl_context_properties)platform, 0
    };
    cl_int error = CL_SUCCESS;
    cl_context context =
        clCreateContext(properties, 1, &device, NULL, NULL, &error);
    check_cl(error, "clCreateContext");
    cl_command_queue queue =
        clCreateCommandQueue(context, device, CL_QUEUE_PROFILING_ENABLE, &error);
    check_cl(error, "clCreateCommandQueue");
    cl_program program = build_program(context, device);
    cl_kernel kernel =
        clCreateKernel(program, "cpp_audio_visualizer_rgba8", &error);
    check_cl(error, "clCreateKernel");

    cl_mem audio = clCreateBuffer(context, CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR,
                                  sizeof(snapshot), snapshot, &error);
    check_cl(error, "clCreateBuffer(audio)");
    cl_mem destination =
        clCreateBuffer(context, CL_MEM_READ_WRITE, output_bytes, NULL, &error);
    check_cl(error, "clCreateBuffer(destination)");

    uint32_t flags = 1;
    check_cl(clSetKernelArg(kernel, 0, sizeof(audio), &audio), "clSetKernelArg(audio)");
    check_cl(clSetKernelArg(kernel, 1, sizeof(destination), &destination),
             "clSetKernelArg(destination)");
    check_cl(clSetKernelArg(kernel, 2, sizeof(pitch), &pitch), "clSetKernelArg(pitch)");
    check_cl(clSetKernelArg(kernel, 3, sizeof(width), &width), "clSetKernelArg(width)");
    check_cl(clSetKernelArg(kernel, 4, sizeof(height), &height), "clSetKernelArg(height)");
    check_cl(clSetKernelArg(kernel, 5, sizeof(time_seconds), &time_seconds),
             "clSetKernelArg(time)");
    check_cl(clSetKernelArg(kernel, 6, sizeof(frame), &frame), "clSetKernelArg(frame)");
    check_cl(clSetKernelArg(kernel, 7, sizeof(flags), &flags), "clSetKernelArg(flags)");

    const size_t local[2] = {16, 1};
    const size_t global[2] = {(pair_width + local[0] - 1) & ~(local[0] - 1),
                              (size_t)height};
    (void)dispatch_profile_ms(queue, kernel, global, local);
    double total_ms = 0.0;
    double minimum_ms = 1e30;
    double maximum_ms = 0.0;
    for (unsigned run = 0; run < PROFILE_RUNS; ++run) {
        double elapsed_ms = dispatch_profile_ms(queue, kernel, global, local);
        total_ms += elapsed_ms;
        minimum_ms = fmin(minimum_ms, elapsed_ms);
        maximum_ms = fmax(maximum_ms, elapsed_ms);
    }
    check_cl(clEnqueueReadBuffer(queue, destination, CL_TRUE, 0, output_bytes,
                                 rgba, 0, NULL, NULL),
             "clEnqueueReadBuffer");
    write_png(output, rgba, width, height);

    printf("cpp-audio-visualizer replay: ok=1 platform=\"%s\" device=\"%s\" "
           "extent=%ux%u lanes=%zu full_pixel_lanes=%zu lane_ratio=0.5000 "
           "cadence_ms=50 profile_runs=%u gpu_ms_avg=%.3f gpu_ms_min=%.3f "
           "gpu_ms_max=%.3f output=%s\n",
           platform_name, device_name, width, height, pair_width * height,
           (size_t)width * height, PROFILE_RUNS, total_ms / PROFILE_RUNS,
           minimum_ms, maximum_ms, output);

    check_cl(clReleaseMemObject(destination), "clReleaseMemObject(destination)");
    check_cl(clReleaseMemObject(audio), "clReleaseMemObject(audio)");
    check_cl(clReleaseKernel(kernel), "clReleaseKernel");
    check_cl(clReleaseProgram(program), "clReleaseProgram");
    check_cl(clReleaseCommandQueue(queue), "clReleaseCommandQueue");
    check_cl(clReleaseContext(context), "clReleaseContext");
    free(rgba);
    return EXIT_SUCCESS;
}
