// Staged vector-font compute bring-up.
//
// The CPU uploads size-independent Skrifa outline records once. Stage 1 checks
// the ABI, stage 2 flattens curves into point storage, and stage 3 emits an
// indexed contour-stroke mesh. The stroke mesh is intentionally not presented
// as fill tessellation: hole-aware contour resolution is the next artifact.

#define OP_WORDS 8u
#define OP_MOVE 0u
#define OP_LINE 1u
#define OP_QUAD 2u
#define OP_CUBIC 3u
#define OP_CLOSE 4u

#define STAGE_AUDIT 1u
#define STAGE_FLATTEN 2u
#define STAGE_STROKE_MESH 3u

#define REPORT_DWORDS 64u
#define VERTEX_DWORD_OFFSET 64u
#define INDEX_DWORD_OFFSET 8192u
#define OUTPUT_LAYOUT_VERSION 2u
#define RESULT_MAGIC 0xF07ECA00u
#define RESULT_DONE 0xC001D00Du

inline float2 map_font_point(float2 p, float scale, float2 origin) {
    // The render viewport already has a negative Y scale. Preserve the font's
    // Y-up coordinates here so ascenders remain above the baseline on screen.
    return (float2)(origin.x + p.x * scale, origin.y + p.y * scale);
}

inline void include_bounds(float2 p, float2 *lo, float2 *hi, uint *have_bounds) {
    if (*have_bounds == 0u) {
        *lo = p;
        *hi = p;
        *have_bounds = 1u;
    } else {
        *lo = fmin(*lo, p);
        *hi = fmax(*hi, p);
    }
}

inline uint emit_flat_point(
    __global uint *output,
    float2 p,
    uint *vertex_count,
    uint max_vertices,
    uint *truncated
) {
    if (*vertex_count >= max_vertices) {
        *truncated = 1u;
        return 0u;
    }
    uint word = VERTEX_DWORD_OFFSET + *vertex_count * 2u;
    output[word] = as_uint(p.x);
    output[word + 1u] = as_uint(p.y);
    *vertex_count += 1u;
    return 1u;
}

inline uint emit_stroke_segment(
    __global uint *output,
    float2 a,
    float2 b,
    float half_width,
    uint *vertex_count,
    uint *index_count,
    uint max_vertices,
    uint max_indices,
    uint *truncated
) {
    float2 delta = b - a;
    float length2 = dot(delta, delta);
    if (!(length2 > 0.000001f)) {
        return 0u;
    }
    if (*vertex_count + 4u > max_vertices || *index_count + 6u > max_indices) {
        *truncated = 1u;
        return 0u;
    }
    float inv_length = native_rsqrt(length2);
    float2 normal = (float2)(-delta.y, delta.x) * (half_width * inv_length);
    float2 q0 = a + normal;
    float2 q1 = a - normal;
    float2 q2 = b + normal;
    float2 q3 = b - normal;
    uint base = *vertex_count;
    uint vertex_word = VERTEX_DWORD_OFFSET + base * 2u;
    output[vertex_word + 0u] = as_uint(q0.x);
    output[vertex_word + 1u] = as_uint(q0.y);
    output[vertex_word + 2u] = as_uint(q1.x);
    output[vertex_word + 3u] = as_uint(q1.y);
    output[vertex_word + 4u] = as_uint(q2.x);
    output[vertex_word + 5u] = as_uint(q2.y);
    output[vertex_word + 6u] = as_uint(q3.x);
    output[vertex_word + 7u] = as_uint(q3.y);
    uint index_word = INDEX_DWORD_OFFSET + *index_count;
    output[index_word + 0u] = base + 0u;
    output[index_word + 1u] = base + 1u;
    output[index_word + 2u] = base + 2u;
    output[index_word + 3u] = base + 2u;
    output[index_word + 4u] = base + 1u;
    output[index_word + 5u] = base + 3u;
    *vertex_count += 4u;
    *index_count += 6u;
    return 1u;
}

inline void emit_segment(
    __global uint *output,
    uint stage,
    float2 a,
    float2 b,
    float half_width,
    uint *vertex_count,
    uint *index_count,
    uint max_vertices,
    uint max_indices,
    uint *segment_count,
    uint *truncated
) {
    *segment_count += 1u;
    if (stage == STAGE_FLATTEN) {
        emit_flat_point(output, b, vertex_count, max_vertices, truncated);
    } else if (stage == STAGE_STROKE_MESH) {
        emit_stroke_segment(
            output, a, b, half_width, vertex_count, index_count,
            max_vertices, max_indices, truncated
        );
    }
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void font_outline_mesh(
    __global const uint *outline_ops,
    __global uint *output,
    uint op_count,
    uint stage,
    uint subdivisions,
    uint max_vertices,
    uint max_indices,
    float scale,
    float origin_x,
    float origin_y,
    float stroke_half_width
) {
    if (get_global_id(0) != 0u) {
        return;
    }

    for (uint i = 0u; i < REPORT_DWORDS; ++i) {
        output[i] = 0u;
    }

    subdivisions = clamp(subdivisions, 1u, 32u);
    uint checksum = 0x811C9DC5u;
    uint move_count = 0u;
    uint line_count = 0u;
    uint quad_count = 0u;
    uint cubic_count = 0u;
    uint close_count = 0u;
    uint invalid = 0u;
    uint truncated = 0u;
    uint vertex_count = 0u;
    uint index_count = 0u;
    uint segment_count = 0u;
    uint have_current = 0u;
    uint contour_open = 0u;
    uint have_bounds = 0u;
    float2 current = (float2)(0.0f, 0.0f);
    float2 contour_start = current;
    float2 bounds_lo = current;
    float2 bounds_hi = current;
    float2 origin = (float2)(origin_x, origin_y);

    for (uint op_index = 0u; op_index < op_count; ++op_index) {
        uint base = op_index * OP_WORDS;
        for (uint word = 0u; word < OP_WORDS; ++word) {
            checksum ^= outline_ops[base + word];
            checksum *= 0x01000193u;
        }
        uint kind = outline_ops[base];
        if (kind > OP_CLOSE) {
            invalid += 1u;
            continue;
        }
        if (kind == OP_MOVE) move_count += 1u;
        else if (kind == OP_LINE) line_count += 1u;
        else if (kind == OP_QUAD) quad_count += 1u;
        else if (kind == OP_CUBIC) cubic_count += 1u;
        else close_count += 1u;

        float2 raw0 = (float2)(as_float(outline_ops[base + 1u]), as_float(outline_ops[base + 2u]));
        float2 raw1 = (float2)(as_float(outline_ops[base + 3u]), as_float(outline_ops[base + 4u]));
        float2 raw2 = (float2)(as_float(outline_ops[base + 5u]), as_float(outline_ops[base + 6u]));
        uint coords_ok = 1u;
        if (kind == OP_MOVE || kind == OP_LINE) {
            coords_ok = all(isfinite(raw0));
        } else if (kind == OP_QUAD) {
            coords_ok = all(isfinite(raw0)) && all(isfinite(raw1));
        } else if (kind == OP_CUBIC) {
            coords_ok = all(isfinite(raw0)) && all(isfinite(raw1)) && all(isfinite(raw2));
        }
        uint sequence_ok = 1u;
        if (kind == OP_MOVE) {
            contour_open = 1u;
        } else if (kind == OP_CLOSE) {
            sequence_ok = contour_open;
            contour_open = 0u;
        } else {
            sequence_ok = contour_open;
        }
        if (coords_ok == 0u || sequence_ok == 0u || outline_ops[base + 7u] != 0u) {
            invalid += 1u;
            if (stage != STAGE_AUDIT) {
                continue;
            }
        }

        if (stage == STAGE_AUDIT) {
            continue;
        }

        float2 p0 = map_font_point(raw0, scale, origin);
        float2 p1 = map_font_point(raw1, scale, origin);
        float2 p2 = map_font_point(raw2, scale, origin);

        if (kind == OP_MOVE) {
            current = p0;
            contour_start = p0;
            have_current = 1u;
            include_bounds(current, &bounds_lo, &bounds_hi, &have_bounds);
            if (stage == STAGE_FLATTEN) {
                emit_flat_point(output, current, &vertex_count, max_vertices, &truncated);
            }
        } else if (kind == OP_LINE && have_current != 0u) {
            include_bounds(p0, &bounds_lo, &bounds_hi, &have_bounds);
            emit_segment(
                output, stage, current, p0, stroke_half_width,
                &vertex_count, &index_count, max_vertices, max_indices,
                &segment_count, &truncated
            );
            current = p0;
        } else if (kind == OP_QUAD && have_current != 0u) {
            float2 start = current;
            for (uint step = 1u; step <= subdivisions; ++step) {
                float t = (float)step / (float)subdivisions;
                float one = 1.0f - t;
                float2 point = one * one * start + 2.0f * one * t * p0 + t * t * p1;
                include_bounds(point, &bounds_lo, &bounds_hi, &have_bounds);
                emit_segment(
                    output, stage, current, point, stroke_half_width,
                    &vertex_count, &index_count, max_vertices, max_indices,
                    &segment_count, &truncated
                );
                current = point;
            }
        } else if (kind == OP_CUBIC && have_current != 0u) {
            float2 start = current;
            for (uint step = 1u; step <= subdivisions; ++step) {
                float t = (float)step / (float)subdivisions;
                float one = 1.0f - t;
                float2 point = one * one * one * start
                    + 3.0f * one * one * t * p0
                    + 3.0f * one * t * t * p1
                    + t * t * t * p2;
                include_bounds(point, &bounds_lo, &bounds_hi, &have_bounds);
                emit_segment(
                    output, stage, current, point, stroke_half_width,
                    &vertex_count, &index_count, max_vertices, max_indices,
                    &segment_count, &truncated
                );
                current = point;
            }
        } else if (kind == OP_CLOSE && have_current != 0u) {
            emit_segment(
                output, stage, current, contour_start, stroke_half_width,
                &vertex_count, &index_count, max_vertices, max_indices,
                &segment_count, &truncated
            );
            current = contour_start;
            have_current = 0u;
        } else if (kind != OP_CLOSE) {
            invalid += 1u;
        }
    }

    if (contour_open != 0u) {
        invalid += 1u;
    }

    uint valid_stage = stage >= STAGE_AUDIT && stage <= STAGE_STROKE_MESH;
    uint status = (invalid == 0u && valid_stage != 0u) ? 1u : 0u;
    output[0] = RESULT_MAGIC | (stage & 0xFFu);
    output[1] = status | (truncated << 1u);
    output[2] = stage;
    output[3] = op_count;
    output[4] = move_count;
    output[5] = line_count;
    output[6] = quad_count;
    output[7] = cubic_count;
    output[8] = close_count;
    output[9] = vertex_count;
    output[10] = segment_count;
    output[11] = stage == STAGE_STROKE_MESH ? vertex_count : 0u;
    output[12] = index_count;
    output[13] = checksum;
    output[14] = invalid;
    output[15] = truncated;
    output[16] = as_uint(bounds_lo.x);
    output[17] = as_uint(bounds_lo.y);
    output[18] = as_uint(bounds_hi.x);
    output[19] = as_uint(bounds_hi.y);
    output[20] = subdivisions;
    output[21] = OUTPUT_LAYOUT_VERSION;
    output[22] = VERTEX_DWORD_OFFSET;
    output[23] = INDEX_DWORD_OFFSET;
    output[24] = RESULT_DONE;
}
