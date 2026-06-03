// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Source and destination are linear RGBA8 buffers.
// - Pitches are expressed in bytes.
// - Rectangles are expressed in pixels.
// - Nearest-neighbor sampling from source rect into destination rect.
// - No blending.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void blit_rgba8_nearest(
    __global const uint *src_rgba,
    __global uint *dst_rgba,
    uint src_pitch_bytes,
    uint dst_pitch_bytes,
    uint src_x,
    uint src_y,
    uint src_width,
    uint src_height,
    uint dst_x,
    uint dst_y,
    uint dst_width,
    uint dst_height)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);

    if (x >= dst_width || y >= dst_height || src_width == 0 || src_height == 0) {
        return;
    }

    uint sample_x = src_x + (x * src_width) / dst_width;
    uint sample_y = src_y + (y * src_height) / dst_height;
    uint src_pitch_pixels = src_pitch_bytes >> 2;
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint src_index = sample_y * src_pitch_pixels + sample_x;
    uint dst_index = (dst_y + y) * dst_pitch_pixels + dst_x + x;

    dst_rgba[dst_index] = src_rgba[src_index];
}

