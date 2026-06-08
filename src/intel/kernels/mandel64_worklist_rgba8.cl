// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Destination is a linear RGBA8 buffer packed as AABBGGRR in a u32.
// - Each descriptor draws one clipped Mandelbrot row-band, up to 64x4 pixels.
// - desc.src_xy is a signed 16-bit Mandelbrot-space pixel offset.
// - desc.dst_xy is a signed 16-bit destination pixel coordinate.
// - One SIMD16 walker consumes a descriptor slice:
//   lane N draws descriptors desc_base + N, desc_base + N+16, ...
// - desc.color_rgba packs the per-pixel iteration cap and grayscale scale.

#define MANDEL64_BAND_ROWS 4u
#define MANDEL64_BAND_COLS 64u
#define MANDEL64_FLAG_ROWS_MASK 0x000000FFu
#define MANDEL64_FLAG_COLS_SHIFT 8u
#define MANDEL64_FLAG_COLS_MASK 0x0000FF00u
#define MANDEL64_FLAG_MIRROR_HEIGHT_SHIFT 16u
#define MANDEL64_DEFAULT_ITER 32u
#define MANDEL64_MAX_ITER 512u
#define MANDEL64_DEFAULT_GRAY_SCALE 2040u

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

static inline uint mandel_gray(
    int src_x,
    int src_y,
    uint local_x,
    uint local_y,
    uint view_height,
    uint max_iter,
    uint gray_scale)
{
    // Q12 fixed-point mapping over the current scanout:
    // real [-2, +1], imaginary [-1, +1].
    int cr = -8192 + ((src_x + (int)local_x) * 12288) / 2560;
    int ci = -4096 + ((src_y + (int)local_y) * 8192) / (int)view_height;
    int zr = 0;
    int zi = 0;
    uint iter = 0;

    for (; iter < MANDEL64_MAX_ITER; iter++) {
        if (iter >= max_iter) {
            break;
        }
        int zr2 = (zr * zr) >> 12;
        int zi2 = (zi * zi) >> 12;
        if (zr2 + zi2 > 16384) {
            break;
        }

        int zri = (zr * zi) >> 11;
        zr = zr2 - zi2 + cr;
        zi = zri + ci;
    }

    if (iter >= max_iter) {
        return 0xFF000000u;
    }

    uint shade = (iter * gray_scale) >> 8;
    if (shade > 255u) {
        shade = 255u;
    }
    uint color = 0xFF000000u | (shade << 16) | (shade << 8) | shade;
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
        uint band_rows = desc.flags & MANDEL64_FLAG_ROWS_MASK;
        uint band_cols = (desc.flags & MANDEL64_FLAG_COLS_MASK) >> MANDEL64_FLAG_COLS_SHIFT;
        uint mirror_height = desc.flags >> MANDEL64_FLAG_MIRROR_HEIGHT_SHIFT;
        uint view_height = mirror_height == 0u ? 1440u : mirror_height;
        uint max_iter = desc.color_rgba & 0xFFFFu;
        uint gray_scale = desc.color_rgba >> 16;
        if (band_rows == 0u || band_rows > MANDEL64_BAND_ROWS) {
            band_rows = MANDEL64_BAND_ROWS;
        }
        if (band_cols == 0u || band_cols > MANDEL64_BAND_COLS) {
            band_cols = MANDEL64_BAND_COLS;
        }
        if (max_iter == 0u) {
            max_iter = MANDEL64_DEFAULT_ITER;
        }
        if (max_iter > MANDEL64_MAX_ITER) {
            max_iter = MANDEL64_MAX_ITER;
        }
        if (gray_scale == 0u) {
            gray_scale = MANDEL64_DEFAULT_GRAY_SCALE;
        }

        for (uint y = 0u; y < band_rows; y++) {
            int out_y = dst_y + (int)y;
            if (out_y < 0) {
                continue;
            }

            for (uint x = 0u; x < band_cols; x++) {
                int out_x = dst_x + (int)x;
                if (out_x < 0) {
                    continue;
                }

                uint color = mandel_gray(src_x, src_y, x, y, view_height, max_iter, gray_scale);
                uint dst_index = (uint)out_y * dst_pitch_pixels + (uint)out_x;
                dst_rgba[dst_index] = color;
                if (mirror_height != 0u) {
                    int mirror_y = (int)mirror_height - 1 - out_y;
                    if (mirror_y >= 0 && mirror_y != out_y) {
                        uint mirror_index = (uint)mirror_y * dst_pitch_pixels + (uint)out_x;
                        dst_rgba[mirror_index] = color;
                    }
                }
            }
        }
    }
}
