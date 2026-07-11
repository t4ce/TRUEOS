// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Writes projected RGBA sample points from one affine plane.
// - The plane is P(u,v) = origin + u * axis_u + v * axis_v, with all values
//   in Q16.  The sampled local domain is u,v in [-1, +1].
// - Up to four local half-space constraints are int4 { a, b, c, ignored }:
//     a*u + b*v + c >= 0
//   with a,b,c in Q16.
// - Output points are uint4 { packed_xy, rgba, z_q16, source_index }.

typedef struct Canvas3dProjectedPoint {
    uint packed_xy;
    uint rgba;
    uint z_q16;
    uint source_index;
} Canvas3dProjectedPoint;

static inline int q16_mul(int a, int b)
{
    return (int)(((long)a * (long)b) >> 16);
}

static inline int q16_lerp_unit(uint index, uint count)
{
    if (count <= 1u) {
        return 0;
    }
    return -65536 + (int)((131072ul * (ulong)index) / (ulong)(count - 1u));
}

static inline int constraint_ok(int4 constraint, int u_q16, int v_q16)
{
    long value = (((long)constraint.x * (long)u_q16) >> 16)
        + (((long)constraint.y * (long)v_q16) >> 16)
        + (long)constraint.z;
    return value >= 0;
}

static inline uint dither_color(uint color_rgba, uint u_index, uint v_index)
{
    uint d = ((u_index ^ (v_index * 3u)) & 3u) * 10u;
    uint r = min(255u, (color_rgba & 0xFFu) + d);
    uint g = min(255u, ((color_rgba >> 8) & 0xFFu) + d);
    uint b = min(255u, ((color_rgba >> 16) & 0xFFu) + d);
    uint a = (color_rgba >> 24) & 0xFFu;
    return (a << 24) | (b << 16) | (g << 8) | r;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void canvas3d_plane_sample_rgba8(
    __global const int4 *unused_q16,
    __global Canvas3dProjectedPoint *out_points,
    uint out_first_point,
    uint sample_count,
    uint canvas_width,
    uint canvas_height,
    int4 origin_q16,
    int4 axis_u_q16,
    int4 axis_v_q16,
    int4 constraint0_q16,
    int4 constraint1_q16,
    int4 constraint2_q16,
    int4 constraint3_q16,
    uint constraint_count,
    uint u_steps,
    uint v_steps,
    uint color_rgba)
{
    (void)unused_q16;

    uint lane = get_global_id(0);
    uint focal = min(canvas_width, canvas_height) >> 1;
    int center_x = (int)(canvas_width >> 1);
    int center_y = (int)(canvas_height >> 1);

    if (u_steps == 0u || v_steps == 0u) {
        return;
    }

    for (uint offset = lane; offset < sample_count; offset += 16u) {
        uint u_index = offset % u_steps;
        uint v_index = offset / u_steps;
        uint out_index = out_first_point + offset;

        Canvas3dProjectedPoint out;
        out.packed_xy = 0u;
        out.rgba = 0u;
        out.z_q16 = 0u;
        out.source_index = offset;

        if (v_index < v_steps) {
            int u_q16 = q16_lerp_unit(u_index, u_steps);
            int v_q16 = q16_lerp_unit(v_index, v_steps);
            int keep = 1;
            if (constraint_count > 0u) keep &= constraint_ok(constraint0_q16, u_q16, v_q16);
            if (constraint_count > 1u) keep &= constraint_ok(constraint1_q16, u_q16, v_q16);
            if (constraint_count > 2u) keep &= constraint_ok(constraint2_q16, u_q16, v_q16);
            if (constraint_count > 3u) keep &= constraint_ok(constraint3_q16, u_q16, v_q16);

            int x = origin_q16.x + q16_mul(axis_u_q16.x, u_q16) + q16_mul(axis_v_q16.x, v_q16);
            int y = origin_q16.y + q16_mul(axis_u_q16.y, u_q16) + q16_mul(axis_v_q16.y, v_q16);
            int z = origin_q16.z + q16_mul(axis_u_q16.z, u_q16) + q16_mul(axis_v_q16.z, v_q16);
            out.z_q16 = (uint)z;

            if (keep && z > 0 && canvas_width > 0u && canvas_height > 0u && focal > 0u) {
                long sx_delta = ((long)x * (long)focal) / (long)z;
                long sy_delta = ((long)y * (long)focal) / (long)z;
                int sx = center_x + (int)sx_delta;
                int sy = center_y - (int)sy_delta;

                if (sx >= 0 && sx < (int)canvas_width && sy >= 0 && sy < (int)canvas_height) {
                    out.packed_xy = 0x80000000u | (((uint)sy & 0xFFFFu) << 16) | ((uint)sx & 0xFFFFu);
                    out.rgba = dither_color(color_rgba, u_index, v_index);
                }
            }
        }

        out_points[out_index] = out;
    }
}
