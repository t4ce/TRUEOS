// TRUEOS UI4 native-video Frame producer for Alder Lake S.
//
// One SIMD16 dispatch converts a decoder-owned media Y-tiled NV12 picture directly
// into one exact UI4-owned, linear premultiplied RGBA8 Frame buffer. Pixels
// outside the selected picture are opaque black. The destination can therefore
// be published and imported by the same release/SURFLIVE lifecycle as the
// GPGPU preview and resident Draw3D consumers.

// The proven linked-plane path de-tiled this exact VDBOX allocation as legacy
// 128x32 Y tiles before scanout.  MFX_SURFACE_STATE's tilemode value must not
// be interpreted as the display-memory Tile64 swizzle here: doing so turns the
// untouched rollback clear into large green blocks separated by decoded
// horizontal bands.
inline uint ui4_media_ytile_8bpp_offset(uint byte_x, uint row_y, uint tiles_per_row)
{
    uint tile_col = byte_x >> 7;
    uint tile_row = row_y >> 5;
    uint in_x = byte_x & 127u;
    uint in_y = row_y & 31u;
    uint within_tile = (in_x >> 4) * 512u + in_y * 16u + (in_x & 15u);
    return (tile_row * tiles_per_row + tile_col) * 4096u + within_tile;
}

inline uint ui4_clamped_bt601_channel(int value)
{
    return (uint)clamp((value + 128) >> 8, 0, 255);
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void ui4_nv12_tile64_to_rgba8_frame(
    __global const uchar *src_nv12,
    __global const uint *base_xrgb,
    __global uint *dst_rgba,
    uint src_pitch_bytes,
    uint src_uv_offset,
    uint base_pitch_bytes,
    uint dst_pitch_bytes,
    uint output_width,
    uint output_height,
    uint content_dst_x,
    uint content_dst_y,
    uint content_width,
    uint content_height,
    uint source_x,
    uint source_y)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);
    if (x >= output_width || y >= output_height) {
        return;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint dst_index = y * dst_pitch_pixels + x;
    uint inside_x = x - content_dst_x;
    uint inside_y = y - content_dst_y;
    if (inside_x >= content_width || inside_y >= content_height) {
        // Preserve the proven three-binding SIMD16 ABI without importing a
        // desktop surface. The Frame producer binds this exact destination as
        // its own base, so every lane reads and rewrites only the same pixel.
        // The alpha repair also makes a freshly zeroed lease opaque black.
        uint base_pitch_pixels = base_pitch_bytes >> 2;
        dst_rgba[dst_index] = base_xrgb[y * base_pitch_pixels + x] | 0xFF000000u;
        return;
    }

    uint sample_x = source_x + inside_x;
    uint sample_y = source_y + inside_y;
    uint tiles_per_row = src_pitch_bytes >> 7;
    uint chroma_row = src_uv_offset / src_pitch_bytes;
    uint y_offset = ui4_media_ytile_8bpp_offset(sample_x, sample_y, tiles_per_row);
    uint uv_x = sample_x & ~1u;
    uint uv_offset = ui4_media_ytile_8bpp_offset(
        uv_x,
        chroma_row + (sample_y >> 1),
        tiles_per_row);

    int c = max((int)src_nv12[y_offset] - 16, 0);
    int d = (int)src_nv12[uv_offset] - 128;
    int e = (int)src_nv12[uv_offset + 1u] - 128;
    uint r = ui4_clamped_bt601_channel(298 * c + 409 * e);
    uint g = ui4_clamped_bt601_channel(298 * c - 100 * d - 208 * e);
    uint b = ui4_clamped_bt601_channel(298 * c + 516 * d);
    dst_rgba[dst_index] = 0xFF000000u | (b << 16) | (g << 8) | r;
}
