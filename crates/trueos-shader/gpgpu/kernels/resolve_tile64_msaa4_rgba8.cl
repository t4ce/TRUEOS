// TRUEOS Gen12.5/Alder Lake 4x-MSAA resolve.
//
// Contract:
// - Source is an R8G8B8A8_UNORM Tile64 4x-MSAA render surface.
// - Destination is a linear RGBA8 buffer.
// - src_pitch_bytes is the physical Tile64 row pitch.
// - Coordinates and dimensions are logical pixels.
// - One SIMD16 lane resolves one pixel by averaging its four samples.

static inline uint tile64_msaa4_rgba8_offset(
    uint x,
    uint y,
    uint sample,
    uint pitch_bytes)
{
    uint tile_x = x >> 6;
    uint tile_y = y >> 6;
    uint u = (x & 63u) << 2;
    uint v = y & 63u;

    // gfx12.5 Tile64 32-bpp 4x-MSAA (MSS/array) address swizzle.
    uint intra = (u & 0x0fu)
        | ((v & 0x03u) << 4)
        | (((u >> 4) & 0x01u) << 6)
        | ((sample & 0x01u) << 7)
        | (((sample >> 1) & 0x01u) << 8)
        | (((u >> 5) & 0x01u) << 9)
        | (((v >> 2) & 0x03u) << 10)
        | (((u >> 6) & 0x03u) << 12)
        | (((v >> 4) & 0x03u) << 14);

    return tile_y * pitch_bytes * 256u + tile_x * 65536u + intra;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void resolve_tile64_msaa4_rgba8(
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

    uint sx = src_x + x;
    uint sy = src_y + y;
    uint c0 = src_rgba[tile64_msaa4_rgba8_offset(sx, sy, 0u, src_pitch_bytes) >> 2];
    uint c1 = src_rgba[tile64_msaa4_rgba8_offset(sx, sy, 1u, src_pitch_bytes) >> 2];
    uint c2 = src_rgba[tile64_msaa4_rgba8_offset(sx, sy, 2u, src_pitch_bytes) >> 2];
    uint c3 = src_rgba[tile64_msaa4_rgba8_offset(sx, sy, 3u, src_pitch_bytes) >> 2];

    uint r = ((c0 & 0xffu) + (c1 & 0xffu) + (c2 & 0xffu) + (c3 & 0xffu) + 2u) >> 2;
    uint g = (((c0 >> 8) & 0xffu) + ((c1 >> 8) & 0xffu)
        + ((c2 >> 8) & 0xffu) + ((c3 >> 8) & 0xffu) + 2u) >> 2;
    uint b = (((c0 >> 16) & 0xffu) + ((c1 >> 16) & 0xffu)
        + ((c2 >> 16) & 0xffu) + ((c3 >> 16) & 0xffu) + 2u) >> 2;
    uint a = (((c0 >> 24) & 0xffu) + ((c1 >> 24) & 0xffu)
        + ((c2 >> 24) & 0xffu) + ((c3 >> 24) & 0xffu) + 2u) >> 2;
    uint resolved = r | (g << 8) | (b << 16) | (a << 24);

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_rgba[(dst_y + y) * dst_pitch_pixels + dst_x + x] = resolved;
}
