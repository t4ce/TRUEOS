// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Input vertices are fixed-point Q16 vec3 values stored as int4 { x, y, z, pad }.
// - Output points are uint4 { packed_xy, rgba, z_q16, source_index }.
// - The walker projects the source subset
//   [src_first_vertex, src_first_vertex + vertex_count) into the output subset
//   [out_first_point, out_first_point + vertex_count).
// - Projection targets a dynamic canvas:
//     center_x = canvas_width / 2
//     center_y = canvas_height / 2
//     focal = min(canvas_width, canvas_height) / 2
//     screen_x = center_x + (x * focal) / z
//     screen_y = center_y - (y * focal) / z
// - packed_xy is 0x80000000 | (screen_y << 16) | screen_x when visible.
// - Invisible/out-of-canvas/depth-failed vertices write zero packed_xy and zero rgba.

typedef struct Canvas3dProjectedPoint {
    uint packed_xy;
    uint rgba;
    uint z_q16;
    uint source_index;
} Canvas3dProjectedPoint;

static inline uint canvas3d_color(uint index, uint z_q16)
{
    uint shade = 96u + ((index * 29u) & 0x7Fu);
    uint depth = (z_q16 >> 10) & 0x7Fu;
    uint r = shade;
    uint g = 255u - depth;
    uint b = 96u + depth;
    return 0xFF000000u | (b << 16) | (g << 8) | r;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void canvas3d_project_rgba8(
    __global const int4 *vertices_q16,
    __global Canvas3dProjectedPoint *out_points,
    uint src_first_vertex,
    uint out_first_point,
    uint vertex_count,
    uint canvas_width,
    uint canvas_height)
{
    uint lane = get_global_id(0);
    uint focal = min(canvas_width, canvas_height) >> 1;
    int center_x = (int)(canvas_width >> 1);
    int center_y = (int)(canvas_height >> 1);

    for (uint offset = lane; offset < vertex_count; offset += 16u) {
        uint src_index = src_first_vertex + offset;
        uint out_index = out_first_point + offset;
        int4 v = vertices_q16[src_index];
        Canvas3dProjectedPoint out;
        out.packed_xy = 0u;
        out.rgba = 0u;
        out.z_q16 = (uint)v.z;
        out.source_index = src_index;

        if (v.z > 0 && canvas_width > 0u && canvas_height > 0u && focal > 0u) {
            long sx_delta = ((long)v.x * (long)focal) / (long)v.z;
            long sy_delta = ((long)v.y * (long)focal) / (long)v.z;
            int sx = center_x + (int)sx_delta;
            int sy = center_y - (int)sy_delta;

            if (sx >= 0 && sx < (int)canvas_width && sy >= 0 && sy < (int)canvas_height) {
                out.packed_xy = 0x80000000u | (((uint)sy & 0xFFFFu) << 16) | ((uint)sx & 0xFFFFu);
                out.rgba = canvas3d_color(src_index, (uint)v.z);
            }
        }

        out_points[out_index] = out;
    }
}
