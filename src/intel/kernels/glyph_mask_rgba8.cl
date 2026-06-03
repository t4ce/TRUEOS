// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Mask source is linear 8-bit coverage.
// - Destination is a linear RGBA8 buffer packed as AABBGGRR in a u32.
// - Color is packed as AABBGGRR; effective alpha is color.a * mask / 255.
// - Source-over blend into destination.

static inline uint div255(uint value)
{
    return (value + 127u) / 255u;
}

static inline uint blend_channel(uint src, uint dst, uint src_alpha)
{
    return div255(src * src_alpha + dst * (255u - src_alpha));
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void glyph_mask_rgba8(
    __global const uchar *mask_u8,
    __global uint *dst_rgba,
    uint mask_pitch_bytes,
    uint dst_pitch_bytes,
    uint mask_x,
    uint mask_y,
    uint dst_x,
    uint dst_y,
    uint width,
    uint height,
    uint color_rgba)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);

    if (x >= width || y >= height) {
        return;
    }

    uint coverage = mask_u8[(mask_y + y) * mask_pitch_bytes + mask_x + x];
    if (coverage == 0u) {
        return;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint dst_index = (dst_y + y) * dst_pitch_pixels + dst_x + x;
    uint dst = dst_rgba[dst_index];

    uint color_alpha = (color_rgba >> 24) & 0xFFu;
    uint effective_alpha = div255(color_alpha * coverage);

    uint cr = color_rgba & 0xFFu;
    uint cg = (color_rgba >> 8) & 0xFFu;
    uint cb = (color_rgba >> 16) & 0xFFu;
    uint da = (dst >> 24) & 0xFFu;
    uint dr = dst & 0xFFu;
    uint dg = (dst >> 8) & 0xFFu;
    uint db = (dst >> 16) & 0xFFu;

    uint out_r = blend_channel(cr, dr, effective_alpha);
    uint out_g = blend_channel(cg, dg, effective_alpha);
    uint out_b = blend_channel(cb, db, effective_alpha);
    uint out_a = effective_alpha + div255(da * (255u - effective_alpha));

    dst_rgba[dst_index] = (out_a << 24) | (out_b << 16) | (out_g << 8) | out_r;
}

