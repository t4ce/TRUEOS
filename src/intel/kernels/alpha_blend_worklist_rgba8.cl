// TRUEOS Gen12/Alder Lake GPGPU evo kernel.
//
// Contract:
// - Source and destination are linear RGBA8 buffers packed as AABBGGRR in a u32.
// - Each descriptor performs one unscaled source-over rectangle.
// - One SIMD16 walker consumes a descriptor slice:
//   lane N draws descriptors desc_base + N, desc_base + N+16, ...

static inline int unpack_i16(uint value)
{
    return (int)((short)(value & 0xFFFFu));
}

static inline uint div255(uint value)
{
    return (value + 127u) / 255u;
}

static inline uint blend_channel(uint src, uint dst, uint src_alpha)
{
    return div255(src * src_alpha + dst * (255u - src_alpha));
}

static inline uint src_over(uint src, uint dst)
{
    uint sa = (src >> 24) & 0xFFu;
    if (sa == 0u) {
        return dst;
    }
    if (sa == 255u) {
        return src;
    }

    uint sr = src & 0xFFu;
    uint sg = (src >> 8) & 0xFFu;
    uint sb = (src >> 16) & 0xFFu;
    uint da = (dst >> 24) & 0xFFu;
    uint dr = dst & 0xFFu;
    uint dg = (dst >> 8) & 0xFFu;
    uint db = (dst >> 16) & 0xFFu;

    uint out_r = blend_channel(sr, dr, sa);
    uint out_g = blend_channel(sg, dg, sa);
    uint out_b = blend_channel(sb, db, sa);
    uint out_a = sa + div255(da * (255u - sa));

    return (out_a << 24) | (out_b << 16) | (out_g << 8) | out_r;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void alpha_blend_worklist_rgba8(
    __global const uint *src_rgba,
    __global uint *dst_rgba,
    __global const uint *descs,
    uint src_pitch_bytes,
    uint dst_pitch_bytes,
    uint desc_base,
    uint desc_count)
{
    uint lane = get_global_id(0);

    if (lane >= 16u) {
        return;
    }

    uint src_pitch_pixels = src_pitch_bytes >> 2;
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;

    for (uint local_desc_id = lane; local_desc_id < desc_count; local_desc_id += 16u) {
        uint desc_index = (desc_base + local_desc_id) * 3u;
        uint src_xy = descs[desc_index + 0u];
        uint dst_xy = descs[desc_index + 1u];
        uint size = descs[desc_index + 2u];
        uint src_x = src_xy & 0xFFFFu;
        uint src_y = src_xy >> 16;
        int dst_x = unpack_i16(dst_xy);
        int dst_y = unpack_i16(dst_xy >> 16);
        uint width = size & 0xFFFFu;
        uint height = size >> 16;

        for (uint y = 0; y < height; y++) {
            int out_y = dst_y + (int)y;
            if (out_y < 0) {
                continue;
            }

            for (uint x = 0; x < width; x++) {
                int out_x = dst_x + (int)x;
                if (out_x < 0) {
                    continue;
                }

                uint src = src_rgba[(src_y + y) * src_pitch_pixels + src_x + x];
                uint dst_index = (uint)out_y * dst_pitch_pixels + (uint)out_x;
                dst_rgba[dst_index] = src_over(src, dst_rgba[dst_index]);
            }
        }
    }
}
