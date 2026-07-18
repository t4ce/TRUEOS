// TRUEOS Gen12/Alder Lake GPGPU evo kernel.
//
// Contract:
// - Destination is a linear RGBA8 buffer packed as AABBGGRR in a u32.
// - Each descriptor fills one rectangle.
// - One workgroup consumes one descriptor; its SIMD16 lanes split every row.
// - One walker launches one workgroup for each descriptor in its slice.

static inline int unpack_i16(uint value)
{
    return (int)((short)(value & 0xFFFFu));
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void fill_rect_worklist_rgba8(
    __global uint *dst_rgba,
    __global const uint *descs,
    uint dst_pitch_bytes,
    uint desc_base,
    uint desc_count)
{
    uint lane = get_local_id(0);
    uint local_desc_id = get_group_id(0);

    if (lane >= 16u || local_desc_id >= desc_count) {
        return;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint desc_index = (desc_base + local_desc_id) * 3u;
    uint dst_xy = descs[desc_index + 0u];
    uint size = descs[desc_index + 1u];
    uint color_rgba = descs[desc_index + 2u];
    int dst_x = unpack_i16(dst_xy);
    int dst_y = unpack_i16(dst_xy >> 16);
    uint width = size & 0xFFFFu;
    uint height = size >> 16;

    for (uint y = 0; y < height; y++) {
        int out_y = dst_y + (int)y;
        if (out_y < 0) {
            continue;
        }

        for (uint x = lane; x < width; x += 16u) {
            int out_x = dst_x + (int)x;
            if (out_x < 0) {
                continue;
            }

            dst_rgba[(uint)out_y * dst_pitch_pixels + (uint)out_x] = color_rgba;
        }
    }
}
