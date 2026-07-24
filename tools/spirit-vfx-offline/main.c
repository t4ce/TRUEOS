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
 * Minimal OpenCL 1.2 declarations. TRUEOS intentionally keeps this replay
 * tool buildable on hosts that have an ICD loader but no OpenCL development
 * header package.
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
extern cl_int clGetDeviceIDs(cl_platform_id, cl_device_type, cl_uint, cl_device_id *, cl_uint *);
extern cl_int clGetDeviceInfo(cl_device_id, cl_uint, size_t, void *, size_t *);
extern cl_context clCreateContext(const cl_context_properties *, cl_uint, const cl_device_id *,
                                  void (*)(const char *, const void *, size_t, void *), void *,
                                  cl_int *);
extern cl_command_queue clCreateCommandQueue(cl_context, cl_device_id,
                                             cl_command_queue_properties, cl_int *);
extern cl_program clCreateProgramWithSource(cl_context, cl_uint, const char **, const size_t *,
                                            cl_int *);
extern cl_program clCreateProgramWithIL(cl_context, const void *, size_t, cl_int *);
extern cl_int clBuildProgram(cl_program, cl_uint, const cl_device_id *, const char *,
                             void (*)(cl_program, void *), void *);
extern cl_int clGetProgramBuildInfo(cl_program, cl_device_id, cl_uint, size_t, void *, size_t *);
extern cl_kernel clCreateKernel(cl_program, const char *, cl_int *);
extern cl_mem clCreateBuffer(cl_context, cl_mem_flags, size_t, void *, cl_int *);
extern cl_int clSetKernelArg(cl_kernel, cl_uint, size_t, const void *);
extern cl_int clEnqueueNDRangeKernel(cl_command_queue, cl_kernel, cl_uint, const size_t *,
                                    const size_t *, const size_t *, cl_uint, const void *,
                                    void *);
extern cl_int clEnqueueReadBuffer(cl_command_queue, cl_mem, cl_uint, size_t, size_t, void *,
                                  cl_uint, const void *, void *);
extern cl_int clEnqueueWriteBuffer(cl_command_queue, cl_mem, cl_uint, size_t, size_t, const void *,
                                   cl_uint, const void *, void *);
extern cl_int clFinish(cl_command_queue);
extern cl_int clReleaseMemObject(cl_mem);
extern cl_int clReleaseKernel(cl_kernel);
extern cl_int clReleaseProgram(cl_program);
extern cl_int clReleaseCommandQueue(cl_command_queue);
extern cl_int clReleaseContext(cl_context);

enum {
    SPIRIT_SIZE = 256,
    CONTROL_DWORDS = 33,
    LILLY_FRAME_COUNT = 7,
    LILLY_FRAME_PERIOD_MS = 110,
    SPIRIT_TARGET_HZ = 60,
    REPLAY_FIRST_ID = 2,
    REPLAY_LAST_ID = 11,
    REPLAY_MODE_COUNT = REPLAY_LAST_ID - REPLAY_FIRST_ID + 1,
    PANEL_COLUMNS = 5,
    PANEL_ROWS = (REPLAY_MODE_COUNT + PANEL_COLUMNS - 1) / PANEL_COLUMNS,
    PANEL_WIDTH = SPIRIT_SIZE * PANEL_COLUMNS,
    PANEL_HEIGHT = SPIRIT_SIZE * PANEL_ROWS,
};

typedef enum {
    REPLAY_ENERGY_RING = REPLAY_FIRST_ID,
    REPLAY_MAGIC_CIRCLE,
    REPLAY_NEBULA_SMOKE,
    REPLAY_CYBER_GRID,
    REPLAY_PORTAL_VORTEX,
    REPLAY_SPEED_LINES,
    REPLAY_BOKEH_FIELD,
    REPLAY_WATER_RIPPLES,
    REPLAY_PIXEL_BURST,
    REPLAY_MAGIC_TIME_CIRCLE,
} ReplayMode;

typedef struct {
    const char *name;
    float scale;
    uint8_t color_a[3];
    uint8_t color_b[3];
} BackgroundPreset;

static const BackgroundPreset BACKGROUND_PRESETS[REPLAY_MODE_COUNT] = {
    {"energy-ring", 1.0f, {0xFF, 0x4D, 0xB8}, {0x60, 0xED, 0xFF}},
    {"magic-circle", 1.0f, {0x8D, 0x68, 0xFF}, {0x6C, 0xF2, 0xFF}},
    {"nebula-smoke", 1.1f, {0x88, 0x3D, 0xFF}, {0x30, 0xC8, 0xFF}},
    {"cyber-grid", 1.1f, {0x7F, 0x5D, 0xFF}, {0x42, 0xEA, 0xFF}},
    {"portal-vortex", 1.0f, {0xF1, 0x5F, 0xFF}, {0x61, 0xEA, 0xFF}},
    {"speed-lines", 1.0f, {0xFF, 0x4F, 0x8D}, {0xFF, 0xE8, 0x6B}},
    {"bokeh-field", 1.0f, {0xFF, 0x8E, 0xDC}, {0x75, 0xEA, 0xFF}},
    {"water-ripples", 1.0f, {0x4F, 0x8D, 0xFF}, {0x6E, 0xFF, 0xE4}},
    {"pixel-burst", 1.0f, {0xB0, 0x6C, 0xFF}, {0x5D, 0xEE, 0xFF}},
    {"magic-time-circle", 1.0f, {0x8D, 0x68, 0xFF}, {0x6C, 0xF2, 0xFF}},
};

typedef struct {
    float speed;
    float intensity;
} RuntimeControls;

static const RuntimeControls DEFAULT_RUNTIME_CONTROLS = {
    .speed = 1.0f,
    .intensity = 1.0f,
};

static const float MAGIC_TIME_STATIC_PREVIEW_SECONDS = 10.0f * 3600.0f + 9.0f * 60.0f + 42.0f;

typedef struct {
    uint32_t width;
    uint32_t height;
    uint8_t *rgba[LILLY_FRAME_COUNT];
} LillyAsset;

static const char *const LILLY_ASSET_KEY = "idle.crossed.soft_blink";
static const char *const LILLY_ASSET_DIRECTORY =
    "Lilly/Idle/Crossed-Arms/idle-1_frames";
static const char *const BACKGROUND_SOURCE =
    "crates/trueos-shader/gpgpu/kernels/spirit_vfx_background_rgba8.cl";
static const char *const SPRITE_SOURCE =
    "crates/trueos-shader/gpgpu/kernels/spirit_vfx_sprite_rgba8.cl";

static void die(const char *message) {
    fprintf(stderr, "spirit-vfx-offline: %s\n", message);
    exit(EXIT_FAILURE);
}

static void check_cl(cl_int error, const char *operation) {
    if (error == CL_SUCCESS) {
        return;
    }
    fprintf(stderr, "spirit-vfx-offline: %s failed with OpenCL error %d\n", operation, error);
    exit(EXIT_FAILURE);
}

static char *read_text_file(const char *path, size_t *length_out) {
    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        fprintf(stderr, "spirit-vfx-offline: cannot open %s: %s\n", path, strerror(errno));
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
        die("out of host memory for shader source");
    }
    if (fread(text, 1, (size_t)length, file) != (size_t)length) {
        die("failed to read shader source");
    }
    fclose(file);
    text[length] = '\0';
    *length_out = (size_t)length;
    return text;
}

static uint8_t *extract_lilly_rgba(unsigned frame, uint32_t *width_out, uint32_t *height_out) {
    if (frame >= LILLY_FRAME_COUNT) {
        die("fixed Lilly frame index is outside the asset");
    }
    char command[256];
    int written =
        snprintf(command, sizeof(command), "7z x -so tools/Lilly.7z "
                                           "'%s/frame_%02u.png' 2>/dev/null",
                 LILLY_ASSET_DIRECTORY, frame + 1);
    if (written < 0 || (size_t)written >= sizeof(command)) {
        die("fixed Lilly extraction command overflowed");
    }
    FILE *stream = popen(command, "r");
    if (stream == NULL) {
        die("failed to start 7z for the fixed Lilly frame");
    }

    png_image image;
    memset(&image, 0, sizeof(image));
    image.version = PNG_IMAGE_VERSION;
    if (!png_image_begin_read_from_stdio(&image, stream)) {
        pclose(stream);
        die("libpng could not read the fixed Lilly frame");
    }
    image.format = PNG_FORMAT_RGBA;
    size_t bytes = PNG_IMAGE_SIZE(image);
    uint8_t *rgba = malloc(bytes);
    if (rgba == NULL) {
        png_image_free(&image);
        pclose(stream);
        die("out of host memory for Lilly RGBA pixels");
    }
    if (!png_image_finish_read(&image, NULL, rgba, 0, NULL)) {
        free(rgba);
        png_image_free(&image);
        pclose(stream);
        die("libpng could not decode the fixed Lilly frame");
    }
    int status = pclose(stream);
    if (status != 0) {
        free(rgba);
        png_image_free(&image);
        die("7z failed while extracting the fixed Lilly frame");
    }
    *width_out = image.width;
    *height_out = image.height;
    png_image_free(&image);
    return rgba;
}

static LillyAsset extract_lilly_asset(void) {
    LillyAsset asset;
    memset(&asset, 0, sizeof(asset));
    for (unsigned frame = 0; frame < LILLY_FRAME_COUNT; ++frame) {
        uint32_t width = 0;
        uint32_t height = 0;
        asset.rgba[frame] = extract_lilly_rgba(frame, &width, &height);
        if (frame == 0) {
            asset.width = width;
            asset.height = height;
        } else if (width != asset.width || height != asset.height) {
            die("Lilly animation frames do not share one surface shape");
        }
    }
    if (asset.width == 0 || asset.height == 0 || asset.width > SPIRIT_SIZE ||
        asset.height > SPIRIT_SIZE) {
        die("fixed Lilly asset dimensions are outside the production shader contract");
    }
    return asset;
}

static void free_lilly_asset(LillyAsset *asset) {
    for (unsigned frame = 0; frame < LILLY_FRAME_COUNT; ++frame) {
        free(asset->rgba[frame]);
        asset->rgba[frame] = NULL;
    }
}

static cl_platform_id choose_platform(void) {
    cl_uint count = 0;
    check_cl(clGetPlatformIDs(0, NULL, &count), "clGetPlatformIDs(count)");
    if (count == 0) {
        die("no OpenCL platforms found");
    }
    cl_platform_id *platforms = calloc(count, sizeof(*platforms));
    if (platforms == NULL) {
        die("out of host memory for OpenCL platforms");
    }
    check_cl(clGetPlatformIDs(count, platforms, NULL), "clGetPlatformIDs(list)");

    cl_platform_id fallback = NULL;
    cl_platform_id selected = NULL;
    for (cl_uint index = 0; index < count; ++index) {
        char name[256] = {0};
        cl_int info_error =
            clGetPlatformInfo(platforms[index], CL_PLATFORM_NAME, sizeof(name), name, NULL);
        cl_device_id candidate = NULL;
        cl_int device_error =
            clGetDeviceIDs(platforms[index], CL_DEVICE_TYPE_GPU, 1, &candidate, NULL);
        if (device_error == CL_DEVICE_NOT_FOUND) {
            continue;
        }
        check_cl(device_error, "clGetDeviceIDs(probe)");
        if (fallback == NULL) {
            fallback = platforms[index];
        }
        if (info_error == CL_SUCCESS && strstr(name, "Intel") != NULL) {
            selected = platforms[index];
            break;
        }
    }
    free(platforms);
    if (selected != NULL) {
        return selected;
    }
    if (fallback != NULL) {
        return fallback;
    }
    die("no GPU OpenCL device found");
    return NULL;
}

static cl_program build_program(
    cl_context context,
    cl_device_id device,
    const char *source_path,
    const char *spirv_path)
{
    size_t source_length = 0;
    char *source = read_text_file(
        spirv_path != NULL ? spirv_path : source_path,
        &source_length);
    cl_int error = CL_SUCCESS;
    cl_program program = NULL;
    if (spirv_path != NULL) {
        program = clCreateProgramWithIL(context, source, source_length, &error);
        check_cl(error, "clCreateProgramWithIL");
    } else {
        const char *sources[] = {source};
        const size_t lengths[] = {source_length};
        program = clCreateProgramWithSource(context, 1, sources, lengths, &error);
        check_cl(error, "clCreateProgramWithSource");
    }

    error = clBuildProgram(
        program,
        1,
        &device,
        spirv_path != NULL ? NULL : "-cl-std=CL1.2",
        NULL,
        NULL);
    if (error != CL_SUCCESS) {
        size_t log_bytes = 0;
        clGetProgramBuildInfo(program, device, CL_PROGRAM_BUILD_LOG, 0, NULL, &log_bytes);
        char *log = calloc(log_bytes + 1, 1);
        if (log != NULL) {
            clGetProgramBuildInfo(program, device, CL_PROGRAM_BUILD_LOG, log_bytes, log, NULL);
            fprintf(
                stderr,
                "OpenCL build log for %s:\n%s\n",
                spirv_path != NULL ? spirv_path : source_path,
                log);
            free(log);
        }
        check_cl(error, "clBuildProgram");
    }
    free(source);
    return program;
}

static uint32_t float_bits(float value) {
    uint32_t bits;
    memcpy(&bits, &value, sizeof(bits));
    return bits;
}

/* Same low-byte-red control-page packing used by SpiritVfxRgb8::packed_rgb. */
static uint32_t pack_control_rgb(uint8_t red, uint8_t green, uint8_t blue) {
    return (uint32_t)red | ((uint32_t)green << 8) | ((uint32_t)blue << 16);
}

static const char *mode_name(ReplayMode mode) {
    unsigned id = (unsigned)mode;
    if (id >= REPLAY_FIRST_ID && id <= REPLAY_LAST_ID) {
        return BACKGROUND_PRESETS[id - REPLAY_FIRST_ID].name;
    }
    return "invalid";
}

static void fill_control(uint32_t control[CONTROL_DWORDS], uint32_t source_width,
                         uint32_t source_height, float animation_time_seconds,
                         float clock_time_seconds, ReplayMode mode,
                         const RuntimeControls *runtime) {
    memset(control, 0, CONTROL_DWORDS * sizeof(*control));
    uint32_t frame =
        (uint32_t)lroundf(fmaxf(animation_time_seconds, 0.0f) * 60.0f);
    float production_time = (float)frame * (1.0f / 60.0f);

    control[0] = 0x53564658U; /* SVFX */
    control[1] = 1;
    control[2] = frame;
    unsigned mode_id = (unsigned)mode;
    if (mode_id < REPLAY_FIRST_ID || mode_id > REPLAY_LAST_ID) {
        die("invalid replay background mode");
    }
    const BackgroundPreset *preset = &BACKGROUND_PRESETS[mode_id - REPLAY_FIRST_ID];
    control[3] = mode_id;
    control[4] = float_bits(production_time);
    control[5] = float_bits(1.0f);
    control[6] = float_bits(preset->scale);
    control[7] = float_bits(runtime->speed);
    control[8] = float_bits(runtime->intensity);
    control[9] =
        pack_control_rgb(preset->color_a[0], preset->color_a[1], preset->color_a[2]);
    control[10] =
        pack_control_rgb(preset->color_b[0], preset->color_b[1], preset->color_b[2]);
    control[11] = float_bits(0.0f);
    control[12] = float_bits(0.0f);
    control[13] = float_bits(0.50f);
    control[14] = float_bits(3.14159265359f);
    control[15] = float_bits(0.02f);
    control[16] = 0; /* nearest / pixel crisp */
    if (mode == REPLAY_MAGIC_TIME_CIRCLE) {
        /* Match Spirit Idle: MagicTimeCircle background + AuraBloom sprite. */
        control[17] = 1;
        control[18] = float_bits(12.0f);
        control[19] = float_bits(2.5f);
        control[20] = float_bits(0.0f);
        control[21] = float_bits(0.0f);
    } else {
        control[17] = 0; /* Original / clean */
    }
    control[22] = pack_control_rgb(0x8D, 0x6C, 0xFF);
    control[23] = pack_control_rgb(0x5E, 0xE7, 0xFF);
    control[24] = source_width;
    control[25] = source_height;
    control[26] = source_width * 4;
    control[27] = SPIRIT_SIZE * 4;
    control[28] = 1;
    control[30] = 60;
    control[31] = float_bits(12.0f);
    control[32] = float_bits(fminf(fmaxf(clock_time_seconds, 0.0f), 86399.0f));
}

static uint8_t unpremultiply(uint8_t value, uint8_t alpha) {
    if (alpha == 0) {
        return 0;
    }
    unsigned straight = ((unsigned)value * 255U + (unsigned)alpha / 2U) / (unsigned)alpha;
    return (uint8_t)(straight > 255U ? 255U : straight);
}

static void write_grid_png(const char *path, const uint32_t *cursor_grid) {
    size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
    size_t grid_pixels = (size_t)PANEL_WIDTH * PANEL_HEIGHT;
    uint8_t *straight_rgba = calloc(grid_pixels, 4);
    if (straight_rgba == NULL) {
        die("out of host memory for output conversion");
    }
    for (unsigned mode = 0; mode < REPLAY_MODE_COUNT; ++mode) {
        unsigned cell_x = mode % PANEL_COLUMNS;
        unsigned cell_y = mode / PANEL_COLUMNS;
        for (unsigned y = 0; y < SPIRIT_SIZE; ++y) {
            for (unsigned x = 0; x < SPIRIT_SIZE; ++x) {
                size_t source_index =
                    (size_t)mode * cell_pixels + (size_t)y * SPIRIT_SIZE + x;
                size_t output_index = ((size_t)cell_y * SPIRIT_SIZE + y) * PANEL_WIDTH
                    + (size_t)cell_x * SPIRIT_SIZE + x;
                uint32_t packed = cursor_grid[source_index];
                uint8_t blue = (uint8_t)packed;
                uint8_t green = (uint8_t)(packed >> 8);
                uint8_t red = (uint8_t)(packed >> 16);
                uint8_t alpha = (uint8_t)(packed >> 24);
                straight_rgba[output_index * 4] = unpremultiply(red, alpha);
                straight_rgba[output_index * 4 + 1] = unpremultiply(green, alpha);
                straight_rgba[output_index * 4 + 2] = unpremultiply(blue, alpha);
                straight_rgba[output_index * 4 + 3] = alpha;
            }
        }
    }

    png_image image;
    memset(&image, 0, sizeof(image));
    image.version = PNG_IMAGE_VERSION;
    image.width = PANEL_WIDTH;
    image.height = PANEL_HEIGHT;
    image.format = PNG_FORMAT_RGBA;
    if (!png_image_write_to_file(&image, path, 0, straight_rgba, 0, NULL)) {
        free(straight_rgba);
        die("libpng failed to write the Spirit preview");
    }
    free(straight_rgba);
}

typedef struct {
    Display *display;
    Window window;
    GC gc;
    XImage *image;
    Atom wm_delete;
    int argb;
} SpiritPanel;

typedef struct {
    unsigned long flags;
    unsigned long functions;
    unsigned long decorations;
    long input_mode;
    unsigned long status;
} MotifWmHints;

static uint64_t monotonic_nanoseconds(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) {
        die("clock_gettime(CLOCK_MONOTONIC) failed");
    }
    return (uint64_t)now.tv_sec * 1000000000ULL + (uint64_t)now.tv_nsec;
}

static float utc_seconds_of_day(void) {
    time_t now = time(NULL);
    if (now < 0) {
        return 0.0f;
    }
    return (float)((uint64_t)now % 86400ULL);
}

static void sleep_until(uint64_t deadline_ns) {
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

static SpiritPanel create_panel(void) {
    SpiritPanel panel;
    memset(&panel, 0, sizeof(panel));
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
        XRenderPictFormat *format = XRenderFindVisualFormat(panel.display, visual_info.visual);
        if (format != NULL && format->type == PictTypeDirect && format->direct.alphaMask != 0) {
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
    attributes.event_mask = ExposureMask | KeyPressMask | StructureNotifyMask;
    panel.window = XCreateWindow(panel.display, root, x, y, PANEL_WIDTH, PANEL_HEIGHT, 0, depth,
                                 InputOutput, visual,
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
    XChangeProperty(panel.display, panel.window, motif_hints, motif_hints, 32, PropModeReplace,
                    (unsigned char *)&hints, 5);

    XSizeHints size_hints;
    memset(&size_hints, 0, sizeof(size_hints));
    size_hints.flags = PMinSize | PMaxSize;
    size_hints.min_width = PANEL_WIDTH;
    size_hints.min_height = PANEL_HEIGHT;
    size_hints.max_width = PANEL_WIDTH;
    size_hints.max_height = PANEL_HEIGHT;
    XSetWMNormalHints(panel.display, panel.window, &size_hints);
    XStoreName(panel.display, panel.window, "Spirit VFX OpenCL Comparison Grid");
    panel.wm_delete = XInternAtom(panel.display, "WM_DELETE_WINDOW", False);
    XSetWMProtocols(panel.display, panel.window, &panel.wm_delete, 1);

    panel.gc = XCreateGC(panel.display, panel.window, 0, NULL);
    panel.image = XCreateImage(panel.display, visual, (unsigned)depth, ZPixmap, 0, NULL, PANEL_WIDTH,
                               PANEL_HEIGHT, 32, 0);
    if (panel.gc == NULL || panel.image == NULL) {
        die("failed to allocate X11 panel image");
    }
    panel.image->data = calloc((size_t)panel.image->bytes_per_line, PANEL_HEIGHT);
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

static float step_runtime_control(float value, float delta, float minimum, float maximum) {
    float stepped = roundf((value + delta) * 100.0f) / 100.0f;
    return fminf(maximum, fmaxf(minimum, stepped));
}

static void print_runtime_controls(const RuntimeControls *runtime) {
    printf("  controls: Speed %.2fx | Intensity %.2fx\n", runtime->speed,
           runtime->intensity);
    fflush(stdout);
}

static int panel_process_events(SpiritPanel *panel, RuntimeControls *runtime) {
    while (XPending(panel->display) > 0) {
        XEvent event;
        XNextEvent(panel->display, &event);
        if (event.type == DestroyNotify) {
            return 0;
        }
        if (event.type == ClientMessage &&
            (Atom)event.xclient.data.l[0] == panel->wm_delete) {
            return 0;
        }
        if (event.type == KeyPress) {
            KeySym key = XLookupKeysym(&event.xkey, 0);
            int changed = 1;
            switch (key) {
            case XK_Escape:
                return 0;
            case XK_Left:
                runtime->speed =
                    step_runtime_control(runtime->speed, -0.01f, 0.0f, 4.0f);
                break;
            case XK_Right:
                runtime->speed =
                    step_runtime_control(runtime->speed, 0.01f, 0.0f, 4.0f);
                break;
            case XK_Down:
                runtime->intensity =
                    step_runtime_control(runtime->intensity, -0.01f, 0.1f, 2.5f);
                break;
            case XK_Up:
                runtime->intensity =
                    step_runtime_control(runtime->intensity, 0.01f, 0.1f, 2.5f);
                break;
            default:
                changed = 0;
                break;
            }
            if (changed) {
                print_runtime_controls(runtime);
            }
        }
    }
    return 1;
}

static void panel_present(SpiritPanel *panel, const uint32_t *cursor_grid) {
    size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
    for (unsigned y = 0; y < PANEL_HEIGHT; ++y) {
        uint32_t *row = (uint32_t *)(panel->image->data + y * panel->image->bytes_per_line);
        memset(row, 0, (size_t)PANEL_WIDTH * sizeof(*row));
    }
    for (unsigned mode = 0; mode < REPLAY_MODE_COUNT; ++mode) {
        unsigned cell_x = mode % PANEL_COLUMNS;
        unsigned cell_y = mode / PANEL_COLUMNS;
        for (unsigned y = 0; y < SPIRIT_SIZE; ++y) {
            uint32_t *row = (uint32_t *)(
                panel->image->data
                + ((size_t)cell_y * SPIRIT_SIZE + y) * panel->image->bytes_per_line);
            for (unsigned x = 0; x < SPIRIT_SIZE; ++x) {
                uint32_t pixel =
                    cursor_grid[(size_t)mode * cell_pixels + (size_t)y * SPIRIT_SIZE + x];
                row[(size_t)cell_x * SPIRIT_SIZE + x] =
                    panel->argb ? pixel : (pixel & 0x00FFFFFFU);
            }
        }
    }
    XPutImage(panel->display, panel->window, panel->gc, panel->image, 0, 0, 0, 0, PANEL_WIDTH,
              PANEL_HEIGHT);
    XFlush(panel->display);
}

static void destroy_panel(SpiritPanel *panel) {
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

static void dispatch_spirit_frame(cl_command_queue queue, cl_kernel background_kernel,
                                  cl_kernel sprite_kernel, cl_mem control_buffer,
                                  cl_mem source_buffer, cl_mem cursor_buffer,
                                  const uint32_t control[CONTROL_DWORDS],
                                  uint32_t *cursor_bgra) {
    check_cl(clEnqueueWriteBuffer(queue, control_buffer, CL_TRUE, 0,
                                  CONTROL_DWORDS * sizeof(uint32_t), control, 0, NULL, NULL),
             "clEnqueueWriteBuffer(control)");
    check_cl(clSetKernelArg(sprite_kernel, 0, sizeof(source_buffer), &source_buffer),
             "clSetKernelArg(sprite.source)");
    const size_t global[2] = {SPIRIT_SIZE, SPIRIT_SIZE};
    const size_t local[2] = {16, 1};
    if (control[3] != 0) {
        check_cl(
            clEnqueueNDRangeKernel(queue, background_kernel, 2, NULL, global, local, 0, NULL, NULL),
            "clEnqueueNDRangeKernel(background)");
    }
    check_cl(clEnqueueNDRangeKernel(queue, sprite_kernel, 2, NULL, global, local, 0, NULL, NULL),
             "clEnqueueNDRangeKernel(sprite)");
    size_t cursor_bytes = (size_t)SPIRIT_SIZE * SPIRIT_SIZE * sizeof(uint32_t);
    check_cl(clEnqueueReadBuffer(queue, cursor_buffer, CL_TRUE, 0, cursor_bytes, cursor_bgra, 0,
                                 NULL, NULL),
             "clEnqueueReadBuffer(cursor)");
}

static void run_panel(cl_command_queue queue, cl_kernel background_kernel,
                      cl_kernel sprite_kernel, cl_mem control_buffer,
                      const cl_mem source_buffers[LILLY_FRAME_COUNT], cl_mem cursor_buffer,
                      uint32_t *cursor_grid, const LillyAsset *asset) {
    SpiritPanel panel = create_panel();
    RuntimeControls runtime = DEFAULT_RUNTIME_CONTROLS;
    uint64_t started_ns = monotonic_nanoseconds();
    uint64_t next_frame_ns = started_ns;
    uint32_t shader_frame = 0;

    printf("Spirit VFX panel running; press ESC to close\n");
    printf("  asset:    %s (%u frames, %u ms/frame, loop)\n", LILLY_ASSET_KEY,
           LILLY_FRAME_COUNT, LILLY_FRAME_PERIOD_MS);
    printf("  grid:     retained background IDs 2..11, row-major, five columns\n");
    printf("  cadence:  shader %u Hz; asset %.3f Hz\n", SPIRIT_TARGET_HZ,
           1000.0 / LILLY_FRAME_PERIOD_MS);
    printf("  window:   borderless %ux%u ARGB=%d\n", PANEL_WIDTH, PANEL_HEIGHT, panel.argb);
    printf("  fixed:    Opacity 1.00; each effect's existing Scale\n");
    printf("  keys:     Left/Right = Speed; Down/Up = Intensity; step 0.01\n");
    print_runtime_controls(&runtime);

    while (panel_process_events(&panel, &runtime)) {
        sleep_until(next_frame_ns);
        uint64_t now_ns = monotonic_nanoseconds();
        uint64_t elapsed_ms = (now_ns - started_ns) / 1000000ULL;
        unsigned source_frame =
            (unsigned)((elapsed_ms / LILLY_FRAME_PERIOD_MS) % LILLY_FRAME_COUNT);
        size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
        for (unsigned mode_index = 0; mode_index < REPLAY_MODE_COUNT; ++mode_index) {
            ReplayMode mode = (ReplayMode)(mode_index + REPLAY_FIRST_ID);
            float animation_time = (float)shader_frame * (1.0f / SPIRIT_TARGET_HZ);
            uint32_t control[CONTROL_DWORDS];
            fill_control(control, asset->width, asset->height, animation_time,
                         utc_seconds_of_day(), mode, &runtime);
            control[2] = shader_frame;
            dispatch_spirit_frame(
                queue, background_kernel, sprite_kernel, control_buffer,
                source_buffers[source_frame], cursor_buffer, control,
                cursor_grid + (size_t)mode_index * cell_pixels);
        }
        panel_present(&panel, cursor_grid);
        shader_frame++;

        next_frame_ns += 1000000000ULL / SPIRIT_TARGET_HZ;
        uint64_t finished_ns = monotonic_nanoseconds();
        if (finished_ns > next_frame_ns) {
            next_frame_ns = finished_ns + 1000000000ULL / SPIRIT_TARGET_HZ;
        }
    }
    destroy_panel(&panel);
}

int main(int argc, char **argv) {
    int panel_mode = 1;
    const char *output_path = "bld/spirit-vfx-grid.png";
    float time_seconds = 1.5f;
    float magic_time_seconds = MAGIC_TIME_STATIC_PREVIEW_SECONDS;
    if (argc > 1 && strcmp(argv[1], "--panel") == 0) {
        if (argc > 2) {
            die("usage: spirit_vfx_offline [--panel]");
        }
    } else if (argc > 1 && strcmp(argv[1], "--render-grid") == 0) {
        panel_mode = 0;
        output_path = argc > 2 ? argv[2] : output_path;
        if (argc > 3) {
            char *end = NULL;
            time_seconds = strtof(argv[3], &end);
            if (end == argv[3] || *end != '\0' || !isfinite(time_seconds) ||
                time_seconds < 0.0f) {
                die("time must be a finite non-negative number of seconds");
            }
        }
        if (argc > 4) {
            char *end = NULL;
            magic_time_seconds = strtof(argv[4], &end);
            if (end == argv[4] || *end != '\0' || !isfinite(magic_time_seconds) ||
                magic_time_seconds < 0.0f || magic_time_seconds >= 86400.0f) {
                die("clock time must be seconds-of-day in the range 0..86399");
            }
        }
        if (argc > 5) {
            die("usage: spirit_vfx_offline --render-grid "
                "[output.png] [time_seconds] [clock_seconds_of_day]");
        }
    } else if (argc > 1) {
        die("usage: spirit_vfx_offline [--panel] | "
            "--render-grid [output.png] [time_seconds] [clock_seconds_of_day]");
    }

    LillyAsset asset = extract_lilly_asset();

    cl_platform_id platform = choose_platform();
    cl_device_id device = NULL;
    check_cl(clGetDeviceIDs(platform, CL_DEVICE_TYPE_GPU, 1, &device, NULL),
             "clGetDeviceIDs(select)");
    char platform_name[256] = {0};
    char device_name[256] = {0};
    check_cl(clGetPlatformInfo(platform, CL_PLATFORM_NAME, sizeof(platform_name), platform_name,
                               NULL),
             "clGetPlatformInfo(name)");
    check_cl(clGetDeviceInfo(device, CL_DEVICE_NAME, sizeof(device_name), device_name, NULL),
             "clGetDeviceInfo(name)");

    cl_int error = CL_SUCCESS;
    cl_context_properties context_properties[] = {
        CL_CONTEXT_PLATFORM,
        (cl_context_properties)platform,
        0,
    };
    cl_context context =
        clCreateContext(context_properties, 1, &device, NULL, NULL, &error);
    check_cl(error, "clCreateContext");
    cl_command_queue queue = clCreateCommandQueue(context, device, 0, &error);
    check_cl(error, "clCreateCommandQueue");

    const char *background_spirv = getenv("SPIRIT_VFX_BACKGROUND_SPV");
    const char *sprite_spirv = getenv("SPIRIT_VFX_SPRITE_SPV");
    cl_program background_program =
        build_program(context, device, BACKGROUND_SOURCE, background_spirv);
    cl_program sprite_program =
        build_program(context, device, SPRITE_SOURCE, sprite_spirv);
    cl_kernel background_kernel =
        clCreateKernel(background_program, "spirit_vfx_background_rgba8", &error);
    check_cl(error, "clCreateKernel(background)");
    cl_kernel sprite_kernel =
        clCreateKernel(sprite_program, "spirit_vfx_sprite_rgba8", &error);
    check_cl(error, "clCreateKernel(sprite)");

    RuntimeControls runtime = DEFAULT_RUNTIME_CONTROLS;
    uint32_t control[CONTROL_DWORDS];
    fill_control(control, asset.width, asset.height, time_seconds, magic_time_seconds,
                 REPLAY_ENERGY_RING, &runtime);
    size_t source_bytes = (size_t)asset.width * asset.height * 4;
    size_t cursor_bytes = (size_t)SPIRIT_SIZE * SPIRIT_SIZE * sizeof(uint32_t);
    uint32_t *cursor_grid = calloc(REPLAY_MODE_COUNT, cursor_bytes);
    if (cursor_grid == NULL) {
        die("out of host memory for cursor grid output");
    }

    cl_mem control_buffer =
        clCreateBuffer(context, CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR, sizeof(control), control,
                       &error);
    check_cl(error, "clCreateBuffer(control)");
    cl_mem source_buffers[LILLY_FRAME_COUNT];
    for (unsigned frame = 0; frame < LILLY_FRAME_COUNT; ++frame) {
        source_buffers[frame] =
            clCreateBuffer(context, CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR, source_bytes,
                           asset.rgba[frame], &error);
        check_cl(error, "clCreateBuffer(source frame)");
    }
    cl_mem cursor_buffer =
        clCreateBuffer(context, CL_MEM_READ_WRITE | CL_MEM_COPY_HOST_PTR, cursor_bytes, cursor_grid,
                       &error);
    check_cl(error, "clCreateBuffer(cursor)");

    check_cl(clSetKernelArg(background_kernel, 0, sizeof(control_buffer), &control_buffer),
             "clSetKernelArg(background.control)");
    check_cl(clSetKernelArg(background_kernel, 1, sizeof(cursor_buffer), &cursor_buffer),
             "clSetKernelArg(background.dst)");
    check_cl(clSetKernelArg(sprite_kernel, 1, sizeof(control_buffer), &control_buffer),
             "clSetKernelArg(sprite.control)");
    check_cl(clSetKernelArg(sprite_kernel, 2, sizeof(cursor_buffer), &cursor_buffer),
             "clSetKernelArg(sprite.dst)");

    printf("  platform: %s\n", platform_name);
    printf("  device:   %s\n", device_name);
    printf(
        "  frontend: background=%s sprite=%s\n",
        background_spirv != NULL ? "C++ SPIR-V" : "OpenCL C source",
        sprite_spirv != NULL ? "C++ SPIR-V" : "OpenCL C source");
    if (panel_mode) {
        run_panel(queue, background_kernel, sprite_kernel, control_buffer, source_buffers,
                  cursor_buffer, cursor_grid, &asset);
    } else {
        size_t cell_pixels = (size_t)SPIRIT_SIZE * SPIRIT_SIZE;
        for (unsigned mode_index = 0; mode_index < REPLAY_MODE_COUNT; ++mode_index) {
            ReplayMode mode = (ReplayMode)(mode_index + REPLAY_FIRST_ID);
            fill_control(control, asset.width, asset.height, time_seconds,
                         magic_time_seconds, mode, &runtime);
            dispatch_spirit_frame(
                queue, background_kernel, sprite_kernel, control_buffer, source_buffers[0],
                cursor_buffer, control, cursor_grid + (size_t)mode_index * cell_pixels);
        }
        check_cl(clFinish(queue), "clFinish");
        write_grid_png(output_path, cursor_grid);
        printf("Spirit VFX offline comparison grid complete\n");
        printf("  asset:    %s/frame_01.png (%ux%u RGBA8)\n", LILLY_ASSET_DIRECTORY,
               asset.width, asset.height);
        printf("  grid:     ");
        for (unsigned mode = 0; mode < REPLAY_MODE_COUNT; ++mode) {
            printf("%s%s", mode == 0 ? "" : " | ",
                   mode_name((ReplayMode)(mode + REPLAY_FIRST_ID)));
        }
        printf("\n");
        printf(
            "  dispatch: ten 256x256 cells, local 16x1, selected GPU programs\n");
        unsigned clock_seconds = (unsigned)magic_time_seconds;
        printf("  time:     %.3f s (frame %u at 60 Hz)\n", time_seconds,
               (unsigned)lroundf(time_seconds * 60.0f));
        printf("  clock:    %02u:%02u:%02u UTC (quantized HH/MM/SS preview)\n",
               clock_seconds / 3600U, (clock_seconds / 60U) % 60U, clock_seconds % 60U);
        printf("  output:   %s\n", output_path);
    }

    clReleaseMemObject(cursor_buffer);
    for (unsigned frame = 0; frame < LILLY_FRAME_COUNT; ++frame) {
        clReleaseMemObject(source_buffers[frame]);
    }
    clReleaseMemObject(control_buffer);
    clReleaseKernel(sprite_kernel);
    clReleaseKernel(background_kernel);
    clReleaseProgram(sprite_program);
    clReleaseProgram(background_program);
    clReleaseCommandQueue(queue);
    clReleaseContext(context);
    free(cursor_grid);
    free_lilly_asset(&asset);
    return EXIT_SUCCESS;
}
