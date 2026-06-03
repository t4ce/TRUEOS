// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Source and destination are linear RGBA8 buffers.
// - Pitches are expressed in bytes.
// - Coordinates and dimensions are pixels.
// - No scaling, filtering, color conversion, or blending.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void copy_rect_rgba8(
    __global const uint *src_rgba,
    __global uint *dst_rgba,
    uint src_pitch_bytes,
    uint dst_pitch_bytes,
    uint src_x,
    uint src_y,
    uint dst_x,
    uint dst_y,
    uint width,
    uint height)
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

    dst_rgba[dst_index] = src_rgba[src_index];
}
