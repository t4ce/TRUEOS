// TRUEOS Gen12/Alder Lake GPGPU evo kernel.
//
// Contract:
// - Destination is a linear RGBA8 buffer packed as AABBGGRR in a u32.
// - Each descriptor fills one rectangle with a horizontal or vertical gradient.
// - One SIMD16 walker consumes a descriptor slice:
//   lane N draws descriptors desc_base + N, desc_base + N+16, ...

#define GRADIENT_RECT_FLAG_VERTICAL 1u

static inline int unpack_i16(uint value)
{
    return (int)((short)(value & 0xFFFFu));
}

static inline uint lerp_channel(uint c0, uint c1, uint pos, uint denom)
{
    if (denom == 0u) {
        return c0;
    }
    return (c0 * (denom - pos) + c1 * pos + (denom >> 1)) / denom;
}

static inline uint lerp_rgba(uint c0, uint c1, uint pos, uint denom)
{
    uint r = lerp_channel(c0 & 0xFFu, c1 & 0xFFu, pos, denom);
    uint g = lerp_channel((c0 >> 8) & 0xFFu, (c1 >> 8) & 0xFFu, pos, denom);
    uint b = lerp_channel((c0 >> 16) & 0xFFu, (c1 >> 16) & 0xFFu, pos, denom);
    uint a = lerp_channel((c0 >> 24) & 0xFFu, (c1 >> 24) & 0xFFu, pos, denom);
    return (a << 24) | (b << 16) | (g << 8) | r;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void gradient_rect_worklist_rgba8(
    __global uint *dst_rgba,
    __global const uint *descs,
    uint dst_pitch_bytes,
    uint desc_base,
    uint desc_count)
{
    uint worker = get_global_id(0);

    if (worker != 0u) {
        return;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;

    for (uint local_desc_id = 0u; local_desc_id < desc_count; local_desc_id++) {
        uint desc_index = (desc_base + local_desc_id) * 5u;
        uint dst_xy = descs[desc_index + 0u];
        uint size = descs[desc_index + 1u];
        uint color0_rgba = descs[desc_index + 2u];
        uint color1_rgba = descs[desc_index + 3u];
        uint flags = descs[desc_index + 4u];
        int dst_x = unpack_i16(dst_xy);
        int dst_y = unpack_i16(dst_xy >> 16);
        uint width = size & 0xFFFFu;
        uint height = size >> 16;
        uint vertical = flags & GRADIENT_RECT_FLAG_VERTICAL;
        uint denom = vertical != 0u ? (height > 0u ? height - 1u : 0u)
                                   : (width > 0u ? width - 1u : 0u);

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

                uint pos = vertical != 0u ? y : x;
                dst_rgba[(uint)out_y * dst_pitch_pixels + (uint)out_x] =
                    lerp_rgba(color0_rgba, color1_rgba, pos, denom);
            }
        }
    }
}
