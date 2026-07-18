// TRUEOS small-font analytical coverage.
//
// Skrifa remains the authority for the outline command stream. This kernel is
// the post-outline GPU stage: it evaluates non-zero-winding fill coverage into
// a reusable linear R8 mask. Coordinates in the command stream are already in
// mask-pixel space. Quadratic and cubic curves are flattened locally so no CPU
// fill tessellation or triangle expansion is involved.

#define OP_WORDS 8u
#define OP_MOVE 0u
#define OP_LINE 1u
#define OP_QUAD 2u
#define OP_CUBIC 3u
#define OP_CLOSE 4u

inline void accumulate_segment(
    float2 p,
    float2 a,
    float2 b,
    float *min_distance2,
    int *winding)
{
    float2 ab = b - a;
    float length2 = dot(ab, ab);
    if (length2 > 0.000001f) {
        float t = clamp(dot(p - a, ab) / length2, 0.0f, 1.0f);
        float2 nearest = a + t * ab;
        float2 delta = p - nearest;
        *min_distance2 = fmin(*min_distance2, dot(delta, delta));
    }

    // Half-open ray crossings avoid double-counting shared vertices. Contour
    // orientation is preserved, so nested counter contours cancel naturally.
    uint upward = a.y <= p.y && b.y > p.y;
    uint downward = a.y > p.y && b.y <= p.y;
    if (upward != 0u || downward != 0u) {
        float dy = b.y - a.y;
        if (fabs(dy) > 0.000001f) {
            float crossing_x = a.x + (p.y - a.y) * (b.x - a.x) / dy;
            if (crossing_x > p.x) {
                *winding += upward != 0u ? 1 : -1;
            }
        }
    }
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void font_outline_coverage_r8(
    __global const uint *outline_ops,
    __global uchar *mask_u8,
    uint op_count,
    uint subdivisions,
    uint mask_pitch_bytes,
    uint mask_width,
    uint mask_height,
    uint rect_x,
    uint rect_y,
    uint rect_width,
    uint rect_height,
    float optical_bias_px)
{
    uint local_x = get_global_id(0);
    uint local_y = get_global_id(1);
    if (local_x >= rect_width || local_y >= rect_height) {
        return;
    }

    uint x = rect_x + local_x;
    uint y = rect_y + local_y;
    if (x >= mask_width || y >= mask_height) {
        return;
    }

    subdivisions = clamp(subdivisions, 1u, 16u);
    optical_bias_px = clamp(optical_bias_px, 0.0f, 0.35f);
    float2 p = (float2)((float)x + 0.5f, (float)y + 0.5f);
    float min_distance2 = 3.402823466e+38f;
    int winding = 0;
    uint have_current = 0u;
    float2 current = (float2)(0.0f, 0.0f);
    float2 contour_start = current;

    for (uint op_index = 0u; op_index < op_count; ++op_index) {
        uint base = op_index * OP_WORDS;
        uint kind = outline_ops[base];
        float2 p0 = (float2)(
            as_float(outline_ops[base + 1u]),
            as_float(outline_ops[base + 2u]));
        float2 p1 = (float2)(
            as_float(outline_ops[base + 3u]),
            as_float(outline_ops[base + 4u]));
        float2 p2 = (float2)(
            as_float(outline_ops[base + 5u]),
            as_float(outline_ops[base + 6u]));

        if (kind == OP_MOVE) {
            current = p0;
            contour_start = p0;
            have_current = 1u;
        } else if (kind == OP_LINE && have_current != 0u) {
            accumulate_segment(p, current, p0, &min_distance2, &winding);
            current = p0;
        } else if (kind == OP_QUAD && have_current != 0u) {
            float2 start = current;
            for (uint step = 1u; step <= subdivisions; ++step) {
                float t = (float)step / (float)subdivisions;
                float one = 1.0f - t;
                float2 next = one * one * start
                    + 2.0f * one * t * p0
                    + t * t * p1;
                accumulate_segment(p, current, next, &min_distance2, &winding);
                current = next;
            }
        } else if (kind == OP_CUBIC && have_current != 0u) {
            float2 start = current;
            for (uint step = 1u; step <= subdivisions; ++step) {
                float t = (float)step / (float)subdivisions;
                float one = 1.0f - t;
                float2 next = one * one * one * start
                    + 3.0f * one * one * t * p0
                    + 3.0f * one * t * t * p1
                    + t * t * t * p2;
                accumulate_segment(p, current, next, &min_distance2, &winding);
                current = next;
            }
        } else if (kind == OP_CLOSE && have_current != 0u) {
            accumulate_segment(p, current, contour_start, &min_distance2, &winding);
            current = contour_start;
            have_current = 0u;
        }
    }

    if (!(min_distance2 < 3.402823466e+38f)) {
        return;
    }
    float distance_px = native_sqrt(fmax(min_distance2, 0.0f));
    float signed_distance_px = winding != 0 ? -distance_px : distance_px;
    float coverage = clamp(0.5f + optical_bias_px - signed_distance_px, 0.0f, 1.0f);
    uchar encoded = convert_uchar_sat_rte(coverage * 255.0f);
    uint destination = y * mask_pitch_bytes + x;
    mask_u8[destination] = max(mask_u8[destination], encoded);
}
