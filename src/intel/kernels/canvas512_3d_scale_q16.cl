// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Input and output vertices are fixed-point Q16 vec3 values stored as int4
//   { x, y, z, pad }.
// - The walker transforms the source subset
//   [src_first_vertex, src_first_vertex + vertex_count) into the destination
//   subset [dst_first_vertex, dst_first_vertex + vertex_count).
// - scale_q16 is int4 { sx, sy, sz, ignored } in Q16 units.
// - The pad lane is preserved from the source vertex.

static inline int q16_mul(int a, int b)
{
    return (int)(((long)a * (long)b) >> 16);
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void canvas512_3d_scale_q16(
    __global const int4 *src_vertices_q16,
    __global int4 *dst_vertices_q16,
    uint src_first_vertex,
    uint dst_first_vertex,
    uint vertex_count,
    int4 scale_q16)
{
    uint lane = get_global_id(0);

    if (lane >= 16u) {
        return;
    }

    for (uint offset = lane; offset < vertex_count; offset += 16u) {
        int4 v = src_vertices_q16[src_first_vertex + offset];
        dst_vertices_q16[dst_first_vertex + offset] = (int4)(
            q16_mul(v.x, scale_q16.x),
            q16_mul(v.y, scale_q16.y),
            q16_mul(v.z, scale_q16.z),
            v.w);
    }
}
