#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/keysym.h>

#ifndef TRUEOS_REPO_ROOT
#define TRUEOS_REPO_ROOT "."
#endif

/* OpenCL declarations kept local, as in the Spirit production preview. */
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
    WINDOW_WIDTH = 1360,
    WINDOW_HEIGHT = 720,
    TOOLBAR_HEIGHT = 48,
    STATUS_HEIGHT = 42,
    EDITOR_WIDTH = 696,
    PREVIEW_WIDTH = 640,
    PREVIEW_HEIGHT = 360,
    PREVIEW_X = EDITOR_WIDTH + 12,
    PREVIEW_Y = 128,
    EDITOR_MARGIN = 12,
    BUTTON_X = 12,
    BUTTON_Y = 10,
    BUTTON_WIDTH = 154,
    BUTTON_HEIGHT = 29,
    MAX_SHADER_BYTES = 1024 * 1024,
    MAX_COMPILE_LOG = 128 * 1024,
};

static const char *const DEFAULT_SHADER =
    "// Paste a texture-free ShaderToy Image pass here.\n"
    "// Run: Ctrl+Enter\n\n"
    "void mainImage(out vec4 fragColor, in vec2 fragCoord)\n"
    "{\n"
    "    vec2 uv = (2.0 * fragCoord - iResolution.xy) / iResolution.y;\n"
    "    float angle = 0.2 * iTime;\n"
    "    uv *= mat2(cos(angle), -sin(angle), sin(angle), cos(angle));\n"
    "    float glow = 0.03 / abs(length(uv) - 0.35 - 0.04*sin(iTime*2.0));\n"
    "    vec3 color = vec3(0.15, 0.55, 1.0) * glow;\n"
    "    fragColor = vec4(color, 1.0);\n"
    "}\n";

static const char *const SESSION_SOURCE =
    TRUEOS_REPO_ROOT "/bld/shadertoy-cpp-offline/session/input.glsl";
static const char *const SESSION_SPIRV =
    TRUEOS_REPO_ROOT "/bld/shadertoy-cpp-offline/bakery/adls/cpp-native/"
    "shadertoy_image/run-a/shadertoy_image.spv";
static const char *const COMPILE_SCRIPT =
    TRUEOS_REPO_ROOT "/tools/shadertoy-cpp-offline/compile_shader.py";

typedef struct {
    cl_platform_id platform;
    cl_device_id device;
    cl_context context;
    cl_command_queue queue;
    cl_program program;
    cl_kernel kernel;
    cl_mem output_buffer;
    cl_mem uniform_buffer;
    int ready;
} ShaderRuntime;

typedef struct {
    Display *display;
    Window window;
    Pixmap backbuffer;
    GC gc;
    XFontStruct *font;
    XImage *preview_image;
    Atom wm_delete;
    Atom clipboard;
    Atom utf8_string;
    Atom clipboard_property;
    unsigned long background;
    unsigned long panel;
    unsigned long editor_background;
    unsigned long foreground;
    unsigned long muted;
    unsigned long accent;
    unsigned long selection;
    unsigned long error;

    char *source;
    size_t source_length;
    size_t source_capacity;
    size_t cursor;
    size_t selection_anchor;
    size_t selection_end;
    size_t scroll_line;
    size_t horizontal_scroll;
    int editor_focus;
    int running;
    int dirty;

    pid_t compiler_pid;
    int compiler_fd;
    char *compile_log;
    size_t compile_log_length;
    int compile_result;

    ShaderRuntime runtime;
    uint32_t *frame_pixels;
    uint64_t playback_started_ns;
    uint64_t previous_frame_ns;
    unsigned frame_number;
    float mouse_x;
    float mouse_y;
    float click_x;
    float click_y;
    int mouse_down;
    char status[512];
} App;

static void set_status(App *app, const char *text) {
    snprintf(app->status, sizeof(app->status), "%s", text);
}

static uint64_t monotonic_nanoseconds(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) {
        return 0;
    }
    return (uint64_t)now.tv_sec * 1000000000ULL + (uint64_t)now.tv_nsec;
}

static unsigned long color(Display *display, int screen, const char *name,
                           unsigned long fallback) {
    XColor exact;
    XColor allocated;
    if (XAllocNamedColor(display, DefaultColormap(display, screen), name, &allocated, &exact)) {
        return allocated.pixel;
    }
    return fallback;
}

static int ensure_source_capacity(App *app, size_t wanted) {
    if (wanted > MAX_SHADER_BYTES) {
        set_status(app, "Source limit is 1 MiB");
        return 0;
    }
    if (wanted + 1 <= app->source_capacity) {
        return 1;
    }
    size_t capacity = app->source_capacity == 0 ? 4096 : app->source_capacity;
    while (capacity < wanted + 1) {
        capacity *= 2;
    }
    char *grown = realloc(app->source, capacity);
    if (grown == NULL) {
        set_status(app, "Out of host memory while editing");
        return 0;
    }
    app->source = grown;
    app->source_capacity = capacity;
    return 1;
}

static void selection_bounds(const App *app, size_t *begin, size_t *end) {
    *begin = app->selection_anchor < app->selection_end
        ? app->selection_anchor : app->selection_end;
    *end = app->selection_anchor < app->selection_end
        ? app->selection_end : app->selection_anchor;
}

static void replace_selection(App *app, const char *text, size_t length) {
    size_t begin;
    size_t end;
    selection_bounds(app, &begin, &end);
    size_t new_length = app->source_length - (end - begin) + length;
    if (!ensure_source_capacity(app, new_length)) {
        return;
    }
    memmove(app->source + begin + length, app->source + end,
            app->source_length - end + 1);
    if (length != 0) {
        memcpy(app->source + begin, text, length);
    }
    app->source_length = new_length;
    app->source[new_length] = '\0';
    app->cursor = begin + length;
    app->selection_anchor = app->cursor;
    app->selection_end = app->cursor;
    app->dirty = 1;
}

static size_t line_start(const App *app, size_t position);

static char *normalize_pasted_text(App *app, const unsigned char *text,
                                   size_t length, size_t *normalized_length) {
    char *normalized = malloc(MAX_SHADER_BYTES + 1);
    if (normalized == NULL) {
        set_status(app, "Out of host memory while normalizing clipboard text");
        return NULL;
    }
    size_t begin;
    size_t end;
    selection_bounds(app, &begin, &end);
    (void)end;
    size_t column = begin - line_start(app, begin);
    size_t output = 0;

#define APPEND_PASTED_BYTE(value) \
    do { \
        if (output >= MAX_SHADER_BYTES) { \
            free(normalized); \
            set_status(app, "Normalized clipboard text exceeds the 1 MiB source limit"); \
            return NULL; \
        } \
        normalized[output++] = (char)(value); \
    } while (0)

    for (size_t input = 0; input < length; ++input) {
        unsigned char byte = text[input];
        if (byte == '\r') {
            if (input + 1 < length && text[input + 1] == '\n') {
                ++input;
            }
            APPEND_PASTED_BYTE('\n');
            column = 0;
            continue;
        }
        if (byte == '\n') {
            APPEND_PASTED_BYTE('\n');
            column = 0;
            continue;
        }
        if (byte == '\t') {
            size_t spaces = 4 - column % 4;
            for (size_t index = 0; index < spaces; ++index) {
                APPEND_PASTED_BYTE(' ');
            }
            column += spaces;
            continue;
        }
        /* UTF-8 NBSP is visually whitespace but is not GLSL whitespace. */
        if (byte == 0xC2 && input + 1 < length && text[input + 1] == 0xA0) {
            APPEND_PASTED_BYTE(' ');
            ++input;
            ++column;
            continue;
        }
        /* Remove BOM and common zero-width formatting marks from web copies. */
        if (byte == 0xEF && input + 2 < length
            && text[input + 1] == 0xBB && text[input + 2] == 0xBF) {
            input += 2;
            continue;
        }
        if (byte == 0xE2 && input + 2 < length && text[input + 1] == 0x80
            && text[input + 2] >= 0x8B && text[input + 2] <= 0x8D) {
            input += 2;
            continue;
        }
        if (byte < 0x20 || byte == 0x7F) {
            continue;
        }
        APPEND_PASTED_BYTE(byte);
        if ((byte & 0xC0) != 0x80) {
            ++column;
        }
    }
#undef APPEND_PASTED_BYTE
    normalized[output] = '\0';
    *normalized_length = output;
    return normalized;
}

static size_t line_start(const App *app, size_t position) {
    while (position > 0 && app->source[position - 1] != '\n') {
        --position;
    }
    return position;
}

static size_t line_end(const App *app, size_t position) {
    while (position < app->source_length && app->source[position] != '\n') {
        ++position;
    }
    return position;
}

static size_t position_line(const App *app, size_t position) {
    size_t line = 0;
    for (size_t i = 0; i < position; ++i) {
        if (app->source[i] == '\n') {
            ++line;
        }
    }
    return line;
}

static size_t line_at(const App *app, size_t wanted_line) {
    size_t line = 0;
    size_t position = 0;
    while (position < app->source_length && line < wanted_line) {
        if (app->source[position++] == '\n') {
            ++line;
        }
    }
    return position;
}

static void keep_cursor_visible(App *app) {
    int line_height = app->font->ascent + app->font->descent + 2;
    size_t visible_lines = (WINDOW_HEIGHT - TOOLBAR_HEIGHT - STATUS_HEIGHT - 12)
        / (size_t)line_height;
    size_t line = position_line(app, app->cursor);
    if (line < app->scroll_line) {
        app->scroll_line = line;
    } else if (line >= app->scroll_line + visible_lines) {
        app->scroll_line = line - visible_lines + 1;
    }
    size_t start = line_start(app, app->cursor);
    size_t column = app->cursor - start;
    size_t visible_columns = (EDITOR_WIDTH - 2 * EDITOR_MARGIN) /
        (size_t)(app->font->max_bounds.width > 0 ? app->font->max_bounds.width : 8);
    if (column < app->horizontal_scroll) {
        app->horizontal_scroll = column;
    } else if (column >= app->horizontal_scroll + visible_columns) {
        app->horizontal_scroll = column - visible_columns + 1;
    }
}

static void move_cursor_vertical(App *app, int delta) {
    size_t current_start = line_start(app, app->cursor);
    size_t column = app->cursor - current_start;
    size_t current_line = position_line(app, current_start);
    size_t target_line;
    if (delta < 0 && current_line < (size_t)(-delta)) {
        target_line = 0;
    } else {
        target_line = (size_t)((long)current_line + delta);
    }
    size_t target = line_at(app, target_line);
    size_t target_end = line_end(app, target);
    app->cursor = target + (column < target_end - target ? column : target_end - target);
    app->selection_anchor = app->cursor;
    app->selection_end = app->cursor;
    keep_cursor_visible(app);
}

static int read_file(const char *path, char **data_out, size_t *length_out) {
    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        return 0;
    }
    if (fseek(file, 0, SEEK_END) != 0) {
        fclose(file);
        return 0;
    }
    long size = ftell(file);
    if (size < 0 || (unsigned long)size > MAX_SHADER_BYTES) {
        fclose(file);
        return 0;
    }
    rewind(file);
    char *data = malloc((size_t)size + 1);
    if (data == NULL) {
        fclose(file);
        return 0;
    }
    size_t read = fread(data, 1, (size_t)size, file);
    fclose(file);
    if (read != (size_t)size) {
        free(data);
        return 0;
    }
    data[read] = '\0';
    *data_out = data;
    *length_out = read;
    return 1;
}

static int write_source(const App *app) {
    char directory[1024];
    snprintf(directory, sizeof(directory), "%s/bld/shadertoy-cpp-offline/session",
             TRUEOS_REPO_ROOT);
    char parent[1024];
    snprintf(parent, sizeof(parent), "%s/bld/shadertoy-cpp-offline", TRUEOS_REPO_ROOT);
    (void)mkdir(TRUEOS_REPO_ROOT "/bld", 0777);
    (void)mkdir(parent, 0777);
    (void)mkdir(directory, 0777);
    FILE *file = fopen(SESSION_SOURCE, "wb");
    if (file == NULL) {
        return 0;
    }
    size_t written = fwrite(app->source, 1, app->source_length, file);
    int okay = written == app->source_length && fclose(file) == 0;
    return okay;
}

static void release_program(ShaderRuntime *runtime) {
    if (runtime->output_buffer != NULL) {
        clReleaseMemObject(runtime->output_buffer);
        runtime->output_buffer = NULL;
    }
    if (runtime->uniform_buffer != NULL) {
        clReleaseMemObject(runtime->uniform_buffer);
        runtime->uniform_buffer = NULL;
    }
    if (runtime->kernel != NULL) {
        clReleaseKernel(runtime->kernel);
        runtime->kernel = NULL;
    }
    if (runtime->program != NULL) {
        clReleaseProgram(runtime->program);
        runtime->program = NULL;
    }
    runtime->ready = 0;
}

static void release_runtime(ShaderRuntime *runtime) {
    release_program(runtime);
    if (runtime->queue != NULL) {
        clReleaseCommandQueue(runtime->queue);
    }
    if (runtime->context != NULL) {
        clReleaseContext(runtime->context);
    }
    memset(runtime, 0, sizeof(*runtime));
}

static int initialize_opencl(App *app) {
    ShaderRuntime *runtime = &app->runtime;
    if (runtime->context != NULL) {
        return 1;
    }
    cl_uint platform_count = 0;
    cl_int error = clGetPlatformIDs(0, NULL, &platform_count);
    if (error != CL_SUCCESS || platform_count == 0) {
        snprintf(app->status, sizeof(app->status),
                 "OpenCL: no platform (error %d); install intel-opencl-icd", error);
        return 0;
    }
    cl_platform_id *platforms = calloc(platform_count, sizeof(*platforms));
    if (platforms == NULL) {
        set_status(app, "OpenCL: out of host memory");
        return 0;
    }
    error = clGetPlatformIDs(platform_count, platforms, NULL);
    if (error != CL_SUCCESS) {
        free(platforms);
        snprintf(app->status, sizeof(app->status), "OpenCL: platform query failed (%d)", error);
        return 0;
    }
    for (cl_uint index = 0; index < platform_count; ++index) {
        char name[256] = {0};
        clGetPlatformInfo(platforms[index], CL_PLATFORM_NAME, sizeof(name), name, NULL);
        if (strstr(name, "Intel") == NULL && strstr(name, "INTEL") == NULL) {
            continue;
        }
        if (clGetDeviceIDs(platforms[index], CL_DEVICE_TYPE_GPU, 1, &runtime->device, NULL)
            == CL_SUCCESS) {
            runtime->platform = platforms[index];
            break;
        }
    }
    free(platforms);
    if (runtime->device == NULL) {
        set_status(app, "OpenCL: no Intel GPU device; this preview intentionally uses the TRUEOS Intel lane");
        return 0;
    }
    cl_context_properties properties[] = {
        CL_CONTEXT_PLATFORM, (cl_context_properties)runtime->platform, 0,
    };
    runtime->context = clCreateContext(properties, 1, &runtime->device, NULL, NULL, &error);
    if (runtime->context == NULL || error != CL_SUCCESS) {
        snprintf(app->status, sizeof(app->status), "OpenCL: clCreateContext failed (%d)", error);
        return 0;
    }
    runtime->queue = clCreateCommandQueue(runtime->context, runtime->device, 0, &error);
    if (runtime->queue == NULL || error != CL_SUCCESS) {
        snprintf(app->status, sizeof(app->status), "OpenCL: clCreateCommandQueue failed (%d)", error);
        return 0;
    }
    return 1;
}

static int load_program(App *app) {
    if (!initialize_opencl(app)) {
        return 0;
    }
    char *spirv = NULL;
    size_t spirv_size = 0;
    if (!read_file(SESSION_SPIRV, &spirv, &spirv_size)) {
        set_status(app, "Bakery succeeded but its SPIR-V output could not be read");
        return 0;
    }
    ShaderRuntime *runtime = &app->runtime;
    release_program(runtime);
    cl_int error = 0;
    runtime->program = clCreateProgramWithIL(runtime->context, spirv, spirv_size, &error);
    free(spirv);
    if (runtime->program == NULL || error != CL_SUCCESS) {
        snprintf(app->status, sizeof(app->status), "OpenCL: clCreateProgramWithIL failed (%d)", error);
        return 0;
    }
    error = clBuildProgram(runtime->program, 1, &runtime->device, NULL, NULL, NULL);
    if (error != CL_SUCCESS) {
        char log[2048] = {0};
        clGetProgramBuildInfo(runtime->program, runtime->device, CL_PROGRAM_BUILD_LOG,
                              sizeof(log), log, NULL);
        fprintf(stderr, "shadertoy-cpp-offline: OpenCL build log:\n%s\n", log);
        snprintf(app->status, sizeof(app->status), "OpenCL: IL build failed (%d): %.350s", error, log);
        return 0;
    }
    runtime->kernel = clCreateKernel(runtime->program, "shadertoy_image", &error);
    if (runtime->kernel == NULL || error != CL_SUCCESS) {
        snprintf(app->status, sizeof(app->status), "OpenCL: kernel creation failed (%d)", error);
        return 0;
    }
    size_t frame_bytes = (size_t)PREVIEW_WIDTH * PREVIEW_HEIGHT * sizeof(uint32_t);
    runtime->output_buffer = clCreateBuffer(runtime->context, CL_MEM_READ_WRITE,
                                             frame_bytes, NULL, &error);
    if (runtime->output_buffer == NULL || error != CL_SUCCESS) {
        snprintf(app->status, sizeof(app->status), "OpenCL: output allocation failed (%d)", error);
        return 0;
    }
    float zero_uniforms[16] = {0};
    runtime->uniform_buffer = clCreateBuffer(runtime->context,
        CL_MEM_READ_ONLY | CL_MEM_COPY_HOST_PTR, sizeof(zero_uniforms), zero_uniforms, &error);
    if (runtime->uniform_buffer == NULL || error != CL_SUCCESS) {
        snprintf(app->status, sizeof(app->status), "OpenCL: uniform allocation failed (%d)", error);
        return 0;
    }
    cl_uint width = PREVIEW_WIDTH;
    cl_uint height = PREVIEW_HEIGHT;
    cl_uint pitch_bytes = PREVIEW_WIDTH * (cl_uint)sizeof(uint32_t);
    if (clSetKernelArg(runtime->kernel, 0, sizeof(runtime->output_buffer), &runtime->output_buffer)
            != CL_SUCCESS
        || clSetKernelArg(runtime->kernel, 1, sizeof(runtime->uniform_buffer), &runtime->uniform_buffer)
            != CL_SUCCESS
        || clSetKernelArg(runtime->kernel, 2, sizeof(width), &width) != CL_SUCCESS
        || clSetKernelArg(runtime->kernel, 3, sizeof(height), &height) != CL_SUCCESS
        || clSetKernelArg(runtime->kernel, 4, sizeof(pitch_bytes), &pitch_bytes) != CL_SUCCESS) {
        set_status(app, "OpenCL: generated kernel ABI does not match the preview");
        return 0;
    }
    runtime->ready = 1;
    app->frame_number = 0;
    app->playback_started_ns = monotonic_nanoseconds();
    app->previous_frame_ns = app->playback_started_ns;
    app->dirty = 0;
    set_status(app, "Running TRUEOS C++ artifact | 640x360 | Ctrl+Enter rebuilds");
    return 1;
}

static void start_compile(App *app) {
    if (app->compiler_pid > 0) {
        set_status(app, "The shader bakery is already running");
        return;
    }
    if (!write_source(app)) {
        snprintf(app->status, sizeof(app->status), "Cannot write %s: %s", SESSION_SOURCE,
                 strerror(errno));
        return;
    }
    int descriptors[2];
    if (pipe(descriptors) != 0) {
        snprintf(app->status, sizeof(app->status), "Cannot create compiler pipe: %s", strerror(errno));
        return;
    }
    pid_t child = fork();
    if (child < 0) {
        close(descriptors[0]);
        close(descriptors[1]);
        snprintf(app->status, sizeof(app->status), "Cannot start compiler: %s", strerror(errno));
        return;
    }
    if (child == 0) {
        close(descriptors[0]);
        dup2(descriptors[1], STDOUT_FILENO);
        dup2(descriptors[1], STDERR_FILENO);
        close(descriptors[1]);
        if (chdir(TRUEOS_REPO_ROOT) != 0) {
            dprintf(STDERR_FILENO, "cannot enter repository root: %s\n", strerror(errno));
            _exit(127);
        }
        execlp("python3", "python3", "-B", COMPILE_SCRIPT, SESSION_SOURCE, (char *)NULL);
        dprintf(STDERR_FILENO, "cannot execute python3: %s\n", strerror(errno));
        _exit(127);
    }
    close(descriptors[1]);
    int flags = fcntl(descriptors[0], F_GETFL, 0);
    fcntl(descriptors[0], F_SETFL, flags | O_NONBLOCK);
    app->compiler_pid = child;
    app->compiler_fd = descriptors[0];
    app->compile_log_length = 0;
    app->compile_log[0] = '\0';
    app->compile_result = -1;
    set_status(app, "Adapting GLSL, then baking C++ -> LLVM -> SPIR-V -> Intel Zebin...");
}

static void consume_compiler_output(App *app) {
    if (app->compiler_fd < 0) {
        return;
    }
    char buffer[4096];
    for (;;) {
        ssize_t count = read(app->compiler_fd, buffer, sizeof(buffer));
        if (count > 0) {
            size_t available = MAX_COMPILE_LOG - 1 - app->compile_log_length;
            size_t copy = (size_t)count < available ? (size_t)count : available;
            if (copy > 0) {
                memcpy(app->compile_log + app->compile_log_length, buffer, copy);
                app->compile_log_length += copy;
                app->compile_log[app->compile_log_length] = '\0';
            }
            continue;
        }
        if (count == 0) {
            close(app->compiler_fd);
            app->compiler_fd = -1;
        }
        break;
    }
}

static const char *last_log_line(const App *app) {
    if (app->compile_log_length == 0) {
        return "No compiler output";
    }
    size_t end = app->compile_log_length;
    while (end > 0 && (app->compile_log[end - 1] == '\n' || app->compile_log[end - 1] == '\r')) {
        --end;
    }
    size_t begin = end;
    while (begin > 0 && app->compile_log[begin - 1] != '\n') {
        --begin;
    }
    return app->compile_log + begin;
}

static void poll_compiler(App *app) {
    if (app->compiler_pid <= 0) {
        return;
    }
    consume_compiler_output(app);
    int status = 0;
    pid_t result = waitpid(app->compiler_pid, &status, WNOHANG);
    if (result == 0) {
        return;
    }
    if (result < 0) {
        snprintf(app->status, sizeof(app->status), "Compiler wait failed: %s", strerror(errno));
        app->compiler_pid = 0;
        return;
    }
    consume_compiler_output(app);
    app->compiler_pid = 0;
    int exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : 128;
    app->compile_result = exit_code;
    if (exit_code == 0) {
        if (!load_program(app)) {
            fprintf(stderr, "%s", app->compile_log);
            fprintf(stderr, "shadertoy-cpp-offline: %s\n", app->status);
        } else {
            printf("shadertoy-cpp-offline: TRUEOS C++ artifact running at %ux%u\n",
                   PREVIEW_WIDTH, PREVIEW_HEIGHT);
            fflush(stdout);
        }
    } else {
        const char *last = last_log_line(app);
        snprintf(app->status, sizeof(app->status), "Build failed (%d): %.400s", exit_code, last);
        fprintf(stderr, "shadertoy-cpp-offline: build failed:\n%s", app->compile_log);
    }
}

static void render_frame(App *app) {
    ShaderRuntime *runtime = &app->runtime;
    if (!runtime->ready) {
        return;
    }
    uint64_t now = monotonic_nanoseconds();
    float time_seconds = (float)(now - app->playback_started_ns) * 1.0e-9f;
    float delta_seconds = (float)(now - app->previous_frame_ns) * 1.0e-9f;
    if (delta_seconds <= 0.0f || delta_seconds > 1.0f) {
        delta_seconds = 1.0f / 60.0f;
    }
    app->previous_frame_ns = now;
    time_t wall = time(NULL);
    struct tm local;
    localtime_r(&wall, &local);
    float seconds_of_day = (float)(local.tm_hour * 3600 + local.tm_min * 60 + local.tm_sec);
    float uniforms[16] = {
        (float)PREVIEW_WIDTH, (float)PREVIEW_HEIGHT, 1.0f, time_seconds,
        app->mouse_x, app->mouse_y,
        app->mouse_down ? app->click_x : -fabsf(app->click_x),
        app->mouse_down ? app->click_y : -fabsf(app->click_y),
        (float)(local.tm_year + 1900), (float)(local.tm_mon + 1),
        (float)local.tm_mday, seconds_of_day,
        delta_seconds, 1.0f / delta_seconds, 44100.0f, (float)app->frame_number,
    };
    cl_int error = clEnqueueWriteBuffer(runtime->queue, runtime->uniform_buffer, CL_TRUE,
                                         0, sizeof(uniforms), uniforms, 0, NULL, NULL);
    const size_t global[2] = {PREVIEW_WIDTH, PREVIEW_HEIGHT};
    const size_t local_size[2] = {16, 1};
    if (error == CL_SUCCESS) {
        error = clEnqueueNDRangeKernel(runtime->queue, runtime->kernel, 2, NULL,
                                       global, local_size, 0, NULL, NULL);
    }
    size_t frame_bytes = (size_t)PREVIEW_WIDTH * PREVIEW_HEIGHT * sizeof(uint32_t);
    if (error == CL_SUCCESS) {
        error = clEnqueueReadBuffer(runtime->queue, runtime->output_buffer, CL_TRUE,
                                    0, frame_bytes, app->frame_pixels, 0, NULL, NULL);
    }
    if (error != CL_SUCCESS) {
        runtime->ready = 0;
        snprintf(app->status, sizeof(app->status), "OpenCL dispatch failed (%d)", error);
        return;
    }
    ++app->frame_number;
    for (size_t i = 0; i < (size_t)PREVIEW_WIDTH * PREVIEW_HEIGHT; ++i) {
        uint32_t rgba = app->frame_pixels[i];
        uint32_t xrgb = ((rgba & 0x000000FFU) << 16)
                      |  (rgba & 0x0000FF00U)
                      | ((rgba & 0x00FF0000U) >> 16);
        ((uint32_t *)app->preview_image->data)[i] = xrgb;
    }
}

static void draw_text_clipped(App *app, int x, int y, const char *text, size_t length, int max_width) {
    int char_width = app->font->max_bounds.width > 0 ? app->font->max_bounds.width : 8;
    size_t maximum = max_width > 0 ? (size_t)(max_width / char_width) : 0;
    if (length > maximum) {
        length = maximum;
    }
    if (length > 0) {
        XDrawString(app->display, app->backbuffer, app->gc, x, y, text, (int)length);
    }
}

static void draw(App *app) {
    XSetForeground(app->display, app->gc, app->background);
    XFillRectangle(app->display, app->backbuffer, app->gc, 0, 0,
                   WINDOW_WIDTH, WINDOW_HEIGHT);

    XSetForeground(app->display, app->gc, app->accent);
    XFillRectangle(app->display, app->backbuffer, app->gc, BUTTON_X, BUTTON_Y,
                   BUTTON_WIDTH, BUTTON_HEIGHT);
    XSetForeground(app->display, app->gc, app->background);
    XDrawString(app->display, app->backbuffer, app->gc, BUTTON_X + 14, BUTTON_Y + 20,
                "RUN  Ctrl+Enter", 15);
    XSetForeground(app->display, app->gc, app->foreground);
    XDrawString(app->display, app->backbuffer, app->gc, 188, 29,
                "ShaderToy Image -> TRUEOS C++ / SPIR-V / ADL-S", 45);

    XSetForeground(app->display, app->gc, app->editor_background);
    XFillRectangle(app->display, app->backbuffer, app->gc, 0, TOOLBAR_HEIGHT,
                   EDITOR_WIDTH, WINDOW_HEIGHT - TOOLBAR_HEIGHT - STATUS_HEIGHT);
    int line_height = app->font->ascent + app->font->descent + 2;
    int baseline = TOOLBAR_HEIGHT + EDITOR_MARGIN + app->font->ascent;
    size_t position = line_at(app, app->scroll_line);
    size_t line_number = app->scroll_line;
    size_t selection_begin;
    size_t selection_end;
    selection_bounds(app, &selection_begin, &selection_end);
    int char_width = app->font->max_bounds.width > 0 ? app->font->max_bounds.width : 8;
    while (position <= app->source_length
           && baseline < WINDOW_HEIGHT - STATUS_HEIGHT - app->font->descent) {
        size_t end = line_end(app, position);
        char number[16];
        snprintf(number, sizeof(number), "%4zu", line_number + 1);
        XSetForeground(app->display, app->gc, app->muted);
        XDrawString(app->display, app->backbuffer, app->gc, 5, baseline, number, 4);
        size_t visible_start = position + app->horizontal_scroll;
        if (visible_start > end) {
            visible_start = end;
        }
        size_t selected_start = selection_begin > visible_start ? selection_begin : visible_start;
        size_t selected_end = selection_end < end ? selection_end : end;
        if (selected_start < selected_end) {
            int selected_x = 45 + (int)(selected_start - visible_start) * char_width;
            unsigned selected_width = (unsigned)(selected_end - selected_start) * (unsigned)char_width;
            if (selected_x < EDITOR_WIDTH - 4) {
                unsigned maximum_width = (unsigned)(EDITOR_WIDTH - 4 - selected_x);
                if (selected_width > maximum_width) {
                    selected_width = maximum_width;
                }
                XSetForeground(app->display, app->gc, app->selection);
                XFillRectangle(app->display, app->backbuffer, app->gc, selected_x,
                               baseline - app->font->ascent, selected_width,
                               (unsigned)line_height);
            }
        }
        XSetForeground(app->display, app->gc, app->foreground);
        draw_text_clipped(app, 45, baseline, app->source + visible_start,
                          end - visible_start, EDITOR_WIDTH - 55);
        if (app->editor_focus && app->cursor >= position && app->cursor <= end) {
            size_t column = app->cursor - position;
            if (column >= app->horizontal_scroll) {
                int cursor_x = 45 + (int)(column - app->horizontal_scroll) * char_width;
                if (cursor_x < EDITOR_WIDTH - 4) {
                    XSetForeground(app->display, app->gc, app->accent);
                    XDrawLine(app->display, app->backbuffer, app->gc, cursor_x,
                              baseline - app->font->ascent, cursor_x,
                              baseline + app->font->descent);
                }
            }
        }
        if (end == app->source_length) {
            break;
        }
        position = end + 1;
        ++line_number;
        baseline += line_height;
    }

    XSetForeground(app->display, app->gc, app->panel);
    XFillRectangle(app->display, app->backbuffer, app->gc, EDITOR_WIDTH, TOOLBAR_HEIGHT,
                   WINDOW_WIDTH - EDITOR_WIDTH, WINDOW_HEIGHT - TOOLBAR_HEIGHT - STATUS_HEIGHT);
    XSetForeground(app->display, app->gc, app->muted);
    XDrawString(app->display, app->backbuffer, app->gc, PREVIEW_X, 91,
                "OUTPUT  640 x 360", 17);
    if (app->runtime.ready) {
        XPutImage(app->display, app->backbuffer, app->gc, app->preview_image,
                  0, 0, PREVIEW_X, PREVIEW_Y, PREVIEW_WIDTH, PREVIEW_HEIGHT);
    } else {
        XSetForeground(app->display, app->gc, app->editor_background);
        XFillRectangle(app->display, app->backbuffer, app->gc, PREVIEW_X, PREVIEW_Y,
                       PREVIEW_WIDTH, PREVIEW_HEIGHT);
        XSetForeground(app->display, app->gc, app->muted);
        const char *message = app->compiler_pid > 0 ? "BAKING..." : "PASTE SHADER, THEN RUN";
        XDrawString(app->display, app->backbuffer, app->gc, PREVIEW_X + 220,
                    PREVIEW_Y + PREVIEW_HEIGHT / 2, message, (int)strlen(message));
    }
    XSetForeground(app->display, app->gc, app->background);
    XFillRectangle(app->display, app->backbuffer, app->gc, 0,
                   WINDOW_HEIGHT - STATUS_HEIGHT, WINDOW_WIDTH, STATUS_HEIGHT);
    XSetForeground(app->display, app->gc,
                   app->compile_result > 0 ? app->error : app->foreground);
    draw_text_clipped(app, 12, WINDOW_HEIGHT - 16, app->status, strlen(app->status),
                      WINDOW_WIDTH - 24);
    XCopyArea(app->display, app->backbuffer, app->window, app->gc,
              0, 0, WINDOW_WIDTH, WINDOW_HEIGHT, 0, 0);
    XFlush(app->display);
}

static size_t click_to_position(App *app, int x, int y) {
    int line_height = app->font->ascent + app->font->descent + 2;
    size_t line = app->scroll_line;
    if (y > TOOLBAR_HEIGHT + EDITOR_MARGIN) {
        line += (size_t)((y - TOOLBAR_HEIGHT - EDITOR_MARGIN) / line_height);
    }
    size_t start = line_at(app, line);
    size_t end = line_end(app, start);
    int char_width = app->font->max_bounds.width > 0 ? app->font->max_bounds.width : 8;
    size_t column = app->horizontal_scroll;
    if (x > 45) {
        column += (size_t)((x - 45 + char_width / 2) / char_width);
    }
    return start + (column < end - start ? column : end - start);
}

static void request_paste(App *app) {
    XConvertSelection(app->display, app->clipboard, app->utf8_string,
                      app->clipboard_property, app->window, CurrentTime);
}

static void handle_selection(App *app, XSelectionEvent *event) {
    if (event->property == None) {
        set_status(app, "Clipboard does not contain UTF-8 text");
        return;
    }
    Atom type;
    int format;
    unsigned long items;
    unsigned long remaining;
    unsigned char *data = NULL;
    int result = XGetWindowProperty(app->display, app->window, event->property,
                                    0, MAX_SHADER_BYTES / 4, True, AnyPropertyType,
                                    &type, &format, &items, &remaining, &data);
    if (result != Success || data == NULL || format != 8 || remaining != 0) {
        if (data != NULL) {
            XFree(data);
        }
        set_status(app, "Clipboard paste failed or exceeds 1 MiB");
        return;
    }
    size_t normalized_length = 0;
    char *normalized = normalize_pasted_text(
        app, data, (size_t)items, &normalized_length);
    if (normalized != NULL) {
        replace_selection(app, normalized, normalized_length);
        free(normalized);
        set_status(app, "Pasted source; tabs and web clipboard whitespace normalized");
    }
    XFree(data);
    keep_cursor_visible(app);
}

static void handle_key(App *app, XKeyEvent *event) {
    char text[64];
    KeySym key = NoSymbol;
    int length = XLookupString(event, text, sizeof(text), &key, NULL);
    int control = (event->state & ControlMask) != 0;
    if (control && (key == XK_Return || key == XK_KP_Enter)) {
        start_compile(app);
        return;
    }
    if (control && (key == XK_v || key == XK_V)) {
        request_paste(app);
        return;
    }
    if (control && (key == XK_a || key == XK_A)) {
        app->selection_anchor = 0;
        app->selection_end = app->source_length;
        app->cursor = app->source_length;
        set_status(app, "All source selected; typing or paste replaces it");
        return;
    }
    if (control && (key == XK_s || key == XK_S)) {
        if (write_source(app)) {
            app->dirty = 0;
            set_status(app, "Saved session source");
        }
        return;
    }
    switch (key) {
    case XK_Escape:
        app->running = 0;
        return;
    case XK_BackSpace: {
        size_t begin;
        size_t end;
        selection_bounds(app, &begin, &end);
        if (begin == end && begin > 0) {
            app->selection_anchor = begin - 1;
            app->selection_end = begin;
        }
        replace_selection(app, "", 0);
        break;
    }
    case XK_Delete: {
        size_t begin;
        size_t end;
        selection_bounds(app, &begin, &end);
        if (begin == end && end < app->source_length) {
            app->selection_anchor = begin;
            app->selection_end = end + 1;
        }
        replace_selection(app, "", 0);
        break;
    }
    case XK_Left:
        if (app->cursor > 0) --app->cursor;
        app->selection_anchor = app->selection_end = app->cursor;
        break;
    case XK_Right:
        if (app->cursor < app->source_length) ++app->cursor;
        app->selection_anchor = app->selection_end = app->cursor;
        break;
    case XK_Up:
        move_cursor_vertical(app, -1);
        return;
    case XK_Down:
        move_cursor_vertical(app, 1);
        return;
    case XK_Home:
        app->cursor = line_start(app, app->cursor);
        app->selection_anchor = app->selection_end = app->cursor;
        break;
    case XK_End:
        app->cursor = line_end(app, app->cursor);
        app->selection_anchor = app->selection_end = app->cursor;
        break;
    case XK_Page_Up:
        app->scroll_line = app->scroll_line > 20 ? app->scroll_line - 20 : 0;
        return;
    case XK_Page_Down:
        app->scroll_line += 20;
        return;
    case XK_Return:
    case XK_KP_Enter:
        replace_selection(app, "\n", 1);
        break;
    case XK_Tab:
        replace_selection(app, "    ", 4);
        break;
    default:
        if (!control && length > 0 && (unsigned char)text[0] >= 0x20) {
            replace_selection(app, text, (size_t)length);
        }
        break;
    }
    keep_cursor_visible(app);
}

static void handle_event(App *app, XEvent *event) {
    if (event->type == ClientMessage
        && (Atom)event->xclient.data.l[0] == app->wm_delete) {
        app->running = 0;
    } else if (event->type == DestroyNotify) {
        app->running = 0;
    } else if (event->type == KeyPress) {
        handle_key(app, &event->xkey);
    } else if (event->type == SelectionNotify) {
        handle_selection(app, &event->xselection);
    } else if (event->type == ButtonPress) {
        int x = event->xbutton.x;
        int y = event->xbutton.y;
        if (x >= BUTTON_X && x < BUTTON_X + BUTTON_WIDTH
            && y >= BUTTON_Y && y < BUTTON_Y + BUTTON_HEIGHT) {
            start_compile(app);
        } else if (x < EDITOR_WIDTH && y >= TOOLBAR_HEIGHT
                   && y < WINDOW_HEIGHT - STATUS_HEIGHT) {
            app->editor_focus = 1;
            app->cursor = click_to_position(app, x, y);
            app->selection_anchor = app->selection_end = app->cursor;
        } else if (x >= PREVIEW_X && x < PREVIEW_X + PREVIEW_WIDTH
                   && y >= PREVIEW_Y && y < PREVIEW_Y + PREVIEW_HEIGHT) {
            app->editor_focus = 0;
            app->mouse_x = (float)(x - PREVIEW_X);
            app->mouse_y = (float)(PREVIEW_HEIGHT - (y - PREVIEW_Y));
            app->click_x = app->mouse_x;
            app->click_y = app->mouse_y;
            app->mouse_down = 1;
        }
    } else if (event->type == ButtonRelease) {
        app->mouse_down = 0;
    } else if (event->type == MotionNotify) {
        int x = event->xmotion.x;
        int y = event->xmotion.y;
        if (x >= PREVIEW_X && x < PREVIEW_X + PREVIEW_WIDTH
            && y >= PREVIEW_Y && y < PREVIEW_Y + PREVIEW_HEIGHT) {
            app->mouse_x = (float)(x - PREVIEW_X);
            app->mouse_y = (float)(PREVIEW_HEIGHT - (y - PREVIEW_Y));
        }
    }
}

static int initialize_window(App *app) {
    app->display = XOpenDisplay(NULL);
    if (app->display == NULL) {
        fprintf(stderr, "shadertoy-cpp-offline: cannot open X11/Xwayland display\n");
        return 0;
    }
    int screen = DefaultScreen(app->display);
    Window root = RootWindow(app->display, screen);
    unsigned long black = BlackPixel(app->display, screen);
    unsigned long white = WhitePixel(app->display, screen);
    app->background = color(app->display, screen, "#101319", black);
    app->panel = color(app->display, screen, "#181d26", black);
    app->editor_background = color(app->display, screen, "#0c0f14", black);
    app->foreground = color(app->display, screen, "#e6eaf2", white);
    app->muted = color(app->display, screen, "#7f8999", white);
    app->accent = color(app->display, screen, "#6ee7c8", white);
    app->selection = color(app->display, screen, "#264d51", black);
    app->error = color(app->display, screen, "#ff7a90", white);
    int x = (DisplayWidth(app->display, screen) - WINDOW_WIDTH) / 2;
    int y = (DisplayHeight(app->display, screen) - WINDOW_HEIGHT) / 2;
    app->window = XCreateSimpleWindow(app->display, root, x, y,
                                       WINDOW_WIDTH, WINDOW_HEIGHT, 0,
                                       app->background, app->background);
    XSelectInput(app->display, app->window,
                 ExposureMask | KeyPressMask | ButtonPressMask | ButtonReleaseMask
                 | PointerMotionMask | StructureNotifyMask);
    XStoreName(app->display, app->window, "TRUEOS ShaderToy C++ Preview");
    XSizeHints hints;
    memset(&hints, 0, sizeof(hints));
    hints.flags = PMinSize | PMaxSize;
    hints.min_width = hints.max_width = WINDOW_WIDTH;
    hints.min_height = hints.max_height = WINDOW_HEIGHT;
    XSetWMNormalHints(app->display, app->window, &hints);
    app->wm_delete = XInternAtom(app->display, "WM_DELETE_WINDOW", False);
    XSetWMProtocols(app->display, app->window, &app->wm_delete, 1);
    app->clipboard = XInternAtom(app->display, "CLIPBOARD", False);
    app->utf8_string = XInternAtom(app->display, "UTF8_STRING", False);
    app->clipboard_property = XInternAtom(app->display, "TRUEOS_SHADER_SOURCE", False);
    app->backbuffer = XCreatePixmap(app->display, app->window,
                                     WINDOW_WIDTH, WINDOW_HEIGHT,
                                     (unsigned)DefaultDepth(app->display, screen));
    app->gc = XCreateGC(app->display, app->window, 0, NULL);
    app->font = XLoadQueryFont(app->display, "-misc-fixed-medium-r-normal--13-*-*-*-*-*-iso10646-1");
    if (app->font == NULL) {
        app->font = XLoadQueryFont(app->display, "fixed");
    }
    if (app->backbuffer == 0 || app->gc == NULL || app->font == NULL) {
        fprintf(stderr, "shadertoy-cpp-offline: cannot create X11 drawing resources\n");
        return 0;
    }
    XSetFont(app->display, app->gc, app->font->fid);
    app->preview_image = XCreateImage(app->display, DefaultVisual(app->display, screen),
                                      (unsigned)DefaultDepth(app->display, screen), ZPixmap,
                                      0, NULL, PREVIEW_WIDTH, PREVIEW_HEIGHT, 32, 0);
    if (app->preview_image == NULL) {
        fprintf(stderr, "shadertoy-cpp-offline: cannot create preview XImage\n");
        return 0;
    }
    app->preview_image->data = calloc((size_t)app->preview_image->bytes_per_line, PREVIEW_HEIGHT);
    app->frame_pixels = calloc((size_t)PREVIEW_WIDTH * PREVIEW_HEIGHT, sizeof(uint32_t));
    if (app->preview_image->data == NULL || app->frame_pixels == NULL) {
        fprintf(stderr, "shadertoy-cpp-offline: out of memory for preview\n");
        return 0;
    }
    XMapRaised(app->display, app->window);
    XFlush(app->display);
    for (;;) {
        XEvent event;
        XNextEvent(app->display, &event);
        if (event.type == MapNotify && event.xmap.window == app->window) {
            break;
        }
    }
    XSetInputFocus(app->display, app->window, RevertToParent, CurrentTime);
    XFlush(app->display);
    return 1;
}

static void destroy_app(App *app) {
    if (app->compiler_pid > 0) {
        kill(app->compiler_pid, SIGTERM);
        waitpid(app->compiler_pid, NULL, 0);
    }
    if (app->compiler_fd >= 0) {
        close(app->compiler_fd);
    }
    release_runtime(&app->runtime);
    free(app->frame_pixels);
    if (app->preview_image != NULL) {
        XDestroyImage(app->preview_image);
    }
    if (app->font != NULL) {
        XFreeFont(app->display, app->font);
    }
    if (app->gc != NULL) {
        XFreeGC(app->display, app->gc);
    }
    if (app->backbuffer != 0) {
        XFreePixmap(app->display, app->backbuffer);
    }
    if (app->window != 0) {
        XDestroyWindow(app->display, app->window);
    }
    if (app->display != NULL) {
        XCloseDisplay(app->display);
    }
    free(app->compile_log);
    free(app->source);
}

int main(int argc, char **argv) {
    if (argc > 2 || (argc == 2 && strcmp(argv[1], "--help") == 0)) {
        fprintf(argc > 2 ? stderr : stdout,
                "usage: shadertoy_cpp_offline [shader.glsl]\n"
                "Paste with Ctrl+V and compile/run with Ctrl+Enter.\n");
        return argc > 2 ? EXIT_FAILURE : EXIT_SUCCESS;
    }
    App app;
    memset(&app, 0, sizeof(app));
    app.compiler_fd = -1;
    app.compile_result = -1;
    app.compile_log = calloc(MAX_COMPILE_LOG, 1);
    const char *initial_path = argc == 2 ? argv[1] : SESSION_SOURCE;
    if (!read_file(initial_path, &app.source, &app.source_length)) {
        app.source_length = strlen(DEFAULT_SHADER);
        app.source_capacity = app.source_length + 1;
        app.source = malloc(app.source_capacity);
        if (app.source != NULL) {
            memcpy(app.source, DEFAULT_SHADER, app.source_capacity);
        }
    } else {
        app.source_capacity = app.source_length + 1;
    }
    if (app.source == NULL || app.compile_log == NULL) {
        fprintf(stderr, "shadertoy-cpp-offline: out of host memory\n");
        destroy_app(&app);
        return EXIT_FAILURE;
    }
    /* Paste-first workflow: the loaded/default source starts selected, so the
     * first Ctrl+V replaces it instead of creating two mainImage functions. */
    app.cursor = 0;
    app.selection_anchor = 0;
    app.selection_end = app.source_length;
    app.editor_focus = 1;
    app.running = 1;
    set_status(&app, "Source selected: Ctrl+V replaces it | Ctrl+Enter runs it");
    if (!initialize_window(&app)) {
        destroy_app(&app);
        return EXIT_FAILURE;
    }

    uint64_t next_frame = monotonic_nanoseconds();
    while (app.running) {
        while (XPending(app.display) > 0) {
            XEvent event;
            XNextEvent(app.display, &event);
            handle_event(&app, &event);
        }
        poll_compiler(&app);
        uint64_t now = monotonic_nanoseconds();
        if (now >= next_frame) {
            render_frame(&app);
            draw(&app);
            next_frame = now + 1000000000ULL / 60;
        }
        struct pollfd fds[2];
        nfds_t count = 0;
        fds[count++] = (struct pollfd){.fd = ConnectionNumber(app.display), .events = POLLIN};
        if (app.compiler_fd >= 0) {
            fds[count++] = (struct pollfd){.fd = app.compiler_fd, .events = POLLIN | POLLHUP};
        }
        (void)poll(fds, count, 8);
    }
    if (app.dirty) {
        (void)write_source(&app);
    }
    destroy_app(&app);
    return EXIT_SUCCESS;
}
