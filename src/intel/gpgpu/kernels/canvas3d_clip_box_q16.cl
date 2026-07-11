// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Input and output vertices are fixed-point Q16 vec3 values stored as int4
//   { x, y, z, pad }.
// - The walker clips the source subset
//   [src_first_vertex, src_first_vertex + vertex_count) into the destination
//   subset [dst_first_vertex, dst_first_vertex + vertex_count).
// - min_q16/max_q16 are int4 { x, y, z, ignored } bounds in Q16 units.
// - The source vertex is not modified; the pad lane is preserved in the sink.

static inline int clamp_i32(int value, int lo, int hi)
{
    return min(max(value, lo), hi);
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void canvas3d_clip_box_q16(
    __global const int4 *src_vertices_q16,
    __global int4 *dst_vertices_q16,
    uint src_first_vertex,
    uint dst_first_vertex,
    uint vertex_count,
    int4 min_q16,
    int4 max_q16)
{
    uint lane = get_global_id(0);

    if (lane >= 16u) {
        return;
    }

    for (uint offset = lane; offset < vertex_count; offset += 16u) {
        int4 v = src_vertices_q16[src_first_vertex + offset];
        dst_vertices_q16[dst_first_vertex + offset] = (int4)(
            clamp_i32(v.x, min_q16.x, max_q16.x),
            clamp_i32(v.y, min_q16.y, max_q16.y),
            clamp_i32(v.z, min_q16.z, max_q16.z),
            v.w);
    }
}
