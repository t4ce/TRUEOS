// Spirit VFX procedural-alpha background pass for Intel Xe-LP / ADL-S.
//
// The dispatch is fixed at 256x256. It writes premultiplied BGRA8 bytes into
// the exact Intel cursor backbuffer. The selected preview.html modes retain
// their original IDs 2 through 10, names, parameters, and two-color contract.

#define SPIRIT_VFX_SIZE 256u
#define SPIRIT_VFX_CONTROL_MAGIC 0x53564658u // "SVFX"
#define SPIRIT_VFX_CONTROL_VERSION 1u
#define VFX_PI  3.14159265359f
#define VFX_TAU 6.28318530718f

#define VFX_CTRL_MAGIC              0u
#define VFX_CTRL_VERSION            1u
#define VFX_CTRL_FRAME              2u
#define VFX_CTRL_BACKGROUND_ID      3u
#define VFX_CTRL_TIME_F32           4u
#define VFX_CTRL_BG_OPACITY_F32     5u
#define VFX_CTRL_BG_SCALE_F32       6u
#define VFX_CTRL_BG_SPEED_F32       7u
#define VFX_CTRL_BG_INTENSITY_F32   8u
#define VFX_CTRL_BG_COLOR_A         9u
#define VFX_CTRL_BG_COLOR_B        10u
#define VFX_CTRL_POSITION_X_F32    11u
#define VFX_CTRL_POSITION_Y_F32    12u
#define VFX_CTRL_DST_PITCH         27u

static inline float vfx_finite_or(float value, float fallback)
{
    return isfinite(value) ? value : fallback;
}

static inline float vfx_clamp01(float value)
{
    return clamp(value, 0.0f, 1.0f);
}

// All authored powers use non-negative bases and fixed integer exponents.
// Keeping them multiply-only prevents IGC from introducing an implicit
// constant surface and external pow helper into this two-BTI artifact.
static inline float vfx_powi(float base, uint exponent)
{
    float result = 1.0f;
    while (exponent != 0u) {
        if ((exponent & 1u) != 0u) {
            result *= base;
        }
        base *= base;
        exponent >>= 1;
    }
    return result;
}

static inline float vfx_fract(float value)
{
    return value - floor(value);
}

static inline float2 vfx_fract2(float2 value)
{
    return value - floor(value);
}

// Float hash and FBM intentionally match preview.html. Keeping the reference
// hash makes the authored palettes and thresholds transfer predictably.
static inline float vfx_hash21f(float2 point)
{
    point = vfx_fract2(point * (float2)(123.34f, 456.21f));
    point += dot(point, point + 45.32f);
    return vfx_fract(point.x * point.y);
}

static inline float vfx_noise21(float2 point)
{
    float2 floored = floor(point);
    float2 fraction = point - floored;
    fraction = fraction * fraction * (3.0f - 2.0f * fraction);
    float a = vfx_hash21f(floored);
    float b = vfx_hash21f(floored + (float2)(1.0f, 0.0f));
    float c = vfx_hash21f(floored + (float2)(0.0f, 1.0f));
    float d = vfx_hash21f(floored + (float2)(1.0f, 1.0f));
    return mix(mix(a, b, fraction.x), mix(c, d, fraction.x), fraction.y);
}

static inline float vfx_fbm(float2 point)
{
    float value = 0.0f;
    float amplitude = 0.5f;
    for (uint octave = 0u; octave < 5u; octave++) {
        value += amplitude * vfx_noise21(point);
        point = (float2)(
            1.6f * point.x - 1.2f * point.y,
            1.2f * point.x + 1.6f * point.y);
        amplitude *= 0.5f;
    }
    return value;
}

static inline float3 vfx_unpack_rgb(uint packed)
{
    return (float3)(
        (float)(packed & 0xFFu),
        (float)((packed >> 8) & 0xFFu),
        (float)((packed >> 16) & 0xFFu)) * (1.0f / 255.0f);
}

// Intel's ARGB cursor consumes B,G,R,A bytes at increasing addresses. The
// dword is therefore AARRGGBB on this little-endian target.
static inline uint vfx_pack_bgra8_premultiplied(float3 color, float alpha)
{
    float a = vfx_clamp01(alpha);
    float3 premultiplied = clamp(color, (float3)(0.0f), (float3)(1.0f)) * a;
    uint b = (uint)(premultiplied.z * 255.0f + 0.5f);
    uint g = (uint)(premultiplied.y * 255.0f + 0.5f);
    uint r = (uint)(premultiplied.x * 255.0f + 0.5f);
    uint ai = (uint)(a * 255.0f + 0.5f);
    return (ai << 24) | (r << 16) | (g << 8) | b;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void spirit_vfx_background_rgba8(
    __global const uint *control,
    __global uint *dst_bgra)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);
    if (x >= SPIRIT_VFX_SIZE || y >= SPIRIT_VFX_SIZE) {
        return;
    }

    uint dst_pitch_bytes = control[VFX_CTRL_DST_PITCH];
    if (dst_pitch_bytes < SPIRIT_VFX_SIZE * 4u
        || control[VFX_CTRL_MAGIC] != SPIRIT_VFX_CONTROL_MAGIC
        || control[VFX_CTRL_VERSION] != SPIRIT_VFX_CONTROL_VERSION) {
        return;
    }

    float2 uv = ((float2)((float)x + 0.5f, (float)y + 0.5f)) * (1.0f / 256.0f);
    float2 center = (float2)(0.5f) + (float2)(
        clamp(vfx_finite_or(as_float(control[VFX_CTRL_POSITION_X_F32]), 0.0f), -0.35f, 0.35f),
        clamp(vfx_finite_or(as_float(control[VFX_CTRL_POSITION_Y_F32]), 0.0f), -0.35f, 0.35f));
    float scale = clamp(
        vfx_finite_or(as_float(control[VFX_CTRL_BG_SCALE_F32]), 1.0f),
        0.25f,
        3.0f);
    float2 point = (uv - center) / max(0.2f, scale);
    float radius = native_sqrt(max(dot(point, point), 1.0e-8f));
    float angle = atan2(point.y, point.x);
    float time = vfx_finite_or(as_float(control[VFX_CTRL_TIME_F32]), 0.0f);
    float opacity = clamp(
        vfx_finite_or(as_float(control[VFX_CTRL_BG_OPACITY_F32]), 0.0f),
        0.0f,
        1.0f);
    float speed = clamp(
        vfx_finite_or(as_float(control[VFX_CTRL_BG_SPEED_F32]), 1.0f),
        0.0f,
        4.0f);
    float intensity = clamp(
        vfx_finite_or(as_float(control[VFX_CTRL_BG_INTENSITY_F32]), 1.0f),
        0.1f,
        2.5f);

    float alpha = 0.0f;
    float color_mix = 0.0f;
    uint background_id = control[VFX_CTRL_BACKGROUND_ID];
    if (background_id == 2u) {
        // preview.html: Energy ring
        float ring_radius = 0.31f + 0.012f * native_sin(time * speed * 2.0f);
        float ring = native_exp(-fabs(radius - ring_radius) * 85.0f);
        float ring2 = native_exp(-fabs(radius - (ring_radius + 0.05f)) * 120.0f)
            * 0.45f;
        float arc_noise = vfx_fbm((float2)(angle * 2.0f, radius * 12.0f));
        float arc_wave = 0.5f + 0.5f * native_sin(
            angle * 9.0f - time * speed * 2.2f + arc_noise * 4.0f);
        float arcs = vfx_powi(arc_wave, 10u);
        alpha = (ring * (0.5f + 0.8f * arcs) + ring2) * intensity;
        color_mix = 0.5f + 0.5f * native_sin(angle * 3.0f + time * speed);
    } else if (background_id == 3u) {
        // preview.html: Magic circle
        float ring1 = native_exp(-fabs(radius - 0.32f) * 130.0f);
        float ring2 = native_exp(-fabs(radius - 0.24f) * 160.0f) * 0.7f;
        float spokes = vfx_powi(
            0.5f + 0.5f * native_cos(angle * 12.0f), 28u)
            * smoothstep(0.12f, 0.18f, radius)
            * (1.0f - smoothstep(0.30f, 0.37f, radius));
        float ticks = vfx_powi(
            0.5f + 0.5f * native_cos(angle * 48.0f - time * speed * 0.35f),
            36u)
            * native_exp(-fabs(radius - 0.28f) * 42.0f);
        float glyph_cell = floor((angle + VFX_PI) * (24.0f / VFX_TAU));
        float glyph = step(
            0.78f,
            vfx_hash21f((float2)(glyph_cell, floor(time * 0.4f))))
            * native_exp(-fabs(radius - 0.205f) * 90.0f) * 0.6f;
        alpha = (ring1 + ring2 + spokes * 0.65f + ticks * 0.55f + glyph)
            * intensity;
        color_mix = 0.5f + 0.5f * native_sin(angle * 4.0f + radius * 20.0f);
    } else if (background_id == 4u) {
        // preview.html: Nebula smoke
        float noise = vfx_fbm(
            point * 5.2f + (float2)(time * speed * 0.11f, -time * speed * 0.07f));
        float noise2 = vfx_fbm(
            point * 9.0f - (float2)(time * speed * 0.05f, time * speed * 0.09f));
        float cloud = smoothstep(0.32f, 0.82f, noise * 0.72f + noise2 * 0.38f)
            * native_exp(-radius * radius * 3.2f);
        // A broad allocation-space ramp makes the unbounded smoke merge into
        // arbitrary scenes without exposing the square cursor-surface edge.
        // The inner 60% remains untouched; the outer region fades linearly,
        // ending in four fully transparent pixels on each side.
        float edge_distance = min(
            min(uv.x, 1.0f - uv.x),
            min(uv.y, 1.0f - uv.y));
        float edge_fade = vfx_clamp01(
            (edge_distance - 0.015625f) * 5.4237288f);
        alpha = cloud * edge_fade * intensity;
        color_mix = vfx_clamp01(noise2);
    } else if (background_id == 5u) {
        // preview.html: Cyber grid
        float2 grid_point = point * 9.0f;
        grid_point.y += time * speed * 0.55f;
        float2 grid_vector = fabs(vfx_fract2(grid_point) - 0.5f);
        float grid = 1.0f - smoothstep(
            0.035f, 0.09f, min(grid_vector.x, grid_vector.y));
        float radial_fade = 1.0f - smoothstep(0.08f, 0.52f, radius);
        alpha = grid * 0.45f * radial_fade * intensity;
        color_mix = vfx_fract(grid_point.x * 0.08f + grid_point.y * 0.05f);
    } else if (background_id == 6u) {
        // preview.html: Portal vortex
        float swirl = angle + radius * 9.0f - time * speed * 1.6f;
        float bands = vfx_powi(
            0.5f + 0.5f * native_sin(swirl * 6.0f), 9u);
        float mask = (1.0f - smoothstep(0.11f, 0.46f, radius))
            * smoothstep(0.04f, 0.12f, radius);
        alpha = bands * 0.55f * mask * intensity;
        color_mix = 0.5f + 0.5f * native_sin(swirl * 2.0f);
    } else if (background_id == 7u) {
        // preview.html: Speed lines
        float sector = floor((angle + VFX_PI) * (80.0f / VFX_TAU));
        float random = vfx_hash21f((float2)(
            sector, floor(time * speed * 2.0f)));
        float line = vfx_powi(
            0.5f + 0.5f * native_cos(angle * 80.0f),
            34u) * step(0.55f, random);
        float radial = smoothstep(0.08f, 0.18f, radius)
            * (1.0f - smoothstep(0.18f, 0.62f, radius));
        alpha = line * radial * (0.35f + random) * intensity;
        color_mix = random;
    } else if (background_id == 8u) {
        // preview.html: Bokeh field. At scale=1 this is the exact uv*7 demo
        // grid; expressing it through point also makes the Scale control live.
        float2 bokeh_point = point * 7.0f + 3.5f;
        bokeh_point.y -= time * speed * 0.23f;
        float2 cell = floor(bokeh_point);
        float2 cell_uv = vfx_fract2(bokeh_point) - 0.5f;
        float2 offset = (float2)(
            vfx_hash21f(cell) - 0.5f,
            vfx_hash21f(cell + 7.3f) - 0.5f) * 0.55f;
        float size = 0.08f + 0.2f * vfx_hash21f(cell + 13.0f);
        float bubble = 1.0f - smoothstep(
            size, size + 0.04f, length(cell_uv - offset));
        float fade = 0.25f + 0.75f * vfx_hash21f(cell + 2.0f);
        alpha = bubble * fade * intensity * 0.72f;
        color_mix = vfx_hash21f(cell + 19.0f);
    } else if (background_id == 9u) {
        // preview.html: Water ripples
        float wave = vfx_powi(
            0.5f + 0.5f * native_sin(radius * 72.0f - time * speed * 4.0f),
            14u);
        float wave2 = vfx_powi(
            0.5f + 0.5f * native_sin(radius * 42.0f + time * speed * 2.2f),
            18u) * 0.45f;
        float radial = smoothstep(0.06f, 0.14f, radius)
            * (1.0f - smoothstep(0.18f, 0.56f, radius));
        alpha = (wave + wave2) * radial * intensity * 0.8f;
        color_mix = vfx_clamp01(radius * 2.0f);
    } else if (background_id == 10u) {
        // preview.html: Pixel burst
        float angular_cell = floor((angle + VFX_PI) * (32.0f / VFX_TAU));
        float radial_phase = radius * 12.0f - time * speed * 1.8f;
        float radial_cell = floor(radial_phase);
        float random = vfx_hash21f((float2)(angular_cell, radial_cell));
        float phase = vfx_fract(radial_phase);
        float cell = step(0.74f, random)
            * step(0.18f, phase)
            * step(phase, 0.78f);
        float radial = smoothstep(0.08f, 0.16f, radius)
            * (1.0f - smoothstep(0.15f, 0.58f, radius));
        alpha = cell * radial * intensity;
        color_mix = random;
    }

    alpha = clamp(alpha * opacity, 0.0f, 0.96f);
    float3 color = mix(
        vfx_unpack_rgb(control[VFX_CTRL_BG_COLOR_A]),
        vfx_unpack_rgb(control[VFX_CTRL_BG_COLOR_B]),
        vfx_clamp01(color_mix));
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_bgra[y * dst_pitch_pixels + x] = vfx_pack_bgra8_premultiplied(color, alpha);
}
