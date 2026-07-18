// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Destination is a linear RGBA8 buffer.
// - dst_x/dst_y act as both stamp destination and Mandelbrot view-space offset.
// - Each pixel runs exactly ten Mandelbrot iterations or exits early on divergence.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void stamp_mandel_rgba8(
    __global uint *dst_rgba,
    uint dst_pitch_bytes,
    uint dst_x,
    uint dst_y,
    uint width,
    uint height)
{
    uint base_x = get_global_id(0) << 4;
    uint y = get_global_id(1);

    if (base_x >= width || y >= height || width == 0 || height == 0) {
        return;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;

    for (uint pixel = 0; pixel < 16; pixel++) {
        uint x = base_x + pixel;
        if (x >= width) {
            continue;
        }

        // Q12 fixed-point mapping:
        // real roughly [-2.50, 1.00], imaginary roughly [-1.50, 1.50].
        int cr = -10240 + (int)(((dst_x + x) * 14336u) / width);
        int ci = -6144 + (int)(((dst_y + y) * 12288u) / height);
        int zr = 0;
        int zi = 0;
        uint iter = 0;

        for (; iter < 10; iter++) {
            long zr2 = ((long)zr * (long)zr) >> 12;
            long zi2 = ((long)zi * (long)zi) >> 12;
            if (zr2 + zi2 > 16384) {
                break;
            }

            long zri = ((long)zr * (long)zi) >> 11;
            zr = (int)(zr2 - zi2) + cr;
            zi = (int)zri + ci;
        }

        uint color;
        if (iter == 10) {
            color = 0xFF000000u;
        } else {
            uint shade = 32u + iter * 22u;
            uint red = shade;
            uint green = 255u - (shade >> 1);
            uint blue = 96u + (shade >> 2);
            color = 0xFF000000u | (blue << 16) | (green << 8) | red;
        }

        uint dst_index = (dst_y + y) * dst_pitch_pixels + dst_x + x;
        dst_rgba[dst_index] = color;
    }
}
