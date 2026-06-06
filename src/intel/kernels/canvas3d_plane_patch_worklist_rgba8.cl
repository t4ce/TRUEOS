// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Stage1 affine plane patch worklist fill: each descriptor is one 3D plane
//   patch with up to four local half-space cuts.
// - Descriptors are processed in order. Each descriptor uses all SIMD16 lanes
//   across its pixels, which keeps painter-order behavior simple for bringup.
// - One destination RGBA8/XRGB-style surface per dispatch.
// - Camera convention matches canvas3d_project_rgba8:
//     screen_x = center_x + x * focal / z
//     screen_y = center_y - y * focal / z
// - The plane is P(u,v) = origin + u * axis_u + v * axis_v, all Q16.
// - Each cut is int4 { a, b, c, ignored } in local plane coordinates:
//     a*u + b*v + c >= 0
//   with a,b,c in Q16.

#define PATCH_DESC_DWORDS 40u

static inline long dot3_q16(int4 a, int4 b)
{
    return (((long)a.x * (long)b.x)
        + ((long)a.y * (long)b.y)
        + ((long)a.z * (long)b.z)) >> 16;
}

static inline long q16_mul_long(long a, long b)
{
    return (a * b) >> 16;
}

static inline int4 cross3_q16(int4 a, int4 b)
{
    int4 out;
    out.x = (int)((((long)a.y * (long)b.z) - ((long)a.z * (long)b.y)) >> 16);
    out.y = (int)((((long)a.z * (long)b.x) - ((long)a.x * (long)b.z)) >> 16);
    out.z = (int)((((long)a.x * (long)b.y) - ((long)a.y * (long)b.x)) >> 16);
    out.w = 0;
    return out;
}

static inline int4 load_int4_desc(__global const uint *descs, uint base)
{
    int4 out;
    out.x = (int)descs[base + 0u];
    out.y = (int)descs[base + 1u];
    out.z = (int)descs[base + 2u];
    out.w = (int)descs[base + 3u];
    return out;
}

static inline int constraint_ok(int4 constraint, int u_q16, int v_q16)
{
    long value = (((long)constraint.x * (long)u_q16) >> 16)
        + (((long)constraint.y * (long)v_q16) >> 16)
        + (long)constraint.z;
    return value >= 0;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void canvas3d_plane_patch_worklist_rgba8(
    __global const uint *unused_src,
    __global uint *dst_rgba,
    __global const uint *descs,
    uint desc_base,
    uint desc_count)
{
    (void)unused_src;

    uint lane = get_global_id(0);
    if (lane >= 16u) {
        return;
    }

    for (uint local_desc_id = 0u; local_desc_id < desc_count; local_desc_id++) {
        uint desc_index = (desc_base + local_desc_id) * PATCH_DESC_DWORDS;
        uint dst_pitch_bytes = descs[desc_index + 0u];
        uint dst_width = descs[desc_index + 1u];
        uint dst_height = descs[desc_index + 2u];
        uint rect_x = descs[desc_index + 3u];
        uint rect_y = descs[desc_index + 4u];
        uint rect_width = descs[desc_index + 5u];
        uint rect_height = descs[desc_index + 6u];
        uint canvas_width = descs[desc_index + 7u];
        uint canvas_height = descs[desc_index + 8u];
        int4 origin_q16 = load_int4_desc(descs, desc_index + 10u);
        int4 axis_u_q16 = load_int4_desc(descs, desc_index + 14u);
        int4 axis_v_q16 = load_int4_desc(descs, desc_index + 18u);
        int4 constraint0_q16 = load_int4_desc(descs, desc_index + 22u);
        int4 constraint1_q16 = load_int4_desc(descs, desc_index + 26u);
        int4 constraint2_q16 = load_int4_desc(descs, desc_index + 30u);
        int4 constraint3_q16 = load_int4_desc(descs, desc_index + 34u);
        uint constraint_count = descs[desc_index + 38u];
        uint color_rgba = descs[desc_index + 39u];

        uint focal = min(canvas_width, canvas_height) >> 1;
        if (dst_pitch_bytes == 0u || dst_width == 0u || dst_height == 0u
            || rect_width == 0u || rect_height == 0u
            || canvas_width == 0u || canvas_height == 0u || focal == 0u) {
            continue;
        }

        uint pitch_pixels = dst_pitch_bytes >> 2;
        uint max_w = rect_width;
        uint max_h = rect_height;
        if (rect_x >= dst_width || rect_y >= dst_height) {
            continue;
        }
        if (max_w > dst_width - rect_x) {
            max_w = dst_width - rect_x;
        }
        if (max_h > dst_height - rect_y) {
            max_h = dst_height - rect_y;
        }

        int center_x = (int)(canvas_width >> 1);
        int center_y = (int)(canvas_height >> 1);
        int4 normal_q16 = cross3_q16(axis_u_q16, axis_v_q16);
        long plane_dot = dot3_q16(normal_q16, origin_q16);
        long uu = dot3_q16(axis_u_q16, axis_u_q16);
        long uv = dot3_q16(axis_u_q16, axis_v_q16);
        long vv = dot3_q16(axis_v_q16, axis_v_q16);
        long det = uu * vv - uv * uv;
        if (plane_dot == 0 || det == 0) {
            continue;
        }

        uint cuts = min(constraint_count, 4u);
        uint total = max_w * max_h;
        for (uint offset = lane; offset < total; offset += 16u) {
            uint lx = offset % max_w;
            uint ly = offset / max_w;
            uint sx_u = rect_x + lx;
            uint sy_u = rect_y + ly;
            int sx = (int)sx_u;
            int sy = (int)sy_u;

            int4 ray_q16;
            ray_q16.x = (int)((((long)(sx - center_x)) << 16) / (long)focal);
            ray_q16.y = (int)((((long)(center_y - sy)) << 16) / (long)focal);
            ray_q16.z = 65536;
            ray_q16.w = 0;

            long denom = dot3_q16(normal_q16, ray_q16);
            if (denom == 0) {
                continue;
            }
            long t_q16 = (plane_dot << 16) / denom;
            if (t_q16 <= 0) {
                continue;
            }

            int4 hit_q16;
            hit_q16.x = (int)q16_mul_long(ray_q16.x, t_q16);
            hit_q16.y = (int)q16_mul_long(ray_q16.y, t_q16);
            hit_q16.z = (int)q16_mul_long(ray_q16.z, t_q16);
            hit_q16.w = 0;

            int4 delta_q16;
            delta_q16.x = hit_q16.x - origin_q16.x;
            delta_q16.y = hit_q16.y - origin_q16.y;
            delta_q16.z = hit_q16.z - origin_q16.z;
            delta_q16.w = 0;

            long du = dot3_q16(delta_q16, axis_u_q16);
            long dv = dot3_q16(delta_q16, axis_v_q16);
            int u_q16 = (int)(((du * vv - dv * uv) << 16) / det);
            int v_q16 = (int)(((dv * uu - du * uv) << 16) / det);

            int keep = 1;
            if (cuts > 0u) keep &= constraint_ok(constraint0_q16, u_q16, v_q16);
            if (cuts > 1u) keep &= constraint_ok(constraint1_q16, u_q16, v_q16);
            if (cuts > 2u) keep &= constraint_ok(constraint2_q16, u_q16, v_q16);
            if (cuts > 3u) keep &= constraint_ok(constraint3_q16, u_q16, v_q16);
            if (keep) {
                dst_rgba[sy_u * pitch_pixels + sx_u] = color_rgba;
            }
        }
    }
}
