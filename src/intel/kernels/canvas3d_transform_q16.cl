// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Input and output vertices are fixed-point Q16 vec3 values stored as int4
//   { x, y, z, pad }.
// - The walker transforms the source subset
//   [src_first_vertex, src_first_vertex + vertex_count) into the destination
//   subset [dst_first_vertex, dst_first_vertex + vertex_count).
// - scale_q16 is int4 { sx, sy, sz, ignored } in Q16 units.
// - quat_q16 is int4 { x, y, z, w } in Q16 units.
// - delta_q16 is int4 { dx, dy, dz, ignored } in Q16 units.
// - Transform order is scale, quaternion rotate, translate.
// - The pad lane is preserved from the source vertex.

static inline int q16_mul(int a, int b)
{
    return (int)(((long)a * (long)b) >> 16);
}

static inline int q16_mul2(int a, int b)
{
    return (int)(((long)a * (long)b) >> 15);
}

static inline int q16_div(int numerator_q16, int denominator_q16)
{
    return (int)(((long)numerator_q16 * 65536L) / (long)denominator_q16);
}

static inline int q16_div2(int numerator_q16, int denominator_q16)
{
    return (int)(((long)numerator_q16 * 131072L) / (long)denominator_q16);
}

static inline int4 q16_cross(int4 a, int4 b)
{
    return (int4)(
        q16_mul(a.y, b.z) - q16_mul(a.z, b.y),
        q16_mul(a.z, b.x) - q16_mul(a.x, b.z),
        q16_mul(a.x, b.y) - q16_mul(a.y, b.x),
        0);
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void canvas3d_transform_q16(
    __global const int4 *src_vertices_q16,
    __global int4 *dst_vertices_q16,
    uint src_first_vertex,
    uint dst_first_vertex,
    uint vertex_count,
    int4 scale_q16,
    int4 quat_q16,
    int4 delta_q16)
{
    uint lane = get_global_id(0);

    if (lane >= 16u) {
        return;
    }

    int norm_q16 =
        q16_mul(quat_q16.x, quat_q16.x) +
        q16_mul(quat_q16.y, quat_q16.y) +
        q16_mul(quat_q16.z, quat_q16.z) +
        q16_mul(quat_q16.w, quat_q16.w);

    for (uint offset = lane; offset < vertex_count; offset += 16u) {
        int4 v = src_vertices_q16[src_first_vertex + offset];
        int4 scaled = (int4)(
            q16_mul(v.x, scale_q16.x),
            q16_mul(v.y, scale_q16.y),
            q16_mul(v.z, scale_q16.z),
            v.w);

        int4 rotated = scaled;
        if (norm_q16 != 0) {
            int4 uv = q16_cross(quat_q16, scaled);
            int4 uuv = q16_cross(quat_q16, uv);
            rotated = (int4)(
                scaled.x + q16_div(q16_mul2(quat_q16.w, uv.x), norm_q16) + q16_div2(uuv.x, norm_q16),
                scaled.y + q16_div(q16_mul2(quat_q16.w, uv.y), norm_q16) + q16_div2(uuv.y, norm_q16),
                scaled.z + q16_div(q16_mul2(quat_q16.w, uv.z), norm_q16) + q16_div2(uuv.z, norm_q16),
                scaled.w);
        }

        dst_vertices_q16[dst_first_vertex + offset] = (int4)(
            rotated.x + delta_q16.x,
            rotated.y + delta_q16.y,
            rotated.z + delta_q16.z,
            v.w);
    }
}
