// TRUEOS Gen12/Alder Lake UI3 sprite quad worklist kernel.
//
// Contract:
// - Source and destination are linear RGBA8 buffers packed as AABBGGRR in a u32.
// - Each descriptor mirrors the UI3 SpriteQuad CABI: four corners with x/y/u/v,
//   plus packed tint color and flags.
// - The rasterizer matches the CPU fallback's parallelogram path: c0/c1/c3
//   define the UV basis, all four corners define the clipped bounds, UV sampling
//   is nearest with clamp-to-edge, and output is straight source-over.
// - Each SIMD16 workgroup consumes one descriptor. Lanes cooperate on that
//   descriptor's pixels, which keeps large UI3 quads parallel without forcing
//   the whole source run through one serial descriptor loop.

#define SPRITE_QUAD_DESC_DWORDS 18u
#define SPRITE_QUAD_FLAG_SRC_OVER (1u << 0)

static inline uint div255(uint value)
{
    return (value + 127u) / 255u;
}

static inline uint mul_u8(uint a, uint b)
{
    return div255(a * b);
}

static inline uint modulate(uint src, uint color_rgba)
{
    uint sr = src & 0xFFu;
    uint sg = (src >> 8) & 0xFFu;
    uint sb = (src >> 16) & 0xFFu;
    uint sa = (src >> 24) & 0xFFu;
    uint tr = color_rgba & 0xFFu;
    uint tg = (color_rgba >> 8) & 0xFFu;
    uint tb = (color_rgba >> 16) & 0xFFu;
    uint ta = (color_rgba >> 24) & 0xFFu;

    return (mul_u8(sa, ta) << 24)
        | (mul_u8(sb, tb) << 16)
        | (mul_u8(sg, tg) << 8)
        | mul_u8(sr, tr);
}

static inline uint blend_channel(uint src, uint src_alpha, uint dst, uint inv_alpha)
{
    return div255(src * src_alpha + dst * inv_alpha);
}

static inline uint src_over(uint src, uint dst)
{
    uint sa = (src >> 24) & 0xFFu;
    if (sa == 0u) {
        return dst;
    }
    if (sa == 255u) {
        return (src & 0x00FFFFFFu) | 0xFF000000u;
    }

    uint inv = 255u - sa;
    uint sr = src & 0xFFu;
    uint sg = (src >> 8) & 0xFFu;
    uint sb = (src >> 16) & 0xFFu;
    uint dr = dst & 0xFFu;
    uint dg = (dst >> 8) & 0xFFu;
    uint db = (dst >> 16) & 0xFFu;
    uint da = (dst >> 24) & 0xFFu;

    uint out_r = blend_channel(sr, sa, dr, inv);
    uint out_g = blend_channel(sg, sa, dg, inv);
    uint out_b = blend_channel(sb, sa, db, inv);
    uint out_a = sa + div255(da * inv);

    return (out_a << 24) | (out_b << 16) | (out_g << 8) | out_r;
}

static inline int clamp_i32(int value, int lo, int hi)
{
    return min(max(value, lo), hi);
}

static inline uint sample_rgba(
    __global const uint *src_rgba,
    uint src_pitch_pixels,
    uint src_width,
    uint src_height,
    float u,
    float v)
{
    float cu = clamp(u, 0.0f, 1.0f);
    float cv = clamp(v, 0.0f, 1.0f);
    int max_x = (int)max(src_width, 1u) - 1;
    int max_y = (int)max(src_height, 1u) - 1;
    int sx = clamp_i32((int)(cu * (float)max(src_width, 1u)), 0, max_x);
    int sy = clamp_i32((int)(cv * (float)max(src_height, 1u)), 0, max_y);
    return src_rgba[(uint)sy * src_pitch_pixels + (uint)sx];
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void sprite_quad_worklist_rgba8(
    __global const uint *src_rgba,
    __global uint *dst_rgba,
    __global const uint *descs,
    uint src_pitch_bytes,
    uint dst_pitch_bytes,
    uint src_width,
    uint src_height,
    uint dst_width,
    uint dst_height,
    uint desc_base,
    uint desc_count)
{
    uint lane = get_local_id(0);
    uint local_desc_id = get_group_id(0);
    if (lane >= 16u) {
        return;
    }
    if (local_desc_id >= desc_count) {
        return;
    }

    uint src_pitch_pixels = src_pitch_bytes >> 2;
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    int max_dst_x = (int)max(dst_width, 1u) - 1;
    int max_dst_y = (int)max(dst_height, 1u) - 1;

    uint desc_index = (desc_base + local_desc_id) * SPRITE_QUAD_DESC_DWORDS;
    float c0x = as_float(descs[desc_index + 0u]);
    float c0y = as_float(descs[desc_index + 1u]);
    float c0u = as_float(descs[desc_index + 2u]);
    float c0v = as_float(descs[desc_index + 3u]);
    float c1x = as_float(descs[desc_index + 4u]);
    float c1y = as_float(descs[desc_index + 5u]);
    float c1u = as_float(descs[desc_index + 6u]);
    float c1v = as_float(descs[desc_index + 7u]);
    float c2x = as_float(descs[desc_index + 8u]);
    float c2y = as_float(descs[desc_index + 9u]);
    float c3x = as_float(descs[desc_index + 12u]);
    float c3y = as_float(descs[desc_index + 13u]);
    float c3u = as_float(descs[desc_index + 14u]);
    float c3v = as_float(descs[desc_index + 15u]);
    uint color_rgba = descs[desc_index + 16u];
    uint flags = descs[desc_index + 17u];

    float exx = c1x - c0x;
    float exy = c1y - c0y;
    float eyx = c3x - c0x;
    float eyy = c3y - c0y;
    float det = exx * eyy - exy * eyx;
    if (fabs(det) < 0.00001f) {
        return;
    }

    int min_x = max((int)floor(min(min(c0x, c1x), min(c2x, c3x))), 0);
    int min_y = max((int)floor(min(min(c0y, c1y), min(c2y, c3y))), 0);
    int max_x = min((int)ceil(max(max(c0x, c1x), max(c2x, c3x))), max_dst_x);
    int max_y = min((int)ceil(max(max(c0y, c1y), max(c2y, c3y))), max_dst_y);
    if (min_x > max_x || min_y > max_y) {
        return;
    }

    uint bbox_w = (uint)(max_x - min_x + 1);
    uint bbox_h = (uint)(max_y - min_y + 1);
    uint pixel_count = bbox_w * bbox_h;
    for (uint pixel_id = lane; pixel_id < pixel_count; pixel_id += 16u) {
        int x = min_x + (int)(pixel_id % bbox_w);
        int y = min_y + (int)(pixel_id / bbox_w);
        float dx = (float)x + 0.5f - c0x;
        float dy = (float)y + 0.5f - c0y;
        float s = (dx * eyy - dy * eyx) / det;
        float t = (exx * dy - exy * dx) / det;
        if (s >= -0.0001f && s <= 1.0001f && t >= -0.0001f && t <= 1.0001f) {
            float u = c0u + (c1u - c0u) * s + (c3u - c0u) * t;
            float v = c0v + (c1v - c0v) * s + (c3v - c0v) * t;
            uint src = modulate(sample_rgba(
                src_rgba,
                src_pitch_pixels,
                src_width,
                src_height,
                u,
                v),
                color_rgba);
            uint dst_index = (uint)y * dst_pitch_pixels + (uint)x;
            if ((flags & SPRITE_QUAD_FLAG_SRC_OVER) != 0u) {
                dst_rgba[dst_index] = src_over(src, dst_rgba[dst_index]);
            } else {
                dst_rgba[dst_index] = src;
            }
        }
    }
}
