#define main shadertoy_editor_main
#include "main.c"
#undef main

static void require_cl(cl_int result, const char *operation) {
    if (result != CL_SUCCESS) {
        fprintf(stderr, "%s: OpenCL error %d\n", operation, result);
        exit(2);
    }
}

int main(int argc, char **argv) {
    if (argc != 3 && argc != 5) {
        fprintf(stderr, "usage: benchmark_protean_clouds kernel.spv output-directory [width height]\n");
        return 2;
    }
    App *app = calloc(1, sizeof(*app));
    if (!app || !initialize_opencl(app)) {
        fprintf(stderr, "OpenCL init: %s\n", app ? app->status : "allocation failed");
        return 2;
    }
    ShaderRuntime *runtime = &app->runtime;
    char device_name[256] = {0};
    require_cl(clGetDeviceInfo(runtime->device, CL_DEVICE_NAME, sizeof(device_name), device_name, NULL), "device name");
    printf("device=%s\n", device_name);
    fflush(stdout);
    char *spirv = NULL;
    size_t spirv_size = 0;
    if (!read_file(argv[1], &spirv, &spirv_size)) return 2;
    cl_int error = 0;
    runtime->program = clCreateProgramWithIL(runtime->context, spirv, spirv_size, &error);
    free(spirv);
    require_cl(error, "IL program");
    // Set SHADERTOY_BUILD_OPTIONS to match the artifact manifest; unset keeps
    // the strict baseline. The option is backend metadata, not part of SPIR-V.
    error = clBuildProgram(runtime->program, 1, &runtime->device,
                           getenv("SHADERTOY_BUILD_OPTIONS"), NULL, NULL);
    if (error != CL_SUCCESS) {
        char log[16384] = {0};
        clGetProgramBuildInfo(runtime->program, runtime->device, CL_PROGRAM_BUILD_LOG, sizeof(log), log, NULL);
        fprintf(stderr, "%s\n", log);
        require_cl(error, "build");
    }
    const char *kernel_name = getenv("SHADERTOY_KERNEL");
    runtime->kernel = clCreateKernel(runtime->program,
        kernel_name ? kernel_name : "shadertoy_protean_clouds", &error);
    require_cl(error, "kernel");
    const cl_uint width = argc == 5 ? (cl_uint)atoi(argv[3]) : 640;
    const cl_uint height = argc == 5 ? (cl_uint)atoi(argv[4]) : 360, pitch = width * 4;
    const size_t bytes = (size_t)pitch * height;
    uint8_t *pixels = malloc(bytes), *first = malloc(bytes);
    if (!pixels || !first) return 2;
    float uniforms[16] = {0};
    runtime->output_buffer = clCreateBuffer(runtime->context, CL_MEM_READ_WRITE, bytes, NULL, &error);
    require_cl(error, "output buffer");
    runtime->uniform_buffer = clCreateBuffer(runtime->context, CL_MEM_READ_ONLY, sizeof(uniforms), NULL, &error);
    require_cl(error, "uniform buffer");
    require_cl(clSetKernelArg(runtime->kernel, 0, sizeof(runtime->output_buffer), &runtime->output_buffer), "output arg");
    require_cl(clSetKernelArg(runtime->kernel, 1, sizeof(runtime->uniform_buffer), &runtime->uniform_buffer), "uniform arg");
    require_cl(clSetKernelArg(runtime->kernel, 2, sizeof(width), &width), "width arg");
    require_cl(clSetKernelArg(runtime->kernel, 3, sizeof(height), &height), "height arg");
    require_cl(clSetKernelArg(runtime->kernel, 4, sizeof(pitch), &pitch), "pitch arg");
    const size_t global[2] = {(width + 15u) & ~15u, height}, local[2] = {16, 1};
    const char *batch_limit = getenv("SHADERTOY_MAX_BATCH_PIXELS");
    const size_t max_pixels = batch_limit ? strtoul(batch_limit, NULL, 10) : 0;
    const size_t batch_rows = max_pixels ? max_pixels / global[0] : height;
    if (!batch_rows) return 2;
    int animation_changed = 0, repeat_equal = 0;
    const float times[] = {0,0,0,2,4,6,8,12,20,35,60,120,0,4,8,0};
    for (int frame = 0; frame < 16; ++frame) {
        memset(uniforms, 0, sizeof(uniforms));
        uniforms[0] = (float)width; uniforms[1] = (float)height; uniforms[2] = 1.f;
        uniforms[3] = times[frame];
        if (frame == 12) { uniforms[4] = width*.5f; uniforms[5] = height*.5f; }
        if (frame == 13) { uniforms[4] = width*.2f; uniforms[5] = height*.8f; }
        if (frame == 14) { uniforms[4] = width*.8f; uniforms[5] = height*.2f; }
        uniforms[12] = 1.f / 60.f; uniforms[13] = 60.f;
        require_cl(clEnqueueWriteBuffer(runtime->queue, runtime->uniform_buffer, CL_TRUE, 0, sizeof(uniforms), uniforms, 0, NULL, NULL), "write uniforms");
        uint64_t start = monotonic_nanoseconds();
        double max_batch_ms = 0.0;
        for (size_t row = 0; row < height; row += batch_rows) {
            const size_t offset[2] = {0, row};
            const size_t extent[2] = {global[0],
                batch_rows < height - row ? batch_rows : height - row};
            const uint64_t batch_start = monotonic_nanoseconds();
            require_cl(clEnqueueNDRangeKernel(runtime->queue, runtime->kernel, 2,
                offset, extent, local, 0, NULL, NULL), "dispatch");
            require_cl(clFinish(runtime->queue), "finish dispatch");
            const double batch_ms = (monotonic_nanoseconds() - batch_start) / 1000000.0;
            if (batch_ms > max_batch_ms) max_batch_ms = batch_ms;
        }
        double elapsed_ms = (monotonic_nanoseconds() - start) / 1000000.0;
        require_cl(clEnqueueReadBuffer(runtime->queue, runtime->output_buffer, CL_TRUE, 0, bytes, pixels, 0, NULL, NULL), "read frame");
        if (frame == 0) memcpy(first, pixels, bytes);
        if (frame == 4) animation_changed = memcmp(first, pixels, bytes) != 0;
        if (frame == 15) repeat_equal = memcmp(first, pixels, bytes) == 0;
        char path[2048];
        snprintf(path, sizeof(path), "%s/frame-%d.ppm", argv[2], frame);
        FILE *output = fopen(path, "wb");
        if (!output) return 2;
        fprintf(output, "P6\n%u %u\n255\n", width, height);
        for (size_t offset = 0; offset < bytes; offset += 4) {
            unsigned char rgb[3] = {pixels[offset], pixels[offset+1], pixels[offset+2]};
            if (fwrite(rgb, 1, 3, output) != 3) return 2;
        }
        if (fclose(output) != 0) return 2;
        printf("frame=%d time=%.1f mouse=%d dispatch_finish_ms=%.3f max_batch_ms=%.3f output=%s\n", frame, uniforms[3], frame >= 12 && frame <= 14, elapsed_ms, max_batch_ms, path);
        fflush(stdout);
    }
    printf("animation_changes_frame=%d repeat_t0_bit_identical=%d\n", animation_changed, repeat_equal);
    free(pixels); free(first); release_runtime(runtime); free(app);
    return animation_changed && repeat_equal ? 0 : 3;
}
