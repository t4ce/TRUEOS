// TRUEOS Gen12/Alder Lake GPGPU kernel seed.
//
// Contract:
// - Stage1 affine plane patch worklist fill: each descriptor is one 3D plane
//   patch with up to eight local half-space cuts.
// - One work item covers an 8x16 pixel tile. The CPU emits multiple proven
//   single-group walkers and passes a base tile index per walker.
// - Each pixel worker walks descriptors in order, preserving painter-order
//   behavior while still fanning the expensive plane math across the surface.
// - One destination RGBA8/XRGB-style surface per dispatch.
// - Camera convention matches canvas3d_project_rgba8:
//     screen_x = center_x + x * focal / z
//     screen_y = center_y - y * focal / z
// - The plane is P(u,v) = origin + u * axis_u + v * axis_v, all Q16.
// - Each cut is int4 { a, b, c, ignored } in local plane coordinates:
//     a*u + b*v + c >= 0
//   with a,b,c in Q16.
// - When descriptor flags has PATCH_DESC_FLAG_SCREEN_EDGES set, the cut
//   slots are screen-space edge equations instead:
//     a*x + b*y + c >= 0
//   with x/y in destination pixels. This avoids per-pixel ray/plane division for
//   CPU-projected polygon faces.

#define PATCH_DESC_DWORDS 56u
#define PATCH_TILE_PIXELS_PER_LANE 8u
#define PATCH_TILE_ROWS 16u
#define PATCH_DESC_FLAG_SCREEN_EDGES 1u
#define PATCH_CONSTRAINT_BASE_DWORD 22u
#define PATCH_MAX_CONSTRAINTS 8u

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

static inline int screen_edge_ok(int4 edge, int x, int y)
{
    long value = ((long)edge.x * (long)x)
        + ((long)edge.y * (long)y)
        + (long)edge.z;
    return value >= 0;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void canvas3d_plane_patch_worklist_rgba8(
    __global const uint *unused_src,
    __global uint *dst_rgba,
    __global const uint *descs,
    uint desc_base,
    uint desc_count,
    uint work_base)
{
    (void)unused_src;

    uint lane = get_global_id(0);
    if (lane >= 16u) {
        return;
    }

    uint work_index = work_base + lane;
    if (desc_count == 0u) {
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
        uint flags = descs[desc_index + 9u];
        int4 origin_q16 = load_int4_desc(descs, desc_index + 10u);
        int4 axis_u_q16 = load_int4_desc(descs, desc_index + 14u);
        int4 axis_v_q16 = load_int4_desc(descs, desc_index + 18u);
        int4 constraints_q16[PATCH_MAX_CONSTRAINTS];
        for (uint constraint_index = 0u; constraint_index < PATCH_MAX_CONSTRAINTS; constraint_index++) {
            constraints_q16[constraint_index] = load_int4_desc(
                descs,
                desc_index + PATCH_CONSTRAINT_BASE_DWORD + constraint_index * 4u);
        }
        uint constraint_count = descs[desc_index + 54u];
        uint color_rgba = descs[desc_index + 55u];

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

        uint tile_cols = (max_w + PATCH_TILE_PIXELS_PER_LANE - 1u) / PATCH_TILE_PIXELS_PER_LANE;
        uint tile_rows = (max_h + PATCH_TILE_ROWS - 1u) / PATCH_TILE_ROWS;
        if (work_index >= tile_cols * tile_rows) {
            continue;
        }

        uint tile_x = (work_index % tile_cols) * PATCH_TILE_PIXELS_PER_LANE;
        uint tile_y = (work_index / tile_cols) * PATCH_TILE_ROWS;
        if (tile_x >= max_w || tile_y >= max_h) {
            continue;
        }

        uint cuts = min(constraint_count, PATCH_MAX_CONSTRAINTS);
        if ((flags & PATCH_DESC_FLAG_SCREEN_EDGES) != 0u) {
            for (uint row = 0u; row < PATCH_TILE_ROWS; row++) {
                uint ly = tile_y + row;
                if (ly >= max_h) {
                    break;
                }
                for (uint pixel = 0u; pixel < PATCH_TILE_PIXELS_PER_LANE; pixel++) {
                    uint lx = tile_x + pixel;
                    if (lx >= max_w) {
                        continue;
                    }
                    uint sx_u = rect_x + lx;
                    uint sy_u = rect_y + ly;
                    int sx = (int)sx_u;
                    int sy = (int)sy_u;

                    int keep = 1;
                    for (uint cut = 0u; cut < cuts; cut++) {
                        keep &= screen_edge_ok(constraints_q16[cut], sx, sy);
                    }
                    if (keep) {
                        dst_rgba[sy_u * pitch_pixels + sx_u] = color_rgba;
                    }
                }
            }
            continue;
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

        for (uint row = 0u; row < PATCH_TILE_ROWS; row++) {
            uint ly = tile_y + row;
            if (ly >= max_h) {
                break;
            }
            for (uint pixel = 0u; pixel < PATCH_TILE_PIXELS_PER_LANE; pixel++) {
                uint lx = tile_x + pixel;
                if (lx >= max_w) {
                    continue;
                }
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
                for (uint cut = 0u; cut < cuts; cut++) {
                    keep &= constraint_ok(constraints_q16[cut], u_q16, v_q16);
                }
                if (keep) {
                    dst_rgba[sy_u * pitch_pixels + sx_u] = color_rgba;
                }
            }
        }
    }
}
