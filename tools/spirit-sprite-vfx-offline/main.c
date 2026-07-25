#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <math.h>
#include <png.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/Xrender.h>
#include <X11/keysym.h>

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
extern cl_program clCreateProgramWithIL(
    cl_context,
    const void *,
    size_t,
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
    LILLY_FRAME_COUNT = 7,
    LILLY_FRAME_PERIOD_MS = 110,
    SPIRIT_TARGET_HZ = 60,
    EFFECT_COUNT = 16,
    GRID_COLUMNS = 4,
    GRID_ROWS = 4,
    GRID_WIDTH = SPIRIT_SIZE * GRID_COLUMNS,
    GRID_HEIGHT = SPIRIT_SIZE * GRID_ROWS,
    CONTROL_PANEL_WIDTH = 320,
    PANEL_WIDTH = GRID_WIDTH + CONTROL_PANEL_WIDTH,
    PANEL_HEIGHT = GRID_HEIGHT,
    SHARED_PARAM_COUNT = 4,
    CONTROL_MARGIN = 24,
    SLIDER_TRACK_X = GRID_WIDTH + CONTROL_MARGIN,
    SLIDER_TRACK_WIDTH = CONTROL_PANEL_WIDTH - CONTROL_MARGIN * 2,
    SLIDER_TOP = 106,
    SLIDER_SPACING = 84,
    COLOR_SWATCH_Y = 506,
    COLOR_SWATCH_WIDTH = 120,
    COLOR_SWATCH_HEIGHT = 64,
    RESET_BUTTON_Y = 614,
    RESET_BUTTON_HEIGHT = 38,
};

typedef struct {
    uint32_t width;
    uint32_t height;
    uint8_t *rgba[LILLY_FRAME_COUNT];
} LillyAsset;

typedef struct {
    Display *display;
    Window window;
    GC gc;
    XImage *image;
    Atom wm_delete;
    int argb;
    int active_slider;
} SpiritPanel;

typedef struct {
    unsigned long flags;
    unsigned long functions;
    unsigned long decorations;
    long input_mode;
    unsigned long status;
} MotifWmHints;

typedef struct {
    const char *name;
    float parameters[4];
    uint8_t color_a[3];
    uint8_t color_b[3];
} SpritePreset;

typedef struct {
    float normalized[SHARED_PARAM_COUNT];
    uint8_t colors[2][3];
    int color_override[2];
} RuntimeControls;

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

static const float SPRITE_PARAM_MIN[EFFECT_COUNT][SHARED_PARAM_COUNT] = {
    {0.0f, 0.0f, 0.0f, 0.0f},
    {2.0f, 0.0f, 0.0f, 0.0f},
    {0.5f, 0.0f, 0.0f, 0.0f},
    {1.0f, 2.0f, 0.0f, 0.0f},
    {0.5f, 0.0f, 2.0f, 0.0f},
    {20.0f, 0.0f, 0.0f, 0.1f},
    {0.0f, 0.0f, 0.0f, 0.0f},
    {0.0f, 0.01f, 2.0f, 0.0f},
    {1.0f, 0.0f, 0.0f, 0.05f},
    {1.0f, 2.0f, 0.0f, 0.0f},
    {0.0f, 0.5f, 0.0f, 0.0f},
    {0.0f, 0.0f, 0.0f, 0.0f},
    {0.0f, 1.0f, 0.0f, 2.0f},
    {2.0f, 0.5f, 0.0f, 0.0f},
    {0.0f, 1.0f, 0.0f, 0.0f},
    {2.0f, 0.0f, 0.0f, 0.0f},
};

static const float SPRITE_PARAM_MAX[EFFECT_COUNT][SHARED_PARAM_COUNT] = {
    {0.0f, 0.0f, 0.0f, 0.0f},
    {30.0f, 2.5f, 4.0f, 1.0f},
    {12.0f, 2.5f, 4.0f, 1.0f},
    {12.0f, 34.0f, 4.0f, 2.5f},
    {12.0f, 3.0f, 24.0f, 1.0f},
    {220.0f, 12.0f, 4.0f, 1.0f},
    {10.0f, 24.0f, 8.0f, 1.0f},
    {1.0f, 0.28f, 22.0f, 3.0f},
    {30.0f, 1.8f, 4.0f, 1.0f},
    {14.0f, 30.0f, 6.0f, 3.0f},
    {3.0f, 14.0f, 1.0f, 10.0f},
    {1.0f, 8.0f, 14.0f, 8.0f},
    {12.0f, 30.0f, 6.0f, 16.0f},
    {16.0f, 8.0f, 2.0f, 8.0f},
    {14.0f, 20.0f, 4.0f, 7.0f},
    {30.0f, 2.0f, 3.0f, 1.0f},
};

static const float SPRITE_PARAM_STEP[EFFECT_COUNT][SHARED_PARAM_COUNT] = {
    {0.0f, 0.0f, 0.0f, 0.0f},
    {0.5f, 0.01f, 0.01f, 0.01f},
    {0.1f, 0.01f, 0.01f, 0.01f},
    {0.1f, 0.5f, 0.01f, 0.01f},
    {0.1f, 0.01f, 0.1f, 0.01f},
    {1.0f, 0.1f, 0.01f, 0.01f},
    {0.1f, 0.1f, 0.01f, 0.01f},
    {0.01f, 0.005f, 0.1f, 0.01f},
    {0.5f, 0.01f, 0.01f, 0.01f},
    {0.1f, 0.1f, 0.01f, 0.01f},
    {0.01f, 0.1f, 0.01f, 0.1f},
    {0.01f, 0.01f, 0.1f, 0.1f},
    {0.1f, 0.1f, 0.01f, 1.0f},
    {1.0f, 0.1f, 0.01f, 0.1f},
    {0.1f, 0.1f, 0.01f, 0.1f},
    {0.5f, 0.01f, 0.01f, 0.01f},
};

static const RuntimeControls DEFAULT_RUNTIME_CONTROLS = {
    .normalized = {0.5f, 0.5f, 0.5f, 0.5f},
    .colors = {{0x9A, 0x7C, 0xFF}, {0x5E, 0xE7, 0xFF}},
    .color_override = {0, 0},
};

static const char *const LILLY_ASSET_KEY = "idle.crossed.soft_blink";
static const char *const LILLY_ASSET_DIRECTORY =
    "Lilly/Idle/Crossed-Arms/idle-1_frames";
static const char *const SPRITE_ARTIFACT =
    "crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/"
    "spirit_vfx_sprite_rgba8.spv";

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

static uint8_t *extract_lilly_frame(
    unsigned frame,
    uint32_t *width_out,
    uint32_t *height_out)
{
    if (frame >= LILLY_FRAME_COUNT) {
        die("fixed Lilly frame index is outside the asset");
    }
    char command[256];
    int written = snprintf(
        command,
        sizeof(command),
        "7z x -so tools/Lilly.7z '%s/frame_%02u.png' 2>/dev/null",
        LILLY_ASSET_DIRECTORY,
        frame + 1);
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

static LillyAsset extract_lilly_asset(void)
{
    LillyAsset asset;
    memset(&asset, 0, sizeof(asset));
    for (unsigned frame = 0; frame < LILLY_FRAME_COUNT; ++frame) {
        uint32_t width = 0;
        uint32_t height = 0;
        asset.rgba[frame] = extract_lilly_frame(frame, &width, &height);
        if (frame == 0) {
            asset.width = width;
            asset.height = height;
        } else if (width != asset.width || height != asset.height) {
            die("Lilly animation frames do not share one surface shape");
        }
    }
    if (asset.width == 0 || asset.height == 0
        || asset.width > SPIRIT_SIZE || asset.height > SPIRIT_SIZE) {
        die("Lilly dimensions are outside the production contract");
    }
    return asset;
}

static void free_lilly_asset(LillyAsset *asset)
{
    for (unsigned frame = 0; frame < LILLY_FRAME_COUNT; ++frame) {
        free(asset->rgba[frame]);
        asset->rgba[frame] = NULL;
    }
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
    const char *artifact_path)
{
    size_t artifact_length = 0;
    char *artifact = read_text_file(artifact_path, &artifact_length);
    cl_int error = CL_SUCCESS;
    cl_program program =
        clCreateProgramWithIL(context, artifact, artifact_length, &error);
    check_cl(error, "clCreateProgramWithIL");
    error = clBuildProgram(
        program,
        1,
        &device,
        NULL,
        NULL,
        NULL);
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
    free(artifact);
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

static float mapped_parameter(
    const RuntimeControls *runtime,
    unsigned effect,
    unsigned parameter)
{
    float position =
        fminf(1.0f, fmaxf(0.0f, runtime->normalized[parameter]));
    float minimum = SPRITE_PARAM_MIN[effect][parameter];
    float authored = SPRITE_PRESETS[effect].parameters[parameter];
    float maximum = SPRITE_PARAM_MAX[effect][parameter];
    float value = position < 0.5f
        ? minimum + position * 2.0f * (authored - minimum)
        : authored + (position - 0.5f) * 2.0f * (maximum - authored);
    float step = SPRITE_PARAM_STEP[effect][parameter];
    if (step > 0.0f) {
        value = roundf(value / step) * step;
    }
    return fminf(maximum, fmaxf(minimum, value));
}

static void fill_control(
    uint32_t control[CONTROL_DWORDS],
    uint32_t source_width,
    uint32_t source_height,
    float time,
    unsigned effect,
    const RuntimeControls *runtime)
{
    memset(control, 0, CONTROL_DWORDS * sizeof(*control));
    const SpritePreset *preset = &SPRITE_PRESETS[effect];
    uint32_t frame = (uint32_t)lroundf(fmaxf(time, 0.0f) * 60.0f);
    control[0] = 0x53564658U;
    control[1] = 1;
    control[2] = frame;
    control[3] = 0;
    control[4] = float_bits((float)frame * (1.0f / 60.0f));
    control[13] = float_bits(0.65f);
    control[14] = float_bits(3.14159265359f);
    control[15] = float_bits(0.02f);
    control[16] = 0;
    control[17] = effect;
    for (unsigned index = 0; index < 4; ++index) {
        control[18 + index] =
            float_bits(mapped_parameter(runtime, effect, index));
    }
    control[22] = pack_rgb(
        runtime->color_override[0] ? runtime->colors[0] : preset->color_a);
    control[23] = pack_rgb(
        runtime->color_override[1] ? runtime->colors[1] : preset->color_b);
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

static uint64_t monotonic_nanoseconds(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) {
        die("clock_gettime(CLOCK_MONOTONIC) failed");
    }
    return (uint64_t)now.tv_sec * 1000000000ULL + (uint64_t)now.tv_nsec;
}

static void sleep_until(uint64_t deadline_ns)
{
    uint64_t now_ns = monotonic_nanoseconds();
    if (now_ns >= deadline_ns) {
        return;
    }
    uint64_t delay_ns = deadline_ns - now_ns;
    struct timespec delay = {
        .tv_sec = (time_t)(delay_ns / 1000000000ULL),
        .tv_nsec = (long)(delay_ns % 1000000000ULL),
    };
    while (nanosleep(&delay, &delay) != 0 && errno == EINTR) {
    }
}

static SpiritPanel create_panel(void)
{
    SpiritPanel panel;
    memset(&panel, 0, sizeof(panel));
    panel.active_slider = -1;
    panel.display = XOpenDisplay(NULL);
    if (panel.display == NULL) {
        die("cannot open X11/Xwayland display");
    }
    int screen = DefaultScreen(panel.display);
    Window root = RootWindow(panel.display, screen);
    Visual *visual = DefaultVisual(panel.display, screen);
    int depth = DefaultDepth(panel.display, screen);
    Colormap colormap = DefaultColormap(panel.display, screen);

    XVisualInfo visual_info;
    if (XMatchVisualInfo(panel.display, screen, 32, TrueColor, &visual_info)) {
        XRenderPictFormat *format =
            XRenderFindVisualFormat(panel.display, visual_info.visual);
        if (format != NULL && format->type == PictTypeDirect
            && format->direct.alphaMask != 0) {
            visual = visual_info.visual;
            depth = visual_info.depth;
            colormap = XCreateColormap(panel.display, root, visual, AllocNone);
            panel.argb = 1;
        }
    }

    int x = (DisplayWidth(panel.display, screen) - PANEL_WIDTH) / 2;
    int y = (DisplayHeight(panel.display, screen) - PANEL_HEIGHT) / 2;
    XSetWindowAttributes attributes;
    memset(&attributes, 0, sizeof(attributes));
    attributes.colormap = colormap;
    attributes.border_pixel = 0;
    attributes.background_pixel = 0;
    attributes.event_mask =
        ExposureMask | KeyPressMask | StructureNotifyMask
        | ButtonPressMask | ButtonReleaseMask | PointerMotionMask;
    panel.window = XCreateWindow(
        panel.display,
        root,
        x,
        y,
        PANEL_WIDTH,
        PANEL_HEIGHT,
        0,
        depth,
        InputOutput,
        visual,
        CWColormap | CWBorderPixel | CWBackPixel | CWEventMask,
        &attributes);
    if (panel.window == 0) {
        die("XCreateWindow failed");
    }

    Atom motif_hints = XInternAtom(panel.display, "_MOTIF_WM_HINTS", False);
    MotifWmHints hints = {
        .flags = 1UL << 1,
        .decorations = 0,
    };
    XChangeProperty(
        panel.display,
        panel.window,
        motif_hints,
        motif_hints,
        32,
        PropModeReplace,
        (unsigned char *)&hints,
        5);

    XSizeHints size_hints;
    memset(&size_hints, 0, sizeof(size_hints));
    size_hints.flags = PMinSize | PMaxSize;
    size_hints.min_width = PANEL_WIDTH;
    size_hints.min_height = PANEL_HEIGHT;
    size_hints.max_width = PANEL_WIDTH;
    size_hints.max_height = PANEL_HEIGHT;
    XSetWMNormalHints(panel.display, panel.window, &size_hints);
    XStoreName(panel.display, panel.window, "Spirit Production Sprite VFX Preview");
    panel.wm_delete = XInternAtom(panel.display, "WM_DELETE_WINDOW", False);
    XSetWMProtocols(panel.display, panel.window, &panel.wm_delete, 1);

    panel.gc = XCreateGC(panel.display, panel.window, 0, NULL);
    panel.image = XCreateImage(
        panel.display,
        visual,
        (unsigned)depth,
        ZPixmap,
        0,
        NULL,
        GRID_WIDTH,
        GRID_HEIGHT,
        32,
        0);
    if (panel.gc == NULL || panel.image == NULL) {
        die("failed to allocate X11 panel image");
    }
    panel.image->data =
        calloc((size_t)panel.image->bytes_per_line, GRID_HEIGHT);
    if (panel.image->data == NULL) {
        die("out of host memory for X11 panel pixels");
    }

    XMapRaised(panel.display, panel.window);
    XFlush(panel.display);
    for (;;) {
        XEvent event;
        XNextEvent(panel.display, &event);
        if (event.type == MapNotify && event.xmap.window == panel.window) {
            break;
        }
    }
    XSetInputFocus(panel.display, panel.window, RevertToParent, CurrentTime);
    XFlush(panel.display);
    return panel;
}

static unsigned long panel_rgb(uint8_t red, uint8_t green, uint8_t blue)
{
    return 0xFF000000UL
        | ((unsigned long)red << 16)
        | ((unsigned long)green << 8)
        | (unsigned long)blue;
}

static int point_in_rect(
    int x,
    int y,
    int left,
    int top,
    int width,
    int height)
{
    return x >= left && x < left + width && y >= top && y < top + height;
}

static void set_shared_parameter(
    RuntimeControls *runtime,
    unsigned parameter,
    int x)
{
    float normalized =
        (float)(x - SLIDER_TRACK_X) / (float)SLIDER_TRACK_WIDTH;
    normalized = fminf(1.0f, fmaxf(0.0f, normalized));
    runtime->normalized[parameter] =
        roundf(normalized * 100.0f) * 0.01f;
}

static void choose_shared_color(RuntimeControls *runtime, unsigned color)
{
    char command[256];
    const char *title = color == 0 ? "Spirit FX Color A" : "Spirit FX Color B";
    int written = snprintf(
        command,
        sizeof(command),
        "zenity --color-selection --show-palette --title='%s' "
        "--color='#%02X%02X%02X' 2>/dev/null",
        title,
        runtime->colors[color][0],
        runtime->colors[color][1],
        runtime->colors[color][2]);
    if (written < 0 || (size_t)written >= sizeof(command)) {
        return;
    }
    FILE *picker = popen(command, "r");
    if (picker == NULL) {
        fprintf(stderr, "spirit-sprite-vfx-offline: could not open color picker\n");
        return;
    }
    char result[128] = {0};
    int have_result = fgets(result, sizeof(result), picker) != NULL;
    int status = pclose(picker);
    if (!have_result || status != 0) {
        return;
    }

    unsigned red = 0;
    unsigned green = 0;
    unsigned blue = 0;
    int parsed = sscanf(result, "#%2x%2x%2x", &red, &green, &blue);
    if (parsed != 3) {
        parsed = sscanf(result, "rgb(%u,%u,%u)", &red, &green, &blue);
    }
    if (parsed != 3 || red > 255 || green > 255 || blue > 255) {
        fprintf(
            stderr,
            "spirit-sprite-vfx-offline: unsupported color-picker result: %s",
            result);
        return;
    }
    runtime->colors[color][0] = (uint8_t)red;
    runtime->colors[color][1] = (uint8_t)green;
    runtime->colors[color][2] = (uint8_t)blue;
    runtime->color_override[color] = 1;
    printf(
        "  Color %c: #%02X%02X%02X (shared)\n",
        color == 0 ? 'A' : 'B',
        red,
        green,
        blue);
    fflush(stdout);
}

static int panel_process_events(
    SpiritPanel *panel,
    RuntimeControls *runtime)
{
    while (XPending(panel->display) > 0) {
        XEvent event;
        XNextEvent(panel->display, &event);
        if (event.type == DestroyNotify) {
            return 0;
        }
        if (event.type == ClientMessage
            && (Atom)event.xclient.data.l[0] == panel->wm_delete) {
            return 0;
        }
        if (event.type == KeyPress
            && XLookupKeysym(&event.xkey, 0) == XK_Escape) {
            return 0;
        }
        if (event.type == ButtonPress && event.xbutton.button == Button1) {
            int handled = 0;
            for (unsigned parameter = 0;
                 parameter < SHARED_PARAM_COUNT;
                 ++parameter) {
                int track_y =
                    SLIDER_TOP + (int)parameter * SLIDER_SPACING + 30;
                if (point_in_rect(
                        event.xbutton.x,
                        event.xbutton.y,
                        SLIDER_TRACK_X - 8,
                        track_y - 14,
                        SLIDER_TRACK_WIDTH + 16,
                        28)) {
                    panel->active_slider = (int)parameter;
                    set_shared_parameter(
                        runtime, parameter, event.xbutton.x);
                    handled = 1;
                    break;
                }
            }
            if (handled) {
                continue;
            }
            int color_a_x = GRID_WIDTH + CONTROL_MARGIN;
            int color_b_x =
                PANEL_WIDTH - CONTROL_MARGIN - COLOR_SWATCH_WIDTH;
            if (point_in_rect(
                    event.xbutton.x,
                    event.xbutton.y,
                    color_a_x,
                    COLOR_SWATCH_Y,
                    COLOR_SWATCH_WIDTH,
                    COLOR_SWATCH_HEIGHT)) {
                choose_shared_color(runtime, 0);
            } else if (point_in_rect(
                           event.xbutton.x,
                           event.xbutton.y,
                           color_b_x,
                           COLOR_SWATCH_Y,
                           COLOR_SWATCH_WIDTH,
                           COLOR_SWATCH_HEIGHT)) {
                choose_shared_color(runtime, 1);
            } else if (point_in_rect(
                           event.xbutton.x,
                           event.xbutton.y,
                           SLIDER_TRACK_X,
                           RESET_BUTTON_Y,
                           SLIDER_TRACK_WIDTH,
                           RESET_BUTTON_HEIGHT)) {
                *runtime = DEFAULT_RUNTIME_CONTROLS;
                printf("  controls reset to each effect's authored defaults\n");
                fflush(stdout);
            }
        }
        if (event.type == MotionNotify && panel->active_slider >= 0) {
            set_shared_parameter(
                runtime,
                (unsigned)panel->active_slider,
                event.xmotion.x);
        }
        if (event.type == ButtonRelease
            && event.xbutton.button == Button1) {
            panel->active_slider = -1;
        }
    }
    return 1;
}

static void draw_control_panel(
    SpiritPanel *panel,
    const RuntimeControls *runtime)
{
    Display *display = panel->display;
    GC gc = panel->gc;
    Window window = panel->window;
    XSetForeground(display, gc, panel_rgb(20, 18, 28));
    XFillRectangle(
        display,
        window,
        gc,
        GRID_WIDTH,
        0,
        CONTROL_PANEL_WIDTH,
        PANEL_HEIGHT);
    XSetForeground(display, gc, panel_rgb(72, 63, 96));
    XFillRectangle(display, window, gc, GRID_WIDTH, 0, 1, PANEL_HEIGHT);

    XSetForeground(display, gc, panel_rgb(241, 238, 248));
    XDrawString(
        display, window, gc, GRID_WIDTH + CONTROL_MARGIN, 38,
        "SPIRIT SPRITE VFX", (int)sizeof("SPIRIT SPRITE VFX") - 1);
    XSetForeground(display, gc, panel_rgb(167, 159, 185));
    XDrawString(
        display, window, gc, GRID_WIDTH + CONTROL_MARGIN, 62,
        "Shared normalized parameters",
        (int)sizeof("Shared normalized parameters") - 1);
    XDrawString(
        display, window, gc, GRID_WIDTH + CONTROL_MARGIN, 80,
        "center = each authored default",
        (int)sizeof("center = each authored default") - 1);

    for (unsigned parameter = 0;
         parameter < SHARED_PARAM_COUNT;
         ++parameter) {
        int top = SLIDER_TOP + (int)parameter * SLIDER_SPACING;
        float adjustment =
            (runtime->normalized[parameter] - 0.5f) * 2.0f;
        char label[64];
        int length = snprintf(
            label,
            sizeof(label),
            "Param %u                         %+.2f",
            parameter + 1,
            adjustment);
        XSetForeground(display, gc, panel_rgb(220, 215, 236));
        XDrawString(
            display,
            window,
            gc,
            SLIDER_TRACK_X,
            top + 12,
            label,
            length);

        int track_y = top + 30;
        XSetForeground(display, gc, panel_rgb(58, 52, 74));
        XFillRectangle(
            display,
            window,
            gc,
            SLIDER_TRACK_X,
            track_y - 3,
            SLIDER_TRACK_WIDTH,
            7);
        int fill_width = (int)lroundf(
            runtime->normalized[parameter] * SLIDER_TRACK_WIDTH);
        XSetForeground(display, gc, panel_rgb(154, 124, 255));
        XFillRectangle(
            display,
            window,
            gc,
            SLIDER_TRACK_X,
            track_y - 3,
            (unsigned)fill_width,
            7);
        int knob_x = SLIDER_TRACK_X + fill_width;
        XSetForeground(display, gc, panel_rgb(224, 216, 255));
        XFillRectangle(
            display, window, gc, knob_x - 4, track_y - 9, 9, 19);
    }

    XSetForeground(display, gc, panel_rgb(241, 238, 248));
    XDrawString(
        display, window, gc, GRID_WIDTH + CONTROL_MARGIN, 474,
        "SHARED FX COLORS", (int)sizeof("SHARED FX COLORS") - 1);
    int color_x[2] = {
        GRID_WIDTH + CONTROL_MARGIN,
        PANEL_WIDTH - CONTROL_MARGIN - COLOR_SWATCH_WIDTH,
    };
    for (unsigned color = 0; color < 2; ++color) {
        char label[32];
        int length;
        if (runtime->color_override[color]) {
            length = snprintf(
                label,
                sizeof(label),
                "%c  #%02X%02X%02X",
                color == 0 ? 'A' : 'B',
                runtime->colors[color][0],
                runtime->colors[color][1],
                runtime->colors[color][2]);
        } else {
            length = snprintf(
                label,
                sizeof(label),
                "%c  per-effect",
                color == 0 ? 'A' : 'B');
        }
        XSetForeground(display, gc, panel_rgb(203, 197, 220));
        XDrawString(
            display, window, gc, color_x[color], COLOR_SWATCH_Y - 10,
            label, length);
        XSetForeground(
            display,
            gc,
            panel_rgb(
                runtime->colors[color][0],
                runtime->colors[color][1],
                runtime->colors[color][2]));
        XFillRectangle(
            display,
            window,
            gc,
            color_x[color],
            COLOR_SWATCH_Y,
            COLOR_SWATCH_WIDTH,
            COLOR_SWATCH_HEIGHT);
        XSetForeground(display, gc, panel_rgb(220, 215, 236));
        XDrawRectangle(
            display,
            window,
            gc,
            color_x[color],
            COLOR_SWATCH_Y,
            COLOR_SWATCH_WIDTH - 1,
            COLOR_SWATCH_HEIGHT - 1);
    }

    XSetForeground(display, gc, panel_rgb(43, 38, 56));
    XFillRectangle(
        display,
        window,
        gc,
        SLIDER_TRACK_X,
        RESET_BUTTON_Y,
        SLIDER_TRACK_WIDTH,
        RESET_BUTTON_HEIGHT);
    XSetForeground(display, gc, panel_rgb(102, 89, 137));
    XDrawRectangle(
        display,
        window,
        gc,
        SLIDER_TRACK_X,
        RESET_BUTTON_Y,
        SLIDER_TRACK_WIDTH - 1,
        RESET_BUTTON_HEIGHT - 1);
    XSetForeground(display, gc, panel_rgb(224, 216, 255));
    XDrawString(
        display,
        window,
        gc,
        SLIDER_TRACK_X + 92,
        RESET_BUTTON_Y + 25,
        "RESET DEFAULTS",
        (int)sizeof("RESET DEFAULTS") - 1);

    XSetForeground(display, gc, panel_rgb(167, 159, 185));
    XDrawString(
        display, window, gc, GRID_WIDTH + CONTROL_MARGIN, 704,
        "Drag sliders to compare all 16.",
        (int)sizeof("Drag sliders to compare all 16.") - 1);
    XDrawString(
        display, window, gc, GRID_WIDTH + CONTROL_MARGIN, 726,
        "Click a swatch to choose a color.",
        (int)sizeof("Click a swatch to choose a color.") - 1);
    XDrawString(
        display, window, gc, GRID_WIDTH + CONTROL_MARGIN, 748,
        "ESC closes the complete panel.",
        (int)sizeof("ESC closes the complete panel.") - 1);
}

static void panel_present(
    SpiritPanel *panel,
    const uint32_t *cells,
    const RuntimeControls *runtime)
{
    size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
    for (unsigned effect = 0; effect < EFFECT_COUNT; ++effect) {
        unsigned cell_x = effect % GRID_COLUMNS;
        unsigned cell_y = effect / GRID_COLUMNS;
        for (unsigned y = 0; y < SPIRIT_SIZE; ++y) {
            uint32_t *row = (uint32_t *)(
                panel->image->data
                + ((size_t)cell_y * SPIRIT_SIZE + y)
                    * panel->image->bytes_per_line);
            for (unsigned x = 0; x < SPIRIT_SIZE; ++x) {
                uint32_t pixel =
                    cells[(size_t)effect * cell_pixels
                        + (size_t)y * SPIRIT_SIZE + x];
                row[(size_t)cell_x * SPIRIT_SIZE + x] =
                    panel->argb ? pixel : (pixel & 0x00FFFFFFU);
            }
        }
    }
    XPutImage(
        panel->display,
        panel->window,
        panel->gc,
        panel->image,
        0,
        0,
        0,
        0,
        GRID_WIDTH,
        GRID_HEIGHT);
    draw_control_panel(panel, runtime);
    XFlush(panel->display);
}

static void destroy_panel(SpiritPanel *panel)
{
    if (panel->image != NULL) {
        XDestroyImage(panel->image);
    }
    if (panel->gc != NULL) {
        XFreeGC(panel->display, panel->gc);
    }
    if (panel->window != 0) {
        XDestroyWindow(panel->display, panel->window);
    }
    if (panel->display != NULL) {
        XCloseDisplay(panel->display);
    }
}

static void dispatch_sprite_frame(
    cl_command_queue queue,
    cl_kernel kernel,
    cl_mem control_buffer,
    cl_mem source_buffer,
    cl_mem cursor_buffer,
    const uint32_t control[CONTROL_DWORDS],
    uint32_t *cursor_bgra)
{
    check_cl(
        clEnqueueWriteBuffer(
            queue,
            control_buffer,
            CL_TRUE,
            0,
            CONTROL_DWORDS * sizeof(uint32_t),
            control,
            0,
            NULL,
            NULL),
        "clEnqueueWriteBuffer(control)");
    check_cl(
        clSetKernelArg(kernel, 0, sizeof(source_buffer), &source_buffer),
        "clSetKernelArg(source)");
    const size_t global[2] = {SPIRIT_SIZE, SPIRIT_SIZE};
    const size_t local[2] = {16, 1};
    check_cl(
        clEnqueueNDRangeKernel(
            queue, kernel, 2, NULL, global, local, 0, NULL, NULL),
        "clEnqueueNDRangeKernel");
    size_t cursor_bytes =
        (size_t)SPIRIT_SIZE * SPIRIT_SIZE * sizeof(uint32_t);
    check_cl(
        clEnqueueReadBuffer(
            queue,
            cursor_buffer,
            CL_TRUE,
            0,
            cursor_bytes,
            cursor_bgra,
            0,
            NULL,
            NULL),
        "clEnqueueReadBuffer(cursor)");
}

static void run_panel(
    cl_command_queue queue,
    cl_kernel kernel,
    cl_mem control_buffer,
    const cl_mem source_buffers[LILLY_FRAME_COUNT],
    cl_mem cursor_buffer,
    uint32_t *cells,
    const LillyAsset *asset)
{
    SpiritPanel panel = create_panel();
    RuntimeControls runtime = DEFAULT_RUNTIME_CONTROLS;
    uint64_t started_ns = monotonic_nanoseconds();
    uint64_t next_frame_ns = started_ns;
    uint32_t shader_frame = 0;

    printf("Spirit Sprite VFX panel running; press ESC to close\n");
    printf(
        "  asset:    %s (%u frames, %u ms/frame, loop)\n",
        LILLY_ASSET_KEY,
        LILLY_FRAME_COUNT,
        LILLY_FRAME_PERIOD_MS);
    printf("  grid:     sprite shader IDs 0..15, row-major, four columns\n");
    printf("  surface:  transparent; no procedural-background dispatch\n");
    printf(
        "  cadence:  shader %u Hz; asset %.3f Hz\n",
        SPIRIT_TARGET_HZ,
        1000.0 / LILLY_FRAME_PERIOD_MS);
    printf(
        "  window:   borderless %ux%u ARGB=%d\n",
        PANEL_WIDTH,
        PANEL_HEIGHT,
        panel.argb);
    printf("  controls: four normalized parameters; shared colors A/B\n");

    while (panel_process_events(&panel, &runtime)) {
        sleep_until(next_frame_ns);
        uint64_t now_ns = monotonic_nanoseconds();
        uint64_t elapsed_ms = (now_ns - started_ns) / 1000000ULL;
        unsigned source_frame =
            (unsigned)((elapsed_ms / LILLY_FRAME_PERIOD_MS)
                % LILLY_FRAME_COUNT);
        size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
        for (unsigned effect = 0; effect < EFFECT_COUNT; ++effect) {
            uint32_t control[CONTROL_DWORDS];
            fill_control(
                control,
                asset->width,
                asset->height,
                (float)shader_frame * (1.0f / SPIRIT_TARGET_HZ),
                effect,
                &runtime);
            dispatch_sprite_frame(
                queue,
                kernel,
                control_buffer,
                source_buffers[source_frame],
                cursor_buffer,
                control,
                cells + (size_t)effect * cell_pixels);
        }
        panel_present(&panel, cells, &runtime);
        shader_frame++;

        next_frame_ns += 1000000000ULL / SPIRIT_TARGET_HZ;
        uint64_t finished_ns = monotonic_nanoseconds();
        if (finished_ns > next_frame_ns) {
            next_frame_ns =
                finished_ns + 1000000000ULL / SPIRIT_TARGET_HZ;
        }
    }
    destroy_panel(&panel);
}

int main(int argc, char **argv)
{
    int panel_mode = 1;
    const char *output = "bld/spirit-sprite-vfx-grid.png";
    float time = 2.25f;
    if (argc > 1 && strcmp(argv[1], "--panel") == 0) {
        if (argc > 2) {
            die("usage: spirit_sprite_vfx_offline [--panel]");
        }
    } else if (argc > 1 && strcmp(argv[1], "--render-grid") == 0) {
        panel_mode = 0;
        output = argc > 2 ? argv[2] : output;
        if (argc > 3) {
            char *end = NULL;
            time = strtof(argv[3], &end);
            if (end == argv[3] || *end != '\0'
                || !isfinite(time) || time < 0.0f) {
                die("time must be a finite non-negative number");
            }
        }
        if (argc > 4) {
            die(
                "usage: spirit_sprite_vfx_offline --render-grid "
                "[output.png] [time_seconds]");
        }
    } else if (argc > 1) {
        die(
            "usage: spirit_sprite_vfx_offline [--panel] | "
            "--render-grid [output.png] [time_seconds]");
    }

    LillyAsset asset = extract_lilly_asset();

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
    cl_program program =
        build_program(context, device, SPRITE_ARTIFACT);
    cl_kernel kernel =
        clCreateKernel(program, "spirit_vfx_sprite_rgba8", &error);
    check_cl(error, "clCreateKernel");

    RuntimeControls static_runtime = DEFAULT_RUNTIME_CONTROLS;
    uint32_t control[CONTROL_DWORDS];
    fill_control(
        control,
        asset.width,
        asset.height,
        time,
        0,
        &static_runtime);
    size_t source_bytes = (size_t)asset.width * asset.height * 4;
    size_t cursor_bytes =
        (size_t)SPIRIT_SIZE * SPIRIT_SIZE * sizeof(uint32_t);
    uint32_t *cells = calloc(EFFECT_COUNT, cursor_bytes);
    if (cells == NULL) {
        die("out of memory for sprite cells");
    }

    cl_mem source_buffers[LILLY_FRAME_COUNT];
    for (unsigned frame = 0; frame < LILLY_FRAME_COUNT; ++frame) {
        source_buffers[frame] = clCreateBuffer(
            context,
            CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR,
            source_bytes,
            asset.rgba[frame],
            &error);
        check_cl(error, "clCreateBuffer(source frame)");
    }
    cl_mem control_buffer = clCreateBuffer(
        context,
        CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR,
        sizeof(control),
        control,
        &error);
    check_cl(error, "clCreateBuffer(control)");
    cl_mem cursor_buffer = clCreateBuffer(
        context,
        CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR,
        cursor_bytes,
        cells,
        &error);
    check_cl(error, "clCreateBuffer(cursor)");
    check_cl(
        clSetKernelArg(kernel, 1, sizeof(control_buffer), &control_buffer),
        "clSetKernelArg(control)");
    check_cl(
        clSetKernelArg(kernel, 2, sizeof(cursor_buffer), &cursor_buffer),
        "clSetKernelArg(cursor)");

    printf("  platform: %s\n", platform_name);
    printf("  device:   %s\n", device_name);
    printf("  artifact: %s\n", SPRITE_ARTIFACT);
    if (panel_mode) {
        run_panel(
            queue,
            kernel,
            control_buffer,
            source_buffers,
            cursor_buffer,
            cells,
            &asset);
    } else {
        size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
        for (unsigned effect = 0; effect < EFFECT_COUNT; ++effect) {
            fill_control(
                control,
                asset.width,
                asset.height,
                time,
                effect,
                &static_runtime);
            dispatch_sprite_frame(
                queue,
                kernel,
                control_buffer,
                source_buffers[0],
                cursor_buffer,
                control,
                cells + (size_t)effect * cell_pixels);
        }
        check_cl(clFinish(queue), "clFinish");
        write_grid(output, cells);

        printf("Spirit production Sprite VFX preview complete\n");
        printf(
            "  asset:    %s/frame_01.png (%ux%u RGBA8)\n",
            LILLY_ASSET_DIRECTORY,
            asset.width,
            asset.height);
        printf("  surface:  transparent; no procedural-background dispatch\n");
        printf("  grid:     ");
        for (unsigned effect = 0; effect < EFFECT_COUNT; ++effect) {
            printf(
                "%s%s",
                effect == 0 ? "" : " | ",
                SPRITE_PRESETS[effect].name);
        }
        printf("\n");
        printf(
            "  dispatch: sixteen 256x256 cells, local 16x1, "
            "production GPU artifact\n");
        printf("  time:     %.3f s\n", time);
        printf("  output:   %s\n", output);
    }

    clReleaseMemObject(cursor_buffer);
    clReleaseMemObject(control_buffer);
    for (unsigned frame = 0; frame < LILLY_FRAME_COUNT; ++frame) {
        clReleaseMemObject(source_buffers[frame]);
    }
    clReleaseKernel(kernel);
    clReleaseProgram(program);
    clReleaseCommandQueue(queue);
    clReleaseContext(context);
    free(cells);
    free_lilly_asset(&asset);
    return EXIT_SUCCESS;
}
