// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Destination is a linear RGBA8 buffer.
// - Pitch is expressed in bytes.
// - Coordinates and dimensions are pixels.
// - Circle is clipped by the provided destination rect.
// - Center is expressed relative to the rect origin in pixels.
// - Fill color is packed as AABBGGRR in a u32.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void fill_circle_rgba8(
    __global uint *dst_rgba,
    uint dst_pitch_bytes,
    uint dst_x,
    uint dst_y,
    uint rect_width,
    uint rect_height,
    int center_x,
    int center_y,
    uint radius,
    uint color_rgba)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);

    if (x >= rect_width || y >= rect_height) {
        return;
    }

    int dx = (int)x - center_x;
    int dy = (int)y - center_y;
    uint distance2 = (uint)(dx * dx + dy * dy);
    uint radius2 = radius * radius;

    if (distance2 > radius2) {
        return;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint dst_index = (dst_y + y) * dst_pitch_pixels + dst_x + x;

    dst_rgba[dst_index] = color_rgba;
}

