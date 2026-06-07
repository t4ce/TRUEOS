// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Destination is a linear RGBA8 buffer packed as AABBGGRR in a u32.
// - Each descriptor draws one fixed 64x4 Mandelbrot row-band.
// - desc.src_xy is a signed 16-bit Mandelbrot-space pixel offset.
// - desc.dst_xy is a signed 16-bit destination pixel coordinate.
// - One SIMD16 walker consumes a descriptor slice:
//   lane N draws descriptors desc_base + N, desc_base + N+16, ...
// - Each output pixel runs up to 256 Mandelbrot iterations.

#define MANDEL64_BAND_ROWS 4u

typedef struct Mandel64Desc {
    uint src_xy;
    uint dst_xy;
    uint flags;
    uint color_rgba;
} Mandel64Desc;

static inline int unpack_i16(uint value)
{
    return (int)((short)(value & 0xFFFFu));
}

static inline uint mandel256_gray(int src_x, int src_y, uint local_x, uint local_y, uint color_rgba)
{
    // Q12 fixed-point mapping over the current 2560x1408 full-tile scanout:
    // real [-2, +1], imaginary [-1, +1].
    int cr = -8192 + ((src_x + (int)local_x) * 12288) / 2560;
    int ci = -4096 + ((src_y + (int)local_y) * 8192) / 1408;
    int zr = 0;
    int zi = 0;
    uint iter = 0;

    for (; iter < 256u; iter++) {
        long zr2 = ((long)zr * (long)zr) >> 12;
        long zi2 = ((long)zi * (long)zi) >> 12;
        if (zr2 + zi2 > 16384) {
            break;
        }

        long zri = ((long)zr * (long)zi) >> 11;
        zr = (int)(zr2 - zi2) + cr;
        zi = (int)zri + ci;
    }

    if (iter == 256u) {
        return 0xFF000000u;
    }

    uint shade = iter & 0xFFu;
    uint color = 0xFF000000u | (shade << 16) | (shade << 8) | shade;

    if (color_rgba != 0u) {
        color ^= color_rgba & 0x00FFFFFFu;
        color |= 0xFF000000u;
    }
    return color;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void mandel64_worklist_rgba8(
    __global uint *dst_rgba,
    __global const Mandel64Desc *descs,
    uint dst_pitch_bytes,
    uint desc_base,
    uint desc_count)
{
    uint lane = get_global_id(0);

    if (lane >= 16u) {
        return;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;

    for (uint local_desc_id = lane; local_desc_id < desc_count; local_desc_id += 16u) {
        uint desc_id = desc_base + local_desc_id;
        Mandel64Desc desc = descs[desc_id];
        int src_x = unpack_i16(desc.src_xy);
        int src_y = unpack_i16(desc.src_xy >> 16);
        int dst_x = unpack_i16(desc.dst_xy);
        int dst_y = unpack_i16(desc.dst_xy >> 16);

        for (uint y = 0u; y < MANDEL64_BAND_ROWS; y++) {
            int out_y = dst_y + (int)y;
            if (out_y < 0) {
                continue;
            }

            for (uint x = 0u; x < 64u; x++) {
                int out_x = dst_x + (int)x;
                if (out_x < 0) {
                    continue;
                }

                uint color = mandel256_gray(src_x, src_y, x, y, desc.color_rgba);
                uint dst_index = (uint)out_y * dst_pitch_pixels + (uint)out_x;
                dst_rgba[dst_index] = color;
            }
        }
    }
}
