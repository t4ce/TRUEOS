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
        fprintf(stderr, "usage: benchmark_cube_field kernel.spv output-directory [width height]\n");
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
    error = clBuildProgram(runtime->program, 1, &runtime->device, NULL, NULL, NULL);
    if (error != CL_SUCCESS) {
        char log[16384] = {0};
        clGetProgramBuildInfo(runtime->program, runtime->device, CL_PROGRAM_BUILD_LOG, sizeof(log), log, NULL);
        fprintf(stderr, "%s\n", log);
        require_cl(error, "build");
    }
    runtime->kernel = clCreateKernel(runtime->program, "shadertoy_cube_field", &error);
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
    int animation_changed = 0, repeat_equal = 0;
    const float times[] = {0,0,0,2,4,6,8,0,0};
    for (int frame = 0; frame < 9; ++frame) {
        memset(uniforms, 0, sizeof(uniforms));
        uniforms[0] = (float)width; uniforms[1] = (float)height; uniforms[2] = 1.f;
        uniforms[3] = times[frame];
        uniforms[12] = 1.f / 60.f; uniforms[13] = 60.f;
        require_cl(clEnqueueWriteBuffer(runtime->queue, runtime->uniform_buffer, CL_TRUE, 0, sizeof(uniforms), uniforms, 0, NULL, NULL), "write uniforms");
        uint64_t start = monotonic_nanoseconds();
        require_cl(clEnqueueNDRangeKernel(runtime->queue, runtime->kernel, 2, NULL, global, local, 0, NULL, NULL), "dispatch");
        require_cl(clFinish(runtime->queue), "finish dispatch");
        double elapsed_ms = (monotonic_nanoseconds() - start) / 1000000.0;
        require_cl(clEnqueueReadBuffer(runtime->queue, runtime->output_buffer, CL_TRUE, 0, bytes, pixels, 0, NULL, NULL), "read frame");
        if (frame == 0) memcpy(first, pixels, bytes);
        if (frame == 4) animation_changed = memcmp(first, pixels, bytes) != 0;
        if (frame == 8) repeat_equal = memcmp(first, pixels, bytes) == 0;
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
        printf("frame=%d time=%.1f mouse=%d dispatch_finish_ms=%.3f output=%s\n", frame, uniforms[3], 0, elapsed_ms, path);
        fflush(stdout);
    }
    printf("animation_changes_frame=%d repeat_t0_bit_identical=%d\n", animation_changed, repeat_equal);
    free(pixels); free(first); release_runtime(runtime); free(app);
    return animation_changed && repeat_equal ? 0 : 3;
}
