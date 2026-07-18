// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Atlas and destination are linear RGBA8 buffers packed as AABBGGRR in a u32.
// - Each descriptor draws one fixed 64x64 sprite from atlas to destination.
// - One SIMD16 walker consumes a descriptor slice:
//   lane N draws descriptors desc_base + N, desc_base + N+16, ...
// - flags bit 0: source-over alpha blend when set, raw copy when clear.
// - flags bit 1: multiply source RGB by color RGB when set.

typedef struct Sprite64Desc {
    uint atlas_xy;
    uint dst_xy;
    uint flags;
    uint color_rgba;
} Sprite64Desc;

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

static inline uint tint_src(uint src, uint color)
{
    uint sr = src & 0xFFu;
    uint sg = (src >> 8) & 0xFFu;
    uint sb = (src >> 16) & 0xFFu;
    uint sa = (src >> 24) & 0xFFu;
    uint cr = color & 0xFFu;
    uint cg = (color >> 8) & 0xFFu;
    uint cb = (color >> 16) & 0xFFu;

    sr = div255(sr * cr);
    sg = div255(sg * cg);
    sb = div255(sb * cb);

    return (sa << 24) | (sb << 16) | (sg << 8) | sr;
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
__kernel void sprite64_worklist_rgba8(
    __global const uint *atlas_rgba,
    __global uint *dst_rgba,
    __global const Sprite64Desc *descs,
    uint atlas_pitch_bytes,
    uint dst_pitch_bytes,
    uint desc_base,
    uint desc_count)
{
    uint lane = get_global_id(0);

    if (lane >= 16u) {
        return;
    }

    uint atlas_pitch_pixels = atlas_pitch_bytes >> 2;
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;

    for (uint local_desc_id = lane; local_desc_id < desc_count; local_desc_id += 16u) {
        uint desc_id = desc_base + local_desc_id;
        Sprite64Desc desc = descs[desc_id];
        uint atlas_x = desc.atlas_xy & 0xFFFFu;
        uint atlas_y = desc.atlas_xy >> 16;
        int dst_x = unpack_i16(desc.dst_xy);
        int dst_y = unpack_i16(desc.dst_xy >> 16);

        for (uint y = 0; y < 64u; y++) {
            int out_y = dst_y + (int)y;
            if (out_y < 0) {
                continue;
            }

            for (uint x = 0; x < 64u; x++) {
                int out_x = dst_x + (int)x;
                if (out_x < 0) {
                    continue;
                }

                uint src_index = (atlas_y + y) * atlas_pitch_pixels + atlas_x + x;
                uint dst_index = (uint)out_y * dst_pitch_pixels + (uint)out_x;
                uint src = atlas_rgba[src_index];

                if ((desc.flags & 2u) != 0u) {
                    src = tint_src(src, desc.color_rgba);
                }

                if ((desc.flags & 1u) != 0u) {
                    dst_rgba[dst_index] = src_over(src, dst_rgba[dst_index]);
                } else {
                    dst_rgba[dst_index] = src;
                }
            }
        }
    }
}
