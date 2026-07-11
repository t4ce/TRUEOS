// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Source is a linear TRUEOS RGBA8 scene buffer, packed as AABBGGRR in a u32.
// - Destination is the Intel primary linear/native XRGB buffer, packed as 00RRGGBB.
// - Pitches are expressed in bytes.
// - Coordinates and dimensions are pixels.
// - flip_y != 0 maps row y to source row height - 1 - y inside the source rect.
// - No scaling, filtering, or blending.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void present_rgba8_to_primary_xrgb_rect(
    __global const uint *src_rgba,
    __global uint *dst_xrgb,
    uint src_pitch_bytes,
    uint dst_pitch_bytes,
    uint src_x,
    uint src_y,
    uint dst_x,
    uint dst_y,
    uint width,
    uint height,
    uint flip_y)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);

    if (x >= width || y >= height) {
        return;
    }

    uint src_pitch_pixels = src_pitch_bytes >> 2;
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint read_y = flip_y ? (height - 1u - y) : y;
    uint src_index = (src_y + read_y) * src_pitch_pixels + src_x + x;
    uint dst_index = (dst_y + y) * dst_pitch_pixels + dst_x + x;

    uint src = src_rgba[src_index];
    uint r = src & 0xFFu;
    uint g = (src >> 8) & 0xFFu;
    uint b = (src >> 16) & 0xFFu;

    dst_xrgb[dst_index] = (r << 16) | (g << 8) | b;
}
