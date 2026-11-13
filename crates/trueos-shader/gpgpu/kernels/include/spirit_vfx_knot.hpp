// Procedural Codex backgrounds selected from tools/lilly/spirit-knot-vfx-offline/knot.frag.
// IDs 12 / 13: Spark lattice / Resonance. No logo image or lookup surface.

static inline float2 knot_rotate(float2 p, float angle)
{
    float c = cos(angle), s = sin(angle);
    return (float2)(c * p.x - s * p.y, s * p.x + c * p.y);
}

// Distance to the centerline and continuous distance along its ribbon.
static inline float2 knot_segment(float2 p, float2 a, float2 b, float offset)
{
    float2 ab = b - a;
    float h = clamp(dot(p - a, ab) / dot(ab, ab), 0.0f, 1.0f);
    return (float2)(length(p - a - ab * h), offset + h * length(ab));
}

static inline float2 knot_ribbon(float2 p)
{
    const float2 c = (float2)(-0.1246f, -0.4125f);
    const float r = 0.358f;
    float2 q = p - c;
    float a = atan2(q.y, q.x) - 0.5f * VFX_PI;
    a = a - VFX_TAU * floor(a / VFX_TAU) - VFX_PI;
    float angle = 1.5f * VFX_PI + clamp(a, -0.5f * VFX_PI, 0.5f * VFX_PI);
    float2 on_arc = c + r * (float2)(cos(angle), sin(angle));
    float2 result = (float2)(length(p - on_arc), (VFX_TAU - angle) * r);
    float arc_length = VFX_PI * r;
    float2 start = c - (float2)(r, 0.0f);
    float2 end = knot_rotate(c + (float2)(r, 0.0f), 2.0f * VFX_PI / 3.0f);
    float2 corner = (float2)(start.x, end.y - (end.x - start.x) / sqrt(3.0f));
    float2 line = knot_segment(p, start, corner, arc_length);
    if (line.x < result.x) result = line;
    line = knot_segment(p, corner, end, arc_length + length(corner - start));
    if (line.x < result.x) result = line;
    return result;
}

static inline uint vfx_knot_background(
    uint mode, float2 uv, float2 point, float scale, float time,
    float intensity, float opacity, float3 color_a, float3 color_b)
{
    float2 p = point * 2.5f;
    float breath = 1.0f + 0.024f * sin(time * 1.7f);
    p /= breath;
    if (mode == 13u) p = knot_rotate(p, 0.07f * sin(time * 0.55f));
    float aa = 2.5f / (256.0f * scale * breath);
    const float half_width = 0.047f;
    float nearest = 10.0f, ribbon_fill = 0.0f, seam = 0.0f;
    for (uint i = 0u; i < 6u; ++i) {
        float2 q = knot_rotate(p, -(float)i * VFX_PI / 3.0f);
        float2 d = knot_ribbon(q);
        nearest = min(nearest, d.x);
        float fill = 1.0f - smoothstep(half_width - aa, half_width + aa, d.x);
        float next_distance = knot_ribbon(knot_rotate(q, -VFX_PI / 3.0f)).x;
        float gap = 1.0f - smoothstep(half_width + 0.012f, half_width + 0.012f + aa,
                                      next_distance);
        ribbon_fill = max(ribbon_fill, fill * (1.0f - gap));
        seam = max(seam, (1.0f - smoothstep(0.006f, 0.006f + aa,
                                          fabs(d.x - 0.036f))) * (1.0f - gap));
    }
    float distance = max(0.0f, nearest - half_width);
    float glow = exp(-distance * 28.0f);
    float hue = 0.5f + 0.5f * sin(atan2(p.y, p.x) * 2.0f - time * 0.4f + length(p) * 3.0f);
    float3 color = mix(color_a, color_b, hue);
    float alpha;
    if (mode == 12u) {
        float2 grid = (p + (float2)(0.0f, time * 0.012f)) * 52.0f;
        float2 cell = floor(grid);
        float seed = vfx_hash21f(cell);
        float2 offset = (float2)(seed, vfx_hash21f(cell + 7.3f)) * 0.55f + 0.225f;
        float dot_radius = 0.14f + 0.13f * seed;
        float dot_mask = 1.0f - smoothstep(dot_radius, dot_radius + 0.11f,
                                          length(vfx_fract2(grid) - offset));
        float shimmer = 0.5f + 0.5f * sin(time * 2.0f + seed * VFX_TAU);
        float twinkle = 0.35f + 0.65f * shimmer * shimmer;
        float sparks = dot_mask * twinkle * exp(-distance * 20.0f);
        alpha = ribbon_fill * 0.14f + seam * 0.12f + sparks * 0.85f + glow * 0.055f;
        color = mix(color, (float3)(0.95f, 0.99f, 1.0f), sparks * 0.65f);
    } else {
        float wave = vfx_fract(distance * 5.0f - time * 0.23f);
        float band = (wave - 0.5f) * 18.0f;
        float echo = exp(-band * band) * exp(-distance * 12.0f)
            * smoothstep(0.01f, 0.04f, distance);
        alpha = ribbon_fill * 0.34f + seam * 0.16f + glow * 0.08f + echo * 0.48f;
        color = mix(color, color_b, 0.35f * echo);
    }
    float edge = min(min(uv.x, uv.y), min(1.0f - uv.x, 1.0f - uv.y));
    alpha = clamp(alpha * intensity * 0.5f * opacity, 0.0f, 0.96f)
        * smoothstep(0.015625f, 0.06f, edge);
    return vfx_pack_bgra8_premultiplied(color, alpha);
}
