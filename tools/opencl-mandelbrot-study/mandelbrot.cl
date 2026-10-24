#pragma OPENCL FP_CONTRACT OFF

#ifdef FORCE_SIMD8
__attribute__((intel_reqd_sub_group_size(8)))
#endif
__kernel void mandelbrot(__global uint *iterations, uint width, uint height,
                         float origin_x, float origin_y, float step,
                         uint max_iterations)
{
    uint x = (uint)get_global_id(0), y = (uint)get_global_id(1);
    if (x >= width || y >= height) return;
    float cx = origin_x + (float)x * step, cy = origin_y + (float)y * step;
    float zx = 0.0f, zy = 0.0f;
    uint n = 0;
    while (n < max_iterations && zx * zx + zy * zy <= 4.0f) {
        float next_x = zx * zx - zy * zy + cx;
        zy = 2.0f * zx * zy + cy;
        zx = next_x;
        ++n;
    }
    iterations[y * width + x] = n;
}
