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

#if defined(TRUEOS_SPIRIT_CPP_REPASS)
// The C++ publication keeps the original nine compositions as its semantic
// base, then gives each one a deliberately small, mode-specific detail layer.
// Templates make those layers compile-time authored without adding a kernel
// argument, runtime table, or C++ runtime dependency.
struct SpiritCppBackgroundLayer {
    float alpha;
    float color_mix;
};

template <uint Mode>
static inline SpiritCppBackgroundLayer vfx_cpp_background_layer(
    float2 uv,
    float2 point,
    float radius,
    float angle,
    float time,
    float speed)
{
    static_assert(Mode >= 2u && Mode <= 11u, "unsupported Spirit background mode");
    const float animated_time = time * speed;
    const float radial_mask = 1.0f - smoothstep(0.34f, 0.57f, radius);
    float detail = 0.0f;
    float color_phase = 0.5f;

    if constexpr (Mode == 2u) {
        // Counter-rotating energy beads and a hairline inner corona.
        const float orbit = native_exp(-fabs(radius - 0.274f) * 210.0f);
        const float beads = vfx_powi(
            0.5f + 0.5f * native_cos(angle * 18.0f + animated_time * 3.1f),
            22u);
        const float corona = native_exp(-fabs(radius - 0.337f) * 260.0f);
        detail = orbit * beads * 0.72f + corona * 0.24f;
        color_phase = 0.5f + 0.5f * native_sin(angle * 5.0f - animated_time * 1.4f);
    } else if constexpr (Mode == 3u) {
        // Two rotating rune belts sharpen the existing magic-circle grammar.
        const float belt = native_exp(-fabs(radius - 0.354f) * 240.0f);
        const float runes = vfx_powi(
            0.5f + 0.5f * native_cos(angle * 36.0f + animated_time * 0.8f),
            30u);
        const float inner_belt = native_exp(-fabs(radius - 0.176f) * 210.0f);
        const float counter = vfx_powi(
            0.5f + 0.5f * native_sin(angle * 20.0f - animated_time * 1.1f),
            24u);
        detail = belt * runes * 0.62f + inner_belt * counter * 0.48f;
        color_phase = 0.5f + 0.5f * native_cos(angle * 6.0f + animated_time * 0.3f);
    } else if constexpr (Mode == 4u) {
        // Sparse star-seed glints sit inside the smoke instead of flattening it.
        const float2 star_cell = floor(point * 42.0f);
        const float star_hash = vfx_hash21f(star_cell);
        const float star = step(0.955f, star_hash)
            * vfx_powi(
                0.5f + 0.5f * native_sin(animated_time * 2.0f + star_hash * VFX_TAU),
                12u);
        const float edge_distance = min(
            min(uv.x, 1.0f - uv.x),
            min(uv.y, 1.0f - uv.y));
        detail = star * radial_mask
            * vfx_clamp01((edge_distance - 0.015625f) * 5.4237288f) * 0.52f;
        color_phase = star_hash;
    } else if constexpr (Mode == 5u) {
        // Pulsing circuit intersections add hierarchy to the moving grid.
        const float2 grid = point * 9.0f + (float2)(0.0f, animated_time * 0.55f);
        const float2 cell = floor(grid);
        const float2 cell_uv = fabs(vfx_fract2(grid) - 0.5f);
        const float node = 1.0f - smoothstep(0.045f, 0.105f, max(cell_uv.x, cell_uv.y));
        const float node_hash = vfx_hash21f(cell);
        const float pulse = vfx_powi(
            0.5f + 0.5f * native_sin(animated_time * 2.4f + node_hash * VFX_TAU),
            8u);
        detail = node * step(0.62f, node_hash) * pulse * radial_mask * 0.68f;
        color_phase = node_hash;
    } else if constexpr (Mode == 6u) {
        // A thin event-horizon rim and travelling spiral sparks deepen the portal.
        const float aperture = native_exp(-fabs(radius - 0.105f) * 250.0f);
        const float spiral = angle * 8.0f + radius * 74.0f - animated_time * 5.2f;
        const float sparks = vfx_powi(0.5f + 0.5f * native_sin(spiral), 28u)
            * (1.0f - smoothstep(0.12f, 0.43f, radius));
        detail = aperture * 0.46f + sparks * 0.36f;
        color_phase = 0.5f + 0.5f * native_sin(spiral * 0.31f);
    } else if constexpr (Mode == 7u) {
        // Bright comet heads travel out along a subset of the speed rays.
        const float sector = floor((angle + VFX_PI) * (80.0f / VFX_TAU));
        const float ray_hash = vfx_hash21f((float2)(sector, 31.0f));
        const float travel = vfx_fract(animated_time * (0.32f + ray_hash * 0.42f) + ray_hash);
        const float head_radius = 0.11f + travel * 0.38f;
        const float head = native_exp(-fabs(radius - head_radius) * 145.0f);
        const float ray = vfx_powi(
            0.5f + 0.5f * native_cos(angle * 80.0f),
            38u);
        detail = head * ray * step(0.52f, ray_hash) * 0.82f;
        color_phase = ray_hash;
    } else if constexpr (Mode == 8u) {
        // A crisp specular pin turns each soft bokeh disc into a light volume.
        const float2 grid = point * 7.0f + 3.5f
            - (float2)(0.0f, animated_time * 0.23f);
        const float2 cell = floor(grid);
        const float2 cell_uv = vfx_fract2(grid) - 0.5f;
        const float2 offset = (float2)(
            vfx_hash21f(cell) - 0.5f,
            vfx_hash21f(cell + 7.3f) - 0.5f) * 0.55f;
        const float glint = 1.0f - smoothstep(
            0.018f,
            0.052f,
            length(cell_uv - offset + (float2)(0.055f, 0.055f)));
        detail = glint * 0.72f;
        color_phase = vfx_hash21f(cell + 19.0f);
    } else if constexpr (Mode == 9u) {
        // Angular caustic breaks keep the concentric ripples from reading flat.
        const float caustic = vfx_powi(
            0.5f + 0.5f * native_sin(
                radius * 96.0f - animated_time * 5.1f
                    + native_sin(angle * 7.0f) * 2.1f),
            24u);
        const float band = smoothstep(0.08f, 0.15f, radius)
            * (1.0f - smoothstep(0.20f, 0.55f, radius));
        detail = caustic * band * 0.54f;
        color_phase = 0.5f + 0.5f * native_cos(angle * 3.0f - animated_time);
    } else if constexpr (Mode == 10u) {
        // Smaller counter-phase chips enrich the pixel burst.
        const float angular_cell = floor((angle + VFX_PI) * (48.0f / VFX_TAU));
        const float radial_phase = radius * 17.0f + animated_time * 1.25f;
        const float radial_cell = floor(radial_phase);
        const float chip_hash = vfx_hash21f((float2)(angular_cell, radial_cell));
        const float phase = vfx_fract(radial_phase);
        const float chip = step(0.82f, chip_hash)
            * step(0.28f, phase) * step(phase, 0.62f);
        detail = chip * smoothstep(0.10f, 0.17f, radius)
            * (1.0f - smoothstep(0.20f, 0.55f, radius)) * 0.72f;
        color_phase = chip_hash;
    } else {
        // Mode 11: MagicTimeCircle. The same segmented circle becomes a
        // clock face without introducing hands that would cross Lilly. UTC
        // seconds-of-day arrive through the existing exact-f32 time dword.
        const float whole_second = floor(clamp(time, 0.0f, 86399.0f));
        const float hour_index = floor(whole_second * (1.0f / 3600.0f));
        const float minute_index = floor(whole_second * (1.0f / 60.0f))
            - hour_index * 60.0f;
        const float second_index = whole_second
            - floor(whole_second * (1.0f / 60.0f)) * 60.0f;
        // Zero is twelve o'clock and positive turns advance clockwise in the
        // cursor's top-left-origin coordinate system.
        const float clock_turn = vfx_fract(
            (angle + 0.5f * VFX_PI) * (1.0f / VFX_TAU) + 1.0f);
        const float hour_turn = vfx_fract(hour_index * (1.0f / 12.0f));
        const float minute_turn = minute_index * (1.0f / 60.0f);
        const float second_turn = second_index * (1.0f / 60.0f);
        const float hour_delta = fabs(
            vfx_fract(clock_turn - hour_turn + 0.5f) - 0.5f);
        const float minute_delta = fabs(
            vfx_fract(clock_turn - minute_turn + 0.5f) - 0.5f);
        const float second_delta = fabs(
            vfx_fract(clock_turn - second_turn + 0.5f) - 0.5f);

        // Large inner HH, smaller middle MM, and a thin outer seconds segment.
        // Each selector is quantized before any pixel math, so the outer mark
        // advances once per wall-clock second instead of sweeping at 60 Hz.
        const float hour_segment =
            (1.0f - smoothstep(0.020f, 0.032f, hour_delta))
            * (1.0f - smoothstep(0.028f, 0.041f, fabs(radius - 0.205f)));
        const float minute_segment =
            (1.0f - smoothstep(0.007f, 0.014f, minute_delta))
            * (1.0f - smoothstep(0.017f, 0.027f, fabs(radius - 0.282f)));
        const float second_segment =
            (1.0f - smoothstep(0.0035f, 0.0075f, second_delta))
            * (1.0f - smoothstep(0.011f, 0.019f, fabs(radius - 0.365f)));

        detail = hour_segment * 0.78f
            + minute_segment * 0.86f
            + second_segment;
        const float indicator_sum =
            hour_segment + minute_segment + second_segment;
        color_phase = indicator_sum > 0.0f
            ? vfx_clamp01(
                (hour_segment * 0.15f
                    + minute_segment * 0.62f
                    + second_segment)
                / indicator_sum)
            : 0.5f;
    }
    return SpiritCppBackgroundLayer {
        clamp(detail, 0.0f, 0.9f),
        vfx_clamp01(color_phase),
    };
}

static inline SpiritCppBackgroundLayer vfx_cpp_background_dispatch(
    uint mode,
    float2 uv,
    float2 point,
    float radius,
    float angle,
    float time,
    float speed)
{
    switch (mode) {
        case 2u: return vfx_cpp_background_layer<2u>(uv, point, radius, angle, time, speed);
        case 3u: return vfx_cpp_background_layer<3u>(uv, point, radius, angle, time, speed);
        case 4u: return vfx_cpp_background_layer<4u>(uv, point, radius, angle, time, speed);
        case 5u: return vfx_cpp_background_layer<5u>(uv, point, radius, angle, time, speed);
        case 6u: return vfx_cpp_background_layer<6u>(uv, point, radius, angle, time, speed);
        case 7u: return vfx_cpp_background_layer<7u>(uv, point, radius, angle, time, speed);
        case 8u: return vfx_cpp_background_layer<8u>(uv, point, radius, angle, time, speed);
        case 9u: return vfx_cpp_background_layer<9u>(uv, point, radius, angle, time, speed);
        case 10u: return vfx_cpp_background_layer<10u>(uv, point, radius, angle, time, speed);
        case 11u: return vfx_cpp_background_layer<11u>(uv, point, radius, angle, time, speed);
        default: return SpiritCppBackgroundLayer { 0.0f, 0.5f };
    }
}
#endif

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
        float animated_time = time * speed;
        float ring1 = native_exp(-fabs(radius - 0.32f) * 130.0f);
        float ring2 = native_exp(-fabs(radius - 0.24f) * 160.0f) * 0.7f;
        float spokes = vfx_powi(
            0.5f + 0.5f * native_cos(angle * 12.0f), 28u)
            * smoothstep(0.12f, 0.18f, radius)
            * (1.0f - smoothstep(0.30f, 0.37f, radius));
        float ticks = vfx_powi(
            0.5f + 0.5f * native_cos(angle * 48.0f - animated_time * 0.35f),
            36u)
            * native_exp(-fabs(radius - 0.28f) * 42.0f);
        float glyph_cell = floor((angle + VFX_PI) * (24.0f / VFX_TAU));
        float glyph = step(
            0.78f,
            vfx_hash21f((float2)(glyph_cell, floor(animated_time * 0.4f))))
            * native_exp(-fabs(radius - 0.205f) * 90.0f) * 0.6f;
        alpha = (ring1 + ring2 + spokes * 0.65f + ticks * 0.55f + glyph)
            * intensity;
        color_mix = 0.5f + 0.5f * native_sin(angle * 4.0f + radius * 20.0f);
    } else if (background_id == 11u) {
        // C++ MagicTimeCircle base: preserve Magic circle's rings and radial
        // grammar, but replace its freely rotating 48-tick belt with a stable
        // twelve-hour / sixty-minute clock face. The selected HH/MM/SS
        // segments are added by the C++-specialized layer above.
        float ring1 = native_exp(-fabs(radius - 0.32f) * 130.0f);
        float ring2 = native_exp(-fabs(radius - 0.24f) * 160.0f) * 0.7f;
        float hour_ticks = vfx_powi(
            0.5f + 0.5f * native_cos((angle + 0.5f * VFX_PI) * 12.0f),
            34u)
            * native_exp(-fabs(radius - 0.235f) * 52.0f);
        float minute_ticks = vfx_powi(
            0.5f + 0.5f * native_cos((angle + 0.5f * VFX_PI) * 60.0f),
            52u)
            * native_exp(-fabs(radius - 0.325f) * 68.0f);
        float spokes = vfx_powi(
            0.5f + 0.5f * native_cos((angle + 0.5f * VFX_PI) * 12.0f),
            28u)
            * smoothstep(0.12f, 0.18f, radius)
            * (1.0f - smoothstep(0.30f, 0.37f, radius));
        alpha = (ring1 + ring2 + spokes * 0.42f
                + hour_ticks * 0.52f + minute_ticks * 0.38f)
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

#if defined(TRUEOS_SPIRIT_CPP_REPASS)
    const SpiritCppBackgroundLayer cpp_layer = vfx_cpp_background_dispatch(
        background_id,
        uv,
        point,
        radius,
        angle,
        time,
        speed);
    alpha += cpp_layer.alpha * intensity;
    color_mix = mix(
        color_mix,
        cpp_layer.color_mix,
        vfx_clamp01(cpp_layer.alpha * 0.72f));
#endif

    alpha = clamp(alpha * opacity, 0.0f, 0.96f);
    float3 color = mix(
        vfx_unpack_rgb(control[VFX_CTRL_BG_COLOR_A]),
        vfx_unpack_rgb(control[VFX_CTRL_BG_COLOR_B]),
        vfx_clamp01(color_mix));
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_bgra[y * dst_pitch_pixels + x] = vfx_pack_bgra8_premultiplied(color, alpha);
}
