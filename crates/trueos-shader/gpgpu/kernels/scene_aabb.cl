// SceneDB positional AABB query for TRUEOS Xe-LP vVideoMem.
//
// All pointer arguments name pages in the submitting tenant's PPGTT.  The
// output contract intentionally mirrors pulsar_scenedb's scalar reference:
// closed intervals, ordered comparisons, liveness rejection, and UINT_MAX as
// the miss token.  The host clears hit_count before dispatch.

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void scene_aabb(
    __global const float *min_x,
    __global const float *max_x,
    __global const float *min_y,
    __global const float *max_y,
    __global const float *min_z,
    __global const float *max_z,
    __global const ulong *liveness,
    __global uint *output,
    __global volatile uint *hit_count,
    uint rows,
    float query_min_x,
    float query_min_y,
    float query_min_z,
    float query_max_x,
    float query_max_y,
    float query_max_z)
{
    uint row = get_global_id(0);
    if (row >= rows) {
        return;
    }

    ulong word = liveness[row >> 6];
    int live = ((word >> (row & 63u)) & 1ul) != 0ul;
    int visible = min_x[row] <= query_max_x
        && max_x[row] >= query_min_x
        && min_y[row] <= query_max_y
        && max_y[row] >= query_min_y
        && min_z[row] <= query_max_z
        && max_z[row] >= query_min_z
        && live;

    output[row] = visible ? row : 0xFFFFFFFFu;
    if (visible) {
        atomic_inc(hit_count);
    }
}
