#define main shadertoy_editor_main
#include "main.c"
#undef main

// Keep strict/full and older five-argument artifacts usable as references.
extern cl_int clGetKernelInfo(cl_kernel, cl_uint, size_t, void *, size_t *);

static void require_cl(cl_int result, const char *operation) {
    if (result != CL_SUCCESS) {
        fprintf(stderr, "%s: OpenCL error %d\n", operation, result);
        exit(2);
    }
}


typedef struct { float x, y, z; } FocusV3;
static FocusV3 fv(float x,float y,float z) { return (FocusV3){x,y,z}; }
static FocusV3 fsub(FocusV3 a,FocusV3 b) {return fv(a.x-b.x,a.y-b.y,a.z-b.z);}
static float fdot(FocusV3 a,FocusV3 b) {return a.x*b.x+a.y*b.y+a.z*b.z;}
static FocusV3 fcross(FocusV3 a,FocusV3 b) {return fv(a.y*b.z-a.z*b.y,a.z*b.x-a.x*b.z,a.x*b.y-a.y*b.x);}
static FocusV3 fnorm(FocusV3 a) {float s=1.f/sqrtf(fdot(a,a));return fv(a.x*s,a.y*s,a.z*s);}
static FocusV3 fdisp(float z) {return fv(2.f*sinf(z*.22f),2.f*cosf(z*.175f),z);}
static void focus_controls(float *f,float w,float h,float time,float mx,float my,float boost) {
    (void)my;
    float z=time*3.f, bsx=(mx-.5f*w)/h;
    FocusV3 ro=fdisp(z);ro.x=ro.x*.85f+sinf(time)*.5f;ro.y*=.85f;
    FocusV3 aim=fdisp(z+3.5f);aim.x*=.85f;aim.y*=.85f;
    FocusV3 target=fnorm(fsub(ro,aim));
    FocusV3 right=fnorm(fcross(target,fv(0,1,0)));
    FocusV3 up=fnorm(fcross(right,target));right=fnorm(fcross(up,target));
    ro.x-=bsx*2.f;
    FocusV3 direction=fsub(fdisp(z+8.f),ro);
    float angle=-fdisp(z+3.5f).x*.2f+bsx,c=cosf(angle),s=sinf(angle);
    direction=fv(direction.x*c-direction.y*s,direction.x*s+direction.y*c,direction.z);
    float depth=-fdot(direction,target);
    float cx=w*.5f,cy=h*.5f;
    if(depth>.01f) {cx+=h*fdot(direction,right)/depth;cy-=h*fdot(direction,up)/depth;}
    cx=fminf(fmaxf(cx,w*.15f),w*.85f);cy=fminf(fmaxf(cy,h*.15f),h*.85f);
    float radius=fminf(h*.48f,fminf(fminf(cx,w-cx),fminf(cy,h-cy)));
    f[0]=cx;f[1]=cy;f[2]=radius;f[3]=boost;
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
    cl_uint num_args = 0;
    require_cl(clGetKernelInfo(runtime->kernel, 0x1191, sizeof(num_args), &num_args, NULL), "argument count");
    if (num_args != 5 && num_args != 6) return 2;
    const cl_uint width = argc == 5 ? (cl_uint)atoi(argv[3]) : 640;
    const cl_uint height = argc == 5 ? (cl_uint)atoi(argv[4]) : 360, pitch = width * 4;
    if (!width || !height || width > 16384 || height > 16384) return 2;
    const size_t bytes = (size_t)pitch * height;
    uint8_t *pixels = malloc(bytes), *first = malloc(bytes);
    if (!pixels || !first) return 2;
    float uniforms[24] = {0};
    const char *mode = getenv("FOVEATED_MODE");
    int radial = mode && strcmp(mode,"radial")==0;
    int reduced = radial || (mode && strcmp(mode,"uniform")==0);
    if (reduced && num_args != 6) { fprintf(stderr,"radial/uniform mode requires the six-argument ABI\n"); return 2; }
    double scale=fmin(2.0,fmax(1.0,sqrt((double)width*height/(1280.0*720.0))));
    const cl_uint sw=(cl_uint)ceil(width/scale), sh=(cl_uint)ceil(height/scale), sp=(sw*4+63u)&~63u;
    reduced = reduced && scale > 1.0;
    cl_mem scratch=clCreateBuffer(runtime->context,CL_MEM_READ_WRITE,(size_t)sp*sh,NULL,&error);
    require_cl(error,"scratch");
    runtime->output_buffer = clCreateBuffer(runtime->context, CL_MEM_READ_WRITE, bytes, NULL, &error);
    require_cl(error, "output buffer");
    runtime->uniform_buffer = clCreateBuffer(runtime->context, CL_MEM_READ_ONLY, sizeof(uniforms), NULL, &error);
    require_cl(error, "uniform buffer");
    require_cl(clSetKernelArg(runtime->kernel, 0, sizeof(runtime->output_buffer), &runtime->output_buffer), "output arg");
    require_cl(clSetKernelArg(runtime->kernel, 1, sizeof(runtime->uniform_buffer), &runtime->uniform_buffer), "uniform arg");
    require_cl(clSetKernelArg(runtime->kernel, 2, sizeof(width), &width), "width arg");
    require_cl(clSetKernelArg(runtime->kernel, 3, sizeof(height), &height), "height arg");
    require_cl(clSetKernelArg(runtime->kernel, 4, sizeof(pitch), &pitch), "pitch arg");
    if (num_args == 6) require_cl(clSetKernelArg(runtime->kernel,5,sizeof(scratch),&scratch),"source arg");
    const size_t local[2] = {16,1};
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
        focus_controls(uniforms+20,(float)width,(float)height,uniforms[3],uniforms[4],uniforms[5],radial?fminf((float)width/sw,(float)height/sh):1.f);
        double phase_ms[2]={0,0};
        uint64_t start=monotonic_nanoseconds();
        double max_batch_ms=0;
        for(int pass=0;pass<(reduced?2:1);++pass) {
            cl_uint phase=reduced?(cl_uint)pass+1:0;
            cl_uint pw=phase==1?sw:width, ph=phase==1?sh:height, pp=phase==1?sp:pitch;
            cl_mem dst=phase==1?scratch:runtime->output_buffer;
            cl_uint controls[4]={phase,sw,sh,sp}; memcpy(uniforms+16,controls,sizeof(controls));
            require_cl(clSetKernelArg(runtime->kernel,0,sizeof(dst),&dst),"dst");
            require_cl(clSetKernelArg(runtime->kernel,2,sizeof(pw),&pw),"pw");
            require_cl(clSetKernelArg(runtime->kernel,3,sizeof(ph),&ph),"ph");
            require_cl(clSetKernelArg(runtime->kernel,4,sizeof(pp),&pp),"pp");
            require_cl(clEnqueueWriteBuffer(runtime->queue,runtime->uniform_buffer,CL_TRUE,0,sizeof(uniforms),uniforms,0,NULL,NULL),"uniforms");
            uint64_t phase_start=monotonic_nanoseconds();
            size_t gx=(pw+15u)&~15u, rows=(phase==2?1048576u:131072u)/gx;
            if (!rows) return 2;
            for(size_t row=0;row<ph;row+=rows) {
                const size_t offset[2]={0,row}, extent[2]={gx,rows<ph-row?rows:ph-row};
                uint64_t bs=monotonic_nanoseconds();
                require_cl(clEnqueueNDRangeKernel(runtime->queue,runtime->kernel,2,offset,extent,local,0,NULL,NULL),"dispatch");
                require_cl(clFinish(runtime->queue),"finish");
                double bm=(monotonic_nanoseconds()-bs)/1000000.;if(bm>max_batch_ms)max_batch_ms=bm;
            }
            phase_ms[pass]=(monotonic_nanoseconds()-phase_start)/1000000.;
        }
        double elapsed_ms=(monotonic_nanoseconds()-start)/1000000.;
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
        printf("shade_ms=%.3f resolve_ms=%.3f focus=%.3f,%.3f,%.3f ",phase_ms[0],phase_ms[1],uniforms[20],uniforms[21],uniforms[22]);
        printf("frame=%d time=%.1f mouse=%d dispatch_finish_ms=%.3f max_batch_ms=%.3f output=%s\n", frame, uniforms[3], frame >= 12 && frame <= 14, elapsed_ms, max_batch_ms, path);
        fflush(stdout);
    }
    printf("animation_changes_frame=%d repeat_t0_bit_identical=%d\n", animation_changed, repeat_equal);
    clReleaseMemObject(scratch);
    free(pixels); free(first); release_runtime(runtime); free(app);
    return animation_changed && repeat_equal ? 0 : 3;
}
