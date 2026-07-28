// TRUEOS UI4 RDP encoder input conversion for Alder Lake S.
//
// One SIMD16 work-item produces one 2x2 NV12 macro-pixel from the logical
// premultiplied RGBA8 scanout.  The fixed 4:3 nearest-neighbour mapping matches
// the former CPU implementation exactly: 2560x1440 RGBA becomes a centered
// 1920x1080 picture in a 1920x1088 linear NV12 encoder surface.

inline uchar ui4_rgb_to_luma(uchar red, uchar green, uchar blue)
{
    int value = ((66 * (int)red + 129 * (int)green + 25 * (int)blue + 128) >> 8) + 16;
    return (uchar)clamp(value, 16, 235);
}

inline uint ui4_downscaled_source_coordinate(uint destination)
{
    return (destination * 4u + 2u) / 3u;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void ui4_rgba8_to_nv12_linear(
    __global const uchar *src_rgba,
    __global uchar *dst_nv12,
    uint src_pitch_bytes,
    uint src_width,
    uint src_height,
    uint dst_pitch_bytes,
    uint dst_width,
    uint dst_height,
    uint active_top,
    uint active_height)
{
    uint dst_x = get_global_id(0) * 2u;
    uint dst_y = get_global_id(1) * 2u;
    if (dst_x >= dst_width || dst_y >= dst_height) {
        return;
    }

    uint uv_offset = dst_pitch_bytes * dst_height;
    uint y_row_0 = dst_y * dst_pitch_bytes;
    uint y_row_1 = (dst_y + 1u) * dst_pitch_bytes;
    uint uv_row = uv_offset + (dst_y >> 1) * dst_pitch_bytes;
    if (dst_y < active_top || dst_y >= active_top + active_height) {
        dst_nv12[y_row_0 + dst_x] = (uchar)16;
        dst_nv12[y_row_0 + dst_x + 1u] = (uchar)16;
        dst_nv12[y_row_1 + dst_x] = (uchar)16;
        dst_nv12[y_row_1 + dst_x + 1u] = (uchar)16;
        dst_nv12[uv_row + dst_x] = (uchar)128;
        dst_nv12[uv_row + dst_x + 1u] = (uchar)128;
        return;
    }

    uint active_y = dst_y - active_top;
    uint src_x_0 = ui4_downscaled_source_coordinate(dst_x);
    uint src_x_1 = ui4_downscaled_source_coordinate(dst_x + 1u);
    uint src_y_0 = ui4_downscaled_source_coordinate(active_y);
    uint src_y_1 = ui4_downscaled_source_coordinate(active_y + 1u);
    if (src_x_1 >= src_width || src_y_1 >= src_height) {
        return;
    }

    uint src_00 = src_y_0 * src_pitch_bytes + src_x_0 * 4u;
    uint src_01 = src_y_0 * src_pitch_bytes + src_x_1 * 4u;
    uint src_10 = src_y_1 * src_pitch_bytes + src_x_0 * 4u;
    uint src_11 = src_y_1 * src_pitch_bytes + src_x_1 * 4u;
    uchar r00 = src_rgba[src_00];
    uchar g00 = src_rgba[src_00 + 1u];
    uchar b00 = src_rgba[src_00 + 2u];
    uchar r01 = src_rgba[src_01];
    uchar g01 = src_rgba[src_01 + 1u];
    uchar b01 = src_rgba[src_01 + 2u];
    uchar r10 = src_rgba[src_10];
    uchar g10 = src_rgba[src_10 + 1u];
    uchar b10 = src_rgba[src_10 + 2u];
    uchar r11 = src_rgba[src_11];
    uchar g11 = src_rgba[src_11 + 1u];
    uchar b11 = src_rgba[src_11 + 2u];

    dst_nv12[y_row_0 + dst_x] = ui4_rgb_to_luma(r00, g00, b00);
    dst_nv12[y_row_0 + dst_x + 1u] = ui4_rgb_to_luma(r01, g01, b01);
    dst_nv12[y_row_1 + dst_x] = ui4_rgb_to_luma(r10, g10, b10);
    dst_nv12[y_row_1 + dst_x + 1u] = ui4_rgb_to_luma(r11, g11, b11);

    uint red = ((uint)r00 + (uint)r01 + (uint)r10 + (uint)r11) >> 2;
    uint green = ((uint)g00 + (uint)g01 + (uint)g10 + (uint)g11) >> 2;
    uint blue = ((uint)b00 + (uint)b01 + (uint)b10 + (uint)b11) >> 2;
    int u = ((-38 * (int)red - 74 * (int)green + 112 * (int)blue + 128) >> 8) + 128;
    int v = ((112 * (int)red - 94 * (int)green - 18 * (int)blue + 128) >> 8) + 128;
    dst_nv12[uv_row + dst_x] = (uchar)clamp(u, 16, 240);
    dst_nv12[uv_row + dst_x + 1u] = (uchar)clamp(v, 16, 240);
}
