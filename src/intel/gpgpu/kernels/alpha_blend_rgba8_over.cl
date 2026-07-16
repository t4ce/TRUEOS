// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Source and destination are linear RGBA8 buffers.
// - Pixels are packed as AABBGGRR in a u32.
// - Source-over accepts straight or premultiplied source RGB.
// - opacity scales source alpha and, for premultiplied input, source RGB.
// - No scaling, filtering, or color conversion.

#define ALPHA_BLEND_FLAG_PREMUL_SRC (1u << 0)

static inline uint div255(uint value)
{
    return (value + 127u) / 255u;
}

static inline uint blend_channel_straight(uint src, uint dst, uint src_alpha)
{
    return div255(src * src_alpha + dst * (255u - src_alpha));
}

static inline uint blend_channel_premul(uint src, uint dst, uint src_alpha)
{
    return src + div255(dst * (255u - src_alpha));
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void alpha_blend_rgba8_over(
    __global const uint *src_rgba,
    __global uint *dst_rgba,
    uint src_pitch_bytes,
    uint dst_pitch_bytes,
    uint src_x,
    uint src_y,
    uint dst_x,
    uint dst_y,
    uint width,
    uint height,
    uint flags,
    uint opacity)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);

    if (x >= width || y >= height) {
        return;
    }

    uint src_pitch_pixels = src_pitch_bytes >> 2;
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint src_index = (src_y + y) * src_pitch_pixels + src_x + x;
    uint dst_index = (dst_y + y) * dst_pitch_pixels + dst_x + x;

    uint src = src_rgba[src_index];
    uint dst = dst_rgba[dst_index];
    uint sa = (src >> 24) & 0xFFu;
    uint da = (dst >> 24) & 0xFFu;

    uint sr = src & 0xFFu;
    uint sg = (src >> 8) & 0xFFu;
    uint sb = (src >> 16) & 0xFFu;
    uint dr = dst & 0xFFu;
    uint dg = (dst >> 8) & 0xFFu;
    uint db = (dst >> 16) & 0xFFu;

    opacity = min(opacity, 255u);
    sa = div255(sa * opacity);

    uint out_r;
    uint out_g;
    uint out_b;
    if ((flags & ALPHA_BLEND_FLAG_PREMUL_SRC) != 0u) {
        sr = div255(sr * opacity);
        sg = div255(sg * opacity);
        sb = div255(sb * opacity);
        out_r = blend_channel_premul(sr, dr, sa);
        out_g = blend_channel_premul(sg, dg, sa);
        out_b = blend_channel_premul(sb, db, sa);
    } else {
        out_r = blend_channel_straight(sr, dr, sa);
        out_g = blend_channel_straight(sg, dg, sa);
        out_b = blend_channel_straight(sb, db, sa);
    }
    uint out_a = sa + div255(da * (255u - sa));

    dst_rgba[dst_index] = (out_a << 24) | (out_b << 16) | (out_g << 8) | out_r;
}
