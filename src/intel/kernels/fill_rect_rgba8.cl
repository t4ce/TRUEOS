// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Destination is a linear RGBA8 buffer.
// - Pitch is expressed in bytes.
// - Coordinates and dimensions are pixels.
// - Fill color is packed as AABBGGRR in a u32, matching the current RGBA8 surface convention.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void fill_rect_rgba8(
    __global uint *dst_rgba,
    uint dst_pitch_bytes,
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

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint dst_index = (dst_y + y) * dst_pitch_pixels + dst_x + x;

    dst_rgba[dst_index] = color_rgba;
}

