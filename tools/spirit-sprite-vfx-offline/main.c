#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <math.h>
#include <png.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*
 * Minimal OpenCL 1.2 declarations keep this replay buildable with an ICD
 * loader even when development headers are not installed.
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

#define CL_SUCCESS 0
#define CL_DEVICE_NOT_FOUND -1
#define CL_TRUE 1
#define CL_DEVICE_TYPE_GPU (1ULL << 2)
#define CL_MEM_READ_WRITE (1ULL << 0)
#define CL_MEM_READ_ONLY (1ULL << 2)
#define CL_MEM_COPY_HOST_PTR (1ULL << 5)
#define CL_PLATFORM_NAME 0x0902
#define CL_DEVICE_NAME 0x102B
#define CL_CONTEXT_PLATFORM 0x1084
#define CL_PROGRAM_BUILD_LOG 0x1183

extern cl_int clGetPlatformIDs(cl_uint, cl_platform_id *, cl_uint *);
extern cl_int clGetPlatformInfo(cl_platform_id, cl_uint, size_t, void *, size_t *);
extern cl_int clGetDeviceIDs(
    cl_platform_id,
    cl_device_type,
    cl_uint,
    cl_device_id *,
    cl_uint *);
extern cl_int clGetDeviceInfo(cl_device_id, cl_uint, size_t, void *, size_t *);
extern cl_context clCreateContext(
    const cl_context_properties *,
    cl_uint,
    const cl_device_id *,
    void (*)(const char *, const void *, size_t, void *),
    void *,
    cl_int *);
extern cl_command_queue clCreateCommandQueue(
    cl_context,
    cl_device_id,
    cl_command_queue_properties,
    cl_int *);
extern cl_program clCreateProgramWithSource(
    cl_context,
    cl_uint,
    const char **,
    const size_t *,
    cl_int *);
extern cl_int clBuildProgram(
    cl_program,
    cl_uint,
    const cl_device_id *,
    const char *,
    void (*)(cl_program, void *),
    void *);
extern cl_int clGetProgramBuildInfo(
    cl_program,
    cl_device_id,
    cl_uint,
    size_t,
    void *,
    size_t *);
extern cl_kernel clCreateKernel(cl_program, const char *, cl_int *);
extern cl_mem clCreateBuffer(cl_context, cl_mem_flags, size_t, void *, cl_int *);
extern cl_int clSetKernelArg(cl_kernel, cl_uint, size_t, const void *);
extern cl_int clEnqueueNDRangeKernel(
    cl_command_queue,
    cl_kernel,
    cl_uint,
    const size_t *,
    const size_t *,
    const size_t *,
    cl_uint,
    const void *,
    void *);
extern cl_int clEnqueueReadBuffer(
    cl_command_queue,
    cl_mem,
    cl_uint,
    size_t,
    size_t,
    void *,
    cl_uint,
    const void *,
    void *);
extern cl_int clEnqueueWriteBuffer(
    cl_command_queue,
    cl_mem,
    cl_uint,
    size_t,
    size_t,
    const void *,
    cl_uint,
    const void *,
    void *);
extern cl_int clFinish(cl_command_queue);
extern cl_int clReleaseMemObject(cl_mem);
extern cl_int clReleaseKernel(cl_kernel);
extern cl_int clReleaseProgram(cl_program);
extern cl_int clReleaseCommandQueue(cl_command_queue);
extern cl_int clReleaseContext(cl_context);

enum {
    SPIRIT_SIZE = 256,
    CONTROL_DWORDS = 32,
    EFFECT_COUNT = 16,
    GRID_COLUMNS = 4,
    GRID_ROWS = 4,
    GRID_WIDTH = SPIRIT_SIZE * GRID_COLUMNS,
    GRID_HEIGHT = SPIRIT_SIZE * GRID_ROWS,
};

typedef struct {
    const char *name;
    float parameters[4];
    uint8_t color_a[3];
    uint8_t color_b[3];
} SpritePreset;

static const SpritePreset SPRITE_PRESETS[EFFECT_COUNT] = {
    {"original-clean", {0.0f, 0.0f, 0.0f, 0.0f},
     {0x9A, 0x7C, 0xFF}, {0x5E, 0xE7, 0xFF}},
    {"aura-bloom", {12.0f, 1.15f, 1.2f, 0.18f},
     {0x8D, 0x6C, 0xFF}, {0x5E, 0xE7, 0xFF}},
    {"neon-edge", {3.2f, 1.35f, 1.1f, 0.12f},
     {0xFF, 0x53, 0xD1}, {0x5E, 0xE7, 0xFF}},
    {"fire-rim", {3.1f, 16.0f, 1.7f, 1.25f},
     {0xFF, 0x4D, 0x2E}, {0xFF, 0xD3, 0x5A}},
    {"ice-shimmer", {3.4f, 1.2f, 10.0f, 0.28f},
     {0x70, 0xEA, 0xFF}, {0xD7, 0xFB, 0xFF}},
    {"hologram", {95.0f, 3.5f, 1.4f, 0.82f},
     {0x36, 0xE7, 0xFF}, {0x85, 0x6C, 0xFF}},
    {"rgb-glitch", {2.7f, 8.0f, 2.8f, 0.36f},
     {0xFF, 0x3F, 0x9F}, {0x39, 0xF4, 0xFF}},
    {"dissolve", {0.42f, 0.08f, 9.5f, 1.45f},
     {0xFF, 0x6A, 0x2B}, {0xFF, 0xE6, 0x6E}},
    {"ghost-trail", {11.0f, 0.8f, 1.2f, 0.68f},
     {0xB5, 0x96, 0xFF}, {0x59, 0xED, 0xFF}},
    {"electric-arc", {4.1f, 14.0f, 2.4f, 1.65f},
     {0x7B, 0x6C, 0xFF}, {0xD8, 0xFB, 0xFF}},
    {"rainbow-prism", {0.55f, 5.5f, 0.58f, 2.2f},
     {0xFF, 0x5C, 0xCF}, {0x58, 0xEA, 0xFF}},
    {"hit-flash", {0.82f, 2.2f, 4.5f, 1.5f},
     {0xFF, 0xFF, 0xFF}, {0xFF, 0x4F, 0x76}},
    {"pixel-wave", {3.2f, 11.0f, 1.7f, 7.0f},
     {0xA8, 0x79, 0xFF}, {0x50, 0xE7, 0xFF}},
    {"toon-ink", {6.0f, 1.7f, 1.18f, 1.2f},
     {0x3B, 0x27, 0x4F}, {0xD9, 0x4D, 0xFF}},
    {"liquid-warp", {4.3f, 7.5f, 1.1f, 1.6f},
     {0x57, 0xF0, 0xDE}, {0x8D, 0x6C, 0xFF}},
    {"dream-bloom", {13.0f, 0.9f, 0.65f, 0.28f},
     {0xFF, 0x8D, 0xDD}, {0x7D, 0xE8, 0xFF}},
};

static const char *const LILLY_FRAME =
    "Lilly/Idle/Crossed-Arms/idle-1_frames/frame_01.png";
static const char *const SPRITE_SOURCE =
    "crates/trueos-shader/gpgpu/kernels/spirit_vfx_sprite_rgba8.cl";

static void die(const char *message)
{
    fprintf(stderr, "spirit-sprite-vfx-offline: %s\n", message);
    exit(EXIT_FAILURE);
}

static void check_cl(cl_int error, const char *operation)
{
    if (error == CL_SUCCESS) {
        return;
    }
    fprintf(
        stderr,
        "spirit-sprite-vfx-offline: %s failed with OpenCL error %d\n",
        operation,
        error);
    exit(EXIT_FAILURE);
}

static char *read_text_file(const char *path, size_t *length_out)
{
    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        fprintf(stderr, "cannot open %s: %s\n", path, strerror(errno));
        exit(EXIT_FAILURE);
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        die("failed to seek shader source");
    }
    long length = ftell(file);
    if (length < 0 || fseek(file, 0, SEEK_SET) != 0) {
        die("failed to size shader source");
    }
    char *text = malloc((size_t)length + 1);
    if (text == NULL) {
        die("out of memory for shader source");
    }
    if (fread(text, 1, (size_t)length, file) != (size_t)length) {
        die("failed to read shader source");
    }
    fclose(file);
    text[length] = '\0';
    *length_out = (size_t)length;
    return text;
}

static uint8_t *extract_lilly(uint32_t *width_out, uint32_t *height_out)
{
    char command[256];
    int written = snprintf(
        command,
        sizeof(command),
        "7z x -so tools/Lilly.7z '%s' 2>/dev/null",
        LILLY_FRAME);
    if (written < 0 || (size_t)written >= sizeof(command)) {
        die("Lilly extraction command overflowed");
    }
    FILE *stream = popen(command, "r");
    if (stream == NULL) {
        die("failed to start 7z");
    }

    png_image image;
    memset(&image, 0, sizeof(image));
    image.version = PNG_IMAGE_VERSION;
    if (!png_image_begin_read_from_stdio(&image, stream)) {
        pclose(stream);
        die("libpng could not read Lilly");
    }
    image.format = PNG_FORMAT_RGBA;
    uint8_t *rgba = malloc(PNG_IMAGE_SIZE(image));
    if (rgba == NULL) {
        png_image_free(&image);
        pclose(stream);
        die("out of memory for Lilly");
    }
    if (!png_image_finish_read(&image, NULL, rgba, 0, NULL)) {
        free(rgba);
        png_image_free(&image);
        pclose(stream);
        die("libpng could not decode Lilly");
    }
    if (pclose(stream) != 0) {
        free(rgba);
        png_image_free(&image);
        die("7z failed while extracting Lilly");
    }
    *width_out = image.width;
    *height_out = image.height;
    png_image_free(&image);
    return rgba;
}

static cl_platform_id choose_platform(void)
{
    cl_uint count = 0;
    check_cl(clGetPlatformIDs(0, NULL, &count), "clGetPlatformIDs(count)");
    if (count == 0) {
        die("no OpenCL platforms found");
    }
    cl_platform_id *platforms = calloc(count, sizeof(*platforms));
    if (platforms == NULL) {
        die("out of memory for OpenCL platforms");
    }
    check_cl(clGetPlatformIDs(count, platforms, NULL), "clGetPlatformIDs(list)");

    cl_platform_id selected = NULL;
    for (cl_uint index = 0; index < count; ++index) {
        char name[256] = {0};
        cl_device_id device = NULL;
        clGetPlatformInfo(
            platforms[index], CL_PLATFORM_NAME, sizeof(name), name, NULL);
        if (strstr(name, "Intel") != NULL
            && clGetDeviceIDs(
                platforms[index],
                CL_DEVICE_TYPE_GPU,
                1,
                &device,
                NULL) == CL_SUCCESS) {
            selected = platforms[index];
            break;
        }
    }
    if (selected == NULL) {
        for (cl_uint index = 0; index < count; ++index) {
            cl_device_id device = NULL;
            if (clGetDeviceIDs(
                    platforms[index],
                    CL_DEVICE_TYPE_GPU,
                    1,
                    &device,
                    NULL) == CL_SUCCESS) {
                selected = platforms[index];
                break;
            }
        }
    }
    free(platforms);
    if (selected == NULL) {
        die("no OpenCL GPU device found");
    }
    return selected;
}

static cl_program build_program(
    cl_context context,
    cl_device_id device,
    const char *path)
{
    size_t source_length = 0;
    char *source = read_text_file(path, &source_length);
    cl_int error = CL_SUCCESS;
    const char *sources[] = {source};
    cl_program program = clCreateProgramWithSource(
        context, 1, sources, &source_length, &error);
    check_cl(error, "clCreateProgramWithSource");
    error = clBuildProgram(program, 1, &device, "-cl-std=CL1.2", NULL, NULL);
    if (error != CL_SUCCESS) {
        size_t log_length = 0;
        clGetProgramBuildInfo(
            program, device, CL_PROGRAM_BUILD_LOG, 0, NULL, &log_length);
        char *log = calloc(log_length + 1, 1);
        if (log != NULL) {
            clGetProgramBuildInfo(
                program,
                device,
                CL_PROGRAM_BUILD_LOG,
                log_length,
                log,
                NULL);
            fprintf(stderr, "OpenCL build log:\n%s\n", log);
            free(log);
        }
        check_cl(error, "clBuildProgram");
    }
    free(source);
    return program;
}

static uint32_t float_bits(float value)
{
    uint32_t bits = 0;
    memcpy(&bits, &value, sizeof(bits));
    return bits;
}

static uint32_t pack_rgb(const uint8_t rgb[3])
{
    return (uint32_t)rgb[0]
        | ((uint32_t)rgb[1] << 8)
        | ((uint32_t)rgb[2] << 16);
}

static void fill_control(
    uint32_t control[CONTROL_DWORDS],
    uint32_t source_width,
    uint32_t source_height,
    float time,
    unsigned effect)
{
    memset(control, 0, CONTROL_DWORDS * sizeof(*control));
    const SpritePreset *preset = &SPRITE_PRESETS[effect];
    uint32_t frame = (uint32_t)lroundf(fmaxf(time, 0.0f) * 60.0f);
    control[0] = 0x53564658U;
    control[1] = 1;
    control[2] = frame;
    control[3] = 0;
    control[4] = float_bits((float)frame * (1.0f / 60.0f));
    control[13] = float_bits(0.5f);
    control[14] = float_bits(3.14159265359f);
    control[15] = float_bits(0.02f);
    control[16] = 0;
    control[17] = effect;
    for (unsigned index = 0; index < 4; ++index) {
        control[18 + index] = float_bits(preset->parameters[index]);
    }
    control[22] = pack_rgb(preset->color_a);
    control[23] = pack_rgb(preset->color_b);
    control[24] = source_width;
    control[25] = source_height;
    control[26] = source_width * 4;
    control[27] = SPIRIT_SIZE * 4;
    control[28] = 1;
    control[30] = 60;
    control[31] = float_bits(12.0f);
}

static uint8_t unpremultiply(uint8_t value, uint8_t alpha)
{
    if (alpha == 0) {
        return 0;
    }
    unsigned straight =
        ((unsigned)value * 255U + (unsigned)alpha / 2U) / (unsigned)alpha;
    return (uint8_t)(straight > 255U ? 255U : straight);
}

static void write_grid(const char *path, const uint32_t *cells)
{
    size_t grid_pixels = (size_t)GRID_WIDTH * GRID_HEIGHT;
    size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
    uint8_t *rgba = calloc(grid_pixels, 4);
    if (rgba == NULL) {
        die("out of memory for output grid");
    }
    for (unsigned effect = 0; effect < EFFECT_COUNT; ++effect) {
        unsigned cell_x = effect % GRID_COLUMNS;
        unsigned cell_y = effect / GRID_COLUMNS;
        for (unsigned y = 0; y < SPIRIT_SIZE; ++y) {
            for (unsigned x = 0; x < SPIRIT_SIZE; ++x) {
                size_t source_index =
                    (size_t)effect * cell_pixels + (size_t)y * SPIRIT_SIZE + x;
                size_t output_index =
                    ((size_t)cell_y * SPIRIT_SIZE + y) * GRID_WIDTH
                    + (size_t)cell_x * SPIRIT_SIZE + x;
                uint32_t packed = cells[source_index];
                uint8_t blue = (uint8_t)packed;
                uint8_t green = (uint8_t)(packed >> 8);
                uint8_t red = (uint8_t)(packed >> 16);
                uint8_t alpha = (uint8_t)(packed >> 24);
                rgba[output_index * 4] = unpremultiply(red, alpha);
                rgba[output_index * 4 + 1] = unpremultiply(green, alpha);
                rgba[output_index * 4 + 2] = unpremultiply(blue, alpha);
                rgba[output_index * 4 + 3] = alpha;
            }
        }
    }

    png_image image;
    memset(&image, 0, sizeof(image));
    image.version = PNG_IMAGE_VERSION;
    image.width = GRID_WIDTH;
    image.height = GRID_HEIGHT;
    image.format = PNG_FORMAT_RGBA;
    if (!png_image_write_to_file(&image, path, 0, rgba, 0, NULL)) {
        free(rgba);
        die("libpng failed to write output grid");
    }
    free(rgba);
}

int main(int argc, char **argv)
{
    const char *output = argc > 1 ? argv[1] : "bld/spirit-sprite-vfx-grid.png";
    float time = 2.25f;
    if (argc > 2) {
        char *end = NULL;
        time = strtof(argv[2], &end);
        if (end == argv[2] || *end != '\0' || !isfinite(time) || time < 0.0f) {
            die("time must be a finite non-negative number");
        }
    }
    if (argc > 3) {
        die("usage: spirit_sprite_vfx_offline [output.png] [time_seconds]");
    }

    uint32_t source_width = 0;
    uint32_t source_height = 0;
    uint8_t *source_rgba = extract_lilly(&source_width, &source_height);
    if (source_width == 0 || source_height == 0
        || source_width > SPIRIT_SIZE || source_height > SPIRIT_SIZE) {
        die("Lilly dimensions are outside the production contract");
    }

    cl_platform_id platform = choose_platform();
    cl_device_id device = NULL;
    check_cl(
        clGetDeviceIDs(platform, CL_DEVICE_TYPE_GPU, 1, &device, NULL),
        "clGetDeviceIDs");
    char platform_name[256] = {0};
    char device_name[256] = {0};
    check_cl(
        clGetPlatformInfo(
            platform,
            CL_PLATFORM_NAME,
            sizeof(platform_name),
            platform_name,
            NULL),
        "clGetPlatformInfo");
    check_cl(
        clGetDeviceInfo(
            device,
            CL_DEVICE_NAME,
            sizeof(device_name),
            device_name,
            NULL),
        "clGetDeviceInfo");

    cl_int error = CL_SUCCESS;
    const cl_context_properties properties[] = {
        CL_CONTEXT_PLATFORM,
        (cl_context_properties)platform,
        0,
    };
    cl_context context =
        clCreateContext(properties, 1, &device, NULL, NULL, &error);
    check_cl(error, "clCreateContext");
    cl_command_queue queue = clCreateCommandQueue(context, device, 0, &error);
    check_cl(error, "clCreateCommandQueue");
    cl_program program = build_program(context, device, SPRITE_SOURCE);
    cl_kernel kernel =
        clCreateKernel(program, "spirit_vfx_sprite_rgba8", &error);
    check_cl(error, "clCreateKernel");

    uint32_t control[CONTROL_DWORDS];
    fill_control(control, source_width, source_height, time, 0);
    size_t source_bytes = (size_t)source_width * source_height * 4;
    size_t cursor_bytes =
        (size_t)SPIRIT_SIZE * SPIRIT_SIZE * sizeof(uint32_t);
    uint32_t *cells = calloc(EFFECT_COUNT, cursor_bytes);
    if (cells == NULL) {
        die("out of memory for sprite cells");
    }

    cl_mem source_buffer = clCreateBuffer(
        context,
        CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR,
        source_bytes,
        source_rgba,
        &error);
    check_cl(error, "clCreateBuffer(source)");
    cl_mem control_buffer = clCreateBuffer(
        context,
        CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR,
        sizeof(control),
        control,
        &error);
    check_cl(error, "clCreateBuffer(control)");
    cl_mem cursor_buffer = clCreateBuffer(
        context,
        CL_MEM_READ_WRITE,
        cursor_bytes,
        NULL,
        &error);
    check_cl(error, "clCreateBuffer(cursor)");
    check_cl(
        clSetKernelArg(kernel, 0, sizeof(source_buffer), &source_buffer),
        "clSetKernelArg(source)");
    check_cl(
        clSetKernelArg(kernel, 1, sizeof(control_buffer), &control_buffer),
        "clSetKernelArg(control)");
    check_cl(
        clSetKernelArg(kernel, 2, sizeof(cursor_buffer), &cursor_buffer),
        "clSetKernelArg(cursor)");

    const size_t global[2] = {SPIRIT_SIZE, SPIRIT_SIZE};
    const size_t local[2] = {16, 1};
    size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
    for (unsigned effect = 0; effect < EFFECT_COUNT; ++effect) {
        fill_control(control, source_width, source_height, time, effect);
        check_cl(
            clEnqueueWriteBuffer(
                queue,
                control_buffer,
                CL_TRUE,
                0,
                sizeof(control),
                control,
                0,
                NULL,
                NULL),
            "clEnqueueWriteBuffer(control)");
        check_cl(
            clEnqueueNDRangeKernel(
                queue,
                kernel,
                2,
                NULL,
                global,
                local,
                0,
                NULL,
                NULL),
            "clEnqueueNDRangeKernel");
        check_cl(
            clEnqueueReadBuffer(
                queue,
                cursor_buffer,
                CL_TRUE,
                0,
                cursor_bytes,
                cells + (size_t)effect * cell_pixels,
                0,
                NULL,
                NULL),
            "clEnqueueReadBuffer(cursor)");
    }
    check_cl(clFinish(queue), "clFinish");
    write_grid(output, cells);

    printf("Spirit Sprite shader comparison grid complete\n");
    printf("  platform: %s\n", platform_name);
    printf("  device:   %s\n", device_name);
    printf("  asset:    %s (%ux%u RGBA8)\n",
           LILLY_FRAME, source_width, source_height);
    printf("  grid:     ");
    for (unsigned effect = 0; effect < EFFECT_COUNT; ++effect) {
        printf("%s%s", effect == 0 ? "" : " | ", SPRITE_PRESETS[effect].name);
    }
    printf("\n");
    printf("  dispatch: sixteen 256x256 cells, local 16x1, production OpenCL source\n");
    printf("  time:     %.3f s\n", time);
    printf("  output:   %s\n", output);

    clReleaseMemObject(cursor_buffer);
    clReleaseMemObject(control_buffer);
    clReleaseMemObject(source_buffer);
    clReleaseKernel(kernel);
    clReleaseProgram(program);
    clReleaseCommandQueue(queue);
    clReleaseContext(context);
    free(cells);
    free(source_rgba);
    return EXIT_SUCCESS;
}
