// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Input vertices are fixed-point Q16 vec3 values stored as int4 { x, y, z, pad }.
// - Output points are uint4 { packed_xy, rgba, z_q16, source_index }.
// - Projection is fixed to a 512x512 canvas:
//     screen_x = 256 + (x * 256) / z
//     screen_y = 256 - (y * 256) / z
// - packed_xy is 0x80000000 | (screen_y << 16) | screen_x when visible.
// - Invisible/out-of-canvas/depth-failed vertices write zero packed_xy and zero rgba.

typedef struct Canvas512ProjectedPoint {
    uint packed_xy;
    uint rgba;
    uint z_q16;
    uint source_index;
} Canvas512ProjectedPoint;

static inline uint canvas512_color(uint index, uint z_q16)
{
    uint shade = 96u + ((index * 29u) & 0x7Fu);
    uint depth = (z_q16 >> 10) & 0x7Fu;
    uint r = shade;
    uint g = 255u - depth;
    uint b = 96u + depth;
    return 0xFF000000u | (b << 16) | (g << 8) | r;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void canvas512_3d_project_rgba8(
    __global const int4 *vertices_q16,
    __global Canvas512ProjectedPoint *out_points,
    uint vertex_count)
{
    uint lane = get_global_id(0);

    for (uint index = lane; index < vertex_count; index += 16u) {
        int4 v = vertices_q16[index];
        Canvas512ProjectedPoint out;
        out.packed_xy = 0u;
        out.rgba = 0u;
        out.z_q16 = (uint)v.z;
        out.source_index = index;

        if (v.z > 0) {
            long sx_delta = ((long)v.x * 256L) / (long)v.z;
            long sy_delta = ((long)v.y * 256L) / (long)v.z;
            int sx = 256 + (int)sx_delta;
            int sy = 256 - (int)sy_delta;

            if (sx >= 0 && sx < 512 && sy >= 0 && sy < 512) {
                out.packed_xy = 0x80000000u | (((uint)sy & 0xFFFFu) << 16) | ((uint)sx & 0xFFFFu);
                out.rgba = canvas512_color(index, (uint)v.z);
            }
        }

        out_points[index] = out;
    }
}
