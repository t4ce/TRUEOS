// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Destination is a linear RGBA8 buffer.
// - Pitch is expressed in bytes.
// - Coordinates and dimensions are pixels.
// - The clear value is fixed for first bring-up: opaque white RGBA8.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void clear_rect_rgba8_white(
    __global uint *dst_rgba,
    uint dst_pitch_bytes,
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

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint dst_index = (dst_y + y) * dst_pitch_pixels + dst_x + x;

    dst_rgba[dst_index] = 0xFFFFFFFFu;
}

