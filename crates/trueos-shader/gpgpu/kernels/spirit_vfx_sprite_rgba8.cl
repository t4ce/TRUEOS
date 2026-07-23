// Spirit VFX resident-sprite pass for Intel Xe-LP / ADL-S.
//
// A Lilly RGBA8 frame is transformed and source-over composited onto the
// procedural background. The bounded artifact implements the complete
// preview.html Sprite shader range under stable IDs 0 through 15.

#define SPIRIT_VFX_SIZE 256u
#define SPIRIT_VFX_CONTROL_MAGIC 0x53564658u // "SVFX"
#define SPIRIT_VFX_CONTROL_VERSION 1u
#define VFX_TAU 6.28318530718f

#define VFX_CTRL_MAGIC              0u
#define VFX_CTRL_VERSION            1u
#define VFX_CTRL_TIME_F32           4u
#define VFX_CTRL_BACKGROUND_ID      3u
#define VFX_CTRL_SCALE_F32         13u
#define VFX_CTRL_ROTATION_F32      14u
#define VFX_CTRL_ALPHA_CUTOFF_F32  15u
#define VFX_CTRL_SAMPLING          16u
#define VFX_CTRL_SHADER_ID         17u
#define VFX_CTRL_SHADER_P0_F32     18u
#define VFX_CTRL_FX_COLOR_A        22u
#define VFX_CTRL_FX_COLOR_B        23u
#define VFX_CTRL_SRC_WIDTH         24u
#define VFX_CTRL_SRC_HEIGHT        25u
#define VFX_CTRL_SRC_PITCH         26u
#define VFX_CTRL_DST_PITCH         27u
#define VFX_CTRL_EDGE_FADE_F32     31u

static inline float vfx_finite_or(float value, float fallback)
{
    return isfinite(value) ? value : fallback;
}

static inline float vfx_clamp01(float value)
{
    return clamp(value, 0.0f, 1.0f);
}

// Fixed integer powers avoid an IGC helper surface and preserve this kernel's
// three-BTI ABI.
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

static inline float3 vfx_fract3(float3 value)
{
    return value - floor(value);
}

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

static inline float3 vfx_hsv2rgb(float3 hsv)
{
    float3 phase = vfx_fract3((float3)(
        hsv.x,
        hsv.x + 2.0f / 3.0f,
        hsv.x + 1.0f / 3.0f));
    float3 wave = fabs(phase * 6.0f - 3.0f);
    return hsv.z * mix((float3)(1.0f), clamp(wave - 1.0f, 0.0f, 1.0f), hsv.y);
}

static inline float3 vfx_unpack_rgb(uint packed)
{
    return (float3)(
        (float)(packed & 0xFFu),
        (float)((packed >> 8) & 0xFFu),
        (float)((packed >> 16) & 0xFFu)) * (1.0f / 255.0f);
}

// Resident PNG data is straight RGBA8, represented as AABBGGRR dwords.
static inline float4 vfx_unpack_rgba8(uint packed)
{
    return (float4)(
        (float)(packed & 0xFFu),
        (float)((packed >> 8) & 0xFFu),
        (float)((packed >> 16) & 0xFFu),
        (float)(packed >> 24)) * (1.0f / 255.0f);
}

// Cursor storage is premultiplied BGRA8 / AARRGGBB dwords.
static inline float4 vfx_unpack_bgra8_premultiplied(uint packed)
{
    return (float4)(
        (float)((packed >> 16) & 0xFFu),
        (float)((packed >> 8) & 0xFFu),
        (float)(packed & 0xFFu),
        (float)(packed >> 24)) * (1.0f / 255.0f);
}

static inline uint vfx_pack_bgra8_premultiplied(float4 color)
{
    color = clamp(color, (float4)(0.0f), (float4)(1.0f));
    uint b = (uint)(color.z * 255.0f + 0.5f);
    uint g = (uint)(color.y * 255.0f + 0.5f);
    uint r = (uint)(color.x * 255.0f + 0.5f);
    uint a = (uint)(color.w * 255.0f + 0.5f);
    return (a << 24) | (r << 16) | (g << 8) | b;
}

static inline float4 vfx_over(float4 bottom, float4 top)
{
    return top + bottom * (1.0f - top.w);
}

static inline float4 vfx_premultiply(float4 straight)
{
    float alpha = vfx_clamp01(straight.w);
    float3 rgb = clamp(straight.xyz, (float3)(0.0f), (float3)(1.0f));
    return (float4)(rgb * alpha, alpha);
}

static inline float4 vfx_layer(float3 rgb, float alpha)
{
    alpha = vfx_clamp01(alpha);
    rgb = clamp(rgb, (float3)(0.0f), (float3)(1.0f));
    return (float4)(rgb * alpha, alpha);
}

static inline float4 vfx_load_rgba(
    __global const uint *src_rgba,
    uint width,
    uint height,
    uint pitch_pixels,
    int x,
    int y)
{
    x = clamp(x, 0, (int)width - 1);
    y = clamp(y, 0, (int)height - 1);
    return vfx_unpack_rgba8(src_rgba[(uint)y * pitch_pixels + (uint)x]);
}

static inline float4 vfx_sample_sprite(
    __global const uint *src_rgba,
    __global const uint *control,
    float2 local_uv)
{
    if (local_uv.x < 0.0f || local_uv.y < 0.0f || local_uv.x > 1.0f || local_uv.y > 1.0f) {
        return (float4)(0.0f);
    }
    uint width = control[VFX_CTRL_SRC_WIDTH];
    uint height = control[VFX_CTRL_SRC_HEIGHT];
    uint pitch_pixels = control[VFX_CTRL_SRC_PITCH] >> 2;
    // Match preview.html's local sprite convention.
    float2 pixel = (float2)(
        local_uv.x * (float)(width - 1u),
        (1.0f - local_uv.y) * (float)(height - 1u));
    float4 sampled;
    if (control[VFX_CTRL_SAMPLING] == 0u) {
        sampled = vfx_load_rgba(
            src_rgba,
            width,
            height,
            pitch_pixels,
            (int)floor(pixel.x + 0.5f),
            (int)floor(pixel.y + 0.5f));
    } else {
        float2 base_f = floor(pixel);
        int2 base = (int2)((int)base_f.x, (int)base_f.y);
        float2 fraction = pixel - base_f;
        float4 top = mix(
            vfx_load_rgba(src_rgba, width, height, pitch_pixels, base.x, base.y),
            vfx_load_rgba(src_rgba, width, height, pitch_pixels, base.x + 1, base.y),
            fraction.x);
        float4 bottom = mix(
            vfx_load_rgba(src_rgba, width, height, pitch_pixels, base.x, base.y + 1),
            vfx_load_rgba(src_rgba, width, height, pitch_pixels, base.x + 1, base.y + 1),
            fraction.x);
        sampled = mix(top, bottom, fraction.y);
    }
    float cutoff = clamp(
        vfx_finite_or(as_float(control[VFX_CTRL_ALPHA_CUTOFF_F32]), 0.02f),
        0.0f,
        0.3f);
    sampled.w = smoothstep(max(0.0f, cutoff - 0.015f), min(1.0f, cutoff + 0.025f), sampled.w);
    return sampled;
}

static inline float vfx_blur_alpha(
    __global const uint *src_rgba,
    __global const uint *control,
    float2 local_uv,
    float radius_pixels)
{
    float2 texel = (float2)(
        1.0f / max(1.0f, (float)control[VFX_CTRL_SRC_WIDTH]),
        1.0f / max(1.0f, (float)control[VFX_CTRL_SRC_HEIGHT]));
    float2 delta = texel * radius_pixels;
    float sum = vfx_sample_sprite(src_rgba, control, local_uv).w * 1.6f;
    sum += vfx_sample_sprite(src_rgba, control, local_uv + (float2)(delta.x, 0.0f)).w;
    sum += vfx_sample_sprite(src_rgba, control, local_uv - (float2)(delta.x, 0.0f)).w;
    sum += vfx_sample_sprite(src_rgba, control, local_uv + (float2)(0.0f, delta.y)).w;
    sum += vfx_sample_sprite(src_rgba, control, local_uv - (float2)(0.0f, delta.y)).w;
    float2 diagonal = delta * 0.707f;
    sum += vfx_sample_sprite(src_rgba, control, local_uv + diagonal).w;
    sum += vfx_sample_sprite(src_rgba, control, local_uv - diagonal).w;
    sum += vfx_sample_sprite(src_rgba, control, local_uv + (float2)(diagonal.x, -diagonal.y)).w;
    sum += vfx_sample_sprite(src_rgba, control, local_uv + (float2)(-diagonal.x, diagonal.y)).w;
    return sum * (1.0f / 9.6f);
}

static inline float vfx_dilate_alpha(
    __global const uint *src_rgba,
    __global const uint *control,
    float2 local_uv,
    float radius_pixels)
{
    float2 texel = (float2)(
        1.0f / max(1.0f, (float)control[VFX_CTRL_SRC_WIDTH]),
        1.0f / max(1.0f, (float)control[VFX_CTRL_SRC_HEIGHT]));
    float2 delta = texel * radius_pixels;
    float result = vfx_sample_sprite(src_rgba, control, local_uv).w;
    result = max(result, vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(delta.x, 0.0f)).w);
    result = max(result, vfx_sample_sprite(
        src_rgba, control, local_uv - (float2)(delta.x, 0.0f)).w);
    result = max(result, vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(0.0f, delta.y)).w);
    result = max(result, vfx_sample_sprite(
        src_rgba, control, local_uv - (float2)(0.0f, delta.y)).w);
    float2 diagonal = delta * 0.707f;
    result = max(result, vfx_sample_sprite(src_rgba, control, local_uv + diagonal).w);
    result = max(result, vfx_sample_sprite(src_rgba, control, local_uv - diagonal).w);
    result = max(result, vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(diagonal.x, -diagonal.y)).w);
    result = max(result, vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(-diagonal.x, diagonal.y)).w);
    return result;
}

static inline float vfx_erode_alpha(
    __global const uint *src_rgba,
    __global const uint *control,
    float2 local_uv,
    float radius_pixels)
{
    float2 texel = (float2)(
        1.0f / max(1.0f, (float)control[VFX_CTRL_SRC_WIDTH]),
        1.0f / max(1.0f, (float)control[VFX_CTRL_SRC_HEIGHT]));
    float2 delta = texel * radius_pixels;
    float result = vfx_sample_sprite(src_rgba, control, local_uv).w;
    result = min(result, vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(delta.x, 0.0f)).w);
    result = min(result, vfx_sample_sprite(
        src_rgba, control, local_uv - (float2)(delta.x, 0.0f)).w);
    result = min(result, vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(0.0f, delta.y)).w);
    result = min(result, vfx_sample_sprite(
        src_rgba, control, local_uv - (float2)(0.0f, delta.y)).w);
    float2 diagonal = delta * 0.707f;
    result = min(result, vfx_sample_sprite(src_rgba, control, local_uv + diagonal).w);
    result = min(result, vfx_sample_sprite(src_rgba, control, local_uv - diagonal).w);
    result = min(result, vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(diagonal.x, -diagonal.y)).w);
    result = min(result, vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(-diagonal.x, diagonal.y)).w);
    return result;
}

#if defined(TRUEOS_SPIRIT_CPP_REPASS)
// A restrained couture pass shared by all fifteen authored Sprite effects.
// The original effect remains the large-form composition. C++ templates add a
// mode-specific animated filament, edge chroma, and sparse micro-highlight so
// every selection gains definition at cursor scale without changing its four
// controls or the clean mode.
struct SpiritCppSpriteEdges {
    float outer;
    float inner;
};

static inline SpiritCppSpriteEdges vfx_cpp_sprite_edges(
    __global const uint *src_rgba,
    __global const uint *control,
    float2 local_uv,
    float2 texel,
    float base_alpha)
{
    const float radius = 1.35f;
    const float left = vfx_sample_sprite(
        src_rgba, control, local_uv - (float2)(texel.x * radius, 0.0f)).w;
    const float right = vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(texel.x * radius, 0.0f)).w;
    const float top = vfx_sample_sprite(
        src_rgba, control, local_uv + (float2)(0.0f, texel.y * radius)).w;
    const float bottom = vfx_sample_sprite(
        src_rgba, control, local_uv - (float2)(0.0f, texel.y * radius)).w;
    const float neighbour_max = max(max(left, right), max(top, bottom));
    const float neighbour_min = min(min(left, right), min(top, bottom));
    return SpiritCppSpriteEdges {
        max(0.0f, neighbour_max - base_alpha),
        max(0.0f, base_alpha - neighbour_min),
    };
}

template <uint Mode>
static inline float4 vfx_cpp_sprite_layer(
    SpiritCppSpriteEdges edges,
    float2 local_uv,
    float time,
    float3 color_a,
    float3 color_b)
{
    static_assert(Mode >= 1u && Mode <= 15u, "unsupported Spirit sprite mode");
    const float2 centered = local_uv - 0.5f;
    const float angle = atan2(centered.y, centered.x);
    const float radius = native_sqrt(max(dot(centered, centered), 1.0e-8f));

    float frequency = 7.0f;
    float velocity = 1.0f;
    float sharpness = 18.0f;
    float outer_strength = 0.28f;
    float inner_strength = 0.12f;
    float color_phase = 0.5f;
    float micro = 0.0f;

    if constexpr (Mode == 1u || Mode == 15u) {
        // Aura and dream: slow pearlescent contour with breathing motes.
        frequency = Mode == 1u ? 6.0f : 5.0f;
        velocity = Mode == 1u ? 0.72f : -0.48f;
        sharpness = 20.0f;
        outer_strength = 0.34f;
        inner_strength = 0.10f;
        const float cell_hash = vfx_hash21f(
            floor(local_uv * 48.0f) + (float2)((float)Mode, 9.0f));
        micro = step(0.965f, cell_hash)
            * vfx_powi(
                0.5f + 0.5f * native_sin(time * 1.7f + cell_hash * VFX_TAU),
                12u)
            * (1.0f - smoothstep(0.36f, 0.72f, radius)) * 0.12f;
    } else if constexpr (Mode == 2u || Mode == 4u || Mode == 9u) {
        // Neon, ice, electric: a fast double-frequency energy filament.
        frequency = Mode == 9u ? 17.0f : (Mode == 4u ? 12.0f : 9.0f);
        velocity = Mode == 4u ? -1.25f : 2.15f;
        sharpness = Mode == 9u ? 30.0f : 24.0f;
        outer_strength = Mode == 9u ? 0.48f : 0.39f;
        inner_strength = 0.16f;
        const float fork = vfx_powi(
            0.5f + 0.5f * native_sin(
                (local_uv.x - local_uv.y) * frequency * VFX_TAU
                    - time * velocity * 0.77f),
            24u);
        micro = edges.outer * fork * 0.22f;
    } else if constexpr (Mode == 3u || Mode == 7u || Mode == 11u) {
        // Fire, dissolve, impact: hotter, asymmetric travelling sparks.
        frequency = Mode == 3u ? 11.0f : 14.0f;
        velocity = Mode == 11u ? 3.4f : 1.8f;
        sharpness = 26.0f;
        outer_strength = 0.42f;
        inner_strength = 0.14f;
        const float2 spark_cell = floor(
            (local_uv + (float2)(0.0f, time * 0.045f * velocity)) * 72.0f);
        const float spark_hash = vfx_hash21f(
            spark_cell + (float2)((float)Mode * 3.0f, 17.0f));
        micro = step(0.974f, spark_hash)
            * smoothstep(0.08f, 0.44f, radius)
            * (1.0f - smoothstep(0.44f, 0.72f, radius)) * 0.18f;
    } else if constexpr (Mode == 5u || Mode == 6u || Mode == 12u) {
        // Hologram, RGB glitch, pixel wave: quantized signal highlights.
        frequency = Mode == 5u ? 13.0f : 19.0f;
        velocity = Mode == 6u ? 3.1f : 1.45f;
        sharpness = 32.0f;
        outer_strength = 0.30f;
        inner_strength = 0.18f;
        const float scan = vfx_fract(
            local_uv.y * (Mode == 5u ? 96.0f : 64.0f) - time * velocity);
        const float packet = step(0.90f, scan)
            * step(0.68f, vfx_hash21f((float2)(
                floor(local_uv.y * 64.0f),
                floor(time * velocity * 7.0f) + (float)Mode)));
        micro = packet * (edges.outer + edges.inner * 0.45f) * 0.28f;
    } else if constexpr (Mode == 8u || Mode == 10u || Mode == 14u) {
        // Ghost, prism, liquid: broad counter-rotating spectral arcs.
        frequency = Mode == 10u ? 8.0f : 6.0f;
        velocity = Mode == 8u ? -0.82f : 0.92f;
        sharpness = 16.0f;
        outer_strength = 0.35f;
        inner_strength = 0.13f;
        const float spectral = 0.5f + 0.5f * native_cos(
            angle * 3.0f - radius * 18.0f + time * velocity);
        color_phase = spectral;
        micro = edges.outer * vfx_powi(spectral, 14u) * 0.16f;
    } else {
        // Toon ink: a sparse cel-animation highlight that respects the ink rim.
        static_assert(Mode == 13u, "unhandled Spirit sprite mode");
        frequency = 5.0f;
        velocity = 0.38f;
        sharpness = 28.0f;
        outer_strength = 0.20f;
        inner_strength = 0.10f;
        const float hatch = vfx_powi(
            0.5f + 0.5f * native_sin(
                (local_uv.x + local_uv.y) * 18.0f * VFX_TAU - time),
            30u);
        micro = edges.inner * hatch * 0.10f;
    }

    const float phase = 0.5f + 0.5f * native_sin(
        angle * frequency
            + (local_uv.y - local_uv.x) * frequency * 1.7f
            + time * velocity);
    const float filament = native_exp(
        native_log(max(phase, 1.0e-5f)) * sharpness);
    if constexpr (!(Mode == 8u || Mode == 10u || Mode == 14u)) {
        color_phase = 0.5f + 0.5f * native_sin(
            angle * 2.0f + time * velocity * 0.31f + (float)Mode);
    }

    const float alpha = clamp(
        edges.outer * (0.035f + filament * outer_strength)
            + edges.inner * filament * inner_strength
            + micro,
        0.0f,
        0.58f);
    const float3 accent = mix(color_a, color_b, vfx_clamp01(color_phase));
    return vfx_layer(accent, alpha);
}

static inline float4 vfx_cpp_sprite_dispatch(
    uint mode,
    SpiritCppSpriteEdges edges,
    float2 local_uv,
    float time,
    float3 color_a,
    float3 color_b)
{
    switch (mode) {
        case 1u: return vfx_cpp_sprite_layer<1u>(edges, local_uv, time, color_a, color_b);
        case 2u: return vfx_cpp_sprite_layer<2u>(edges, local_uv, time, color_a, color_b);
        case 3u: return vfx_cpp_sprite_layer<3u>(edges, local_uv, time, color_a, color_b);
        case 4u: return vfx_cpp_sprite_layer<4u>(edges, local_uv, time, color_a, color_b);
        case 5u: return vfx_cpp_sprite_layer<5u>(edges, local_uv, time, color_a, color_b);
        case 6u: return vfx_cpp_sprite_layer<6u>(edges, local_uv, time, color_a, color_b);
        case 7u: return vfx_cpp_sprite_layer<7u>(edges, local_uv, time, color_a, color_b);
        case 8u: return vfx_cpp_sprite_layer<8u>(edges, local_uv, time, color_a, color_b);
        case 9u: return vfx_cpp_sprite_layer<9u>(edges, local_uv, time, color_a, color_b);
        case 10u: return vfx_cpp_sprite_layer<10u>(edges, local_uv, time, color_a, color_b);
        case 11u: return vfx_cpp_sprite_layer<11u>(edges, local_uv, time, color_a, color_b);
        case 12u: return vfx_cpp_sprite_layer<12u>(edges, local_uv, time, color_a, color_b);
        case 13u: return vfx_cpp_sprite_layer<13u>(edges, local_uv, time, color_a, color_b);
        case 14u: return vfx_cpp_sprite_layer<14u>(edges, local_uv, time, color_a, color_b);
        case 15u: return vfx_cpp_sprite_layer<15u>(edges, local_uv, time, color_a, color_b);
        default: return (float4)(0.0f);
    }
}
#endif

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void spirit_vfx_sprite_rgba8(
    __global const uint *src_rgba,
    __global const uint *control,
    __global uint *dst_bgra)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);
    if (x >= SPIRIT_VFX_SIZE || y >= SPIRIT_VFX_SIZE
        || control[VFX_CTRL_MAGIC] != SPIRIT_VFX_CONTROL_MAGIC
        || control[VFX_CTRL_VERSION] != SPIRIT_VFX_CONTROL_VERSION
        || control[VFX_CTRL_SRC_WIDTH] == 0u
        || control[VFX_CTRL_SRC_HEIGHT] == 0u
        || control[VFX_CTRL_SRC_PITCH] < control[VFX_CTRL_SRC_WIDTH] * 4u
        || control[VFX_CTRL_DST_PITCH] < SPIRIT_VFX_SIZE * 4u) {
        return;
    }

    float2 uv = ((float2)((float)x + 0.5f, (float)y + 0.5f)) * (1.0f / 256.0f);
    float2 point = uv - 0.5f - (float2)(
        clamp(vfx_finite_or(as_float(control[11]), 0.0f), -0.35f, 0.35f),
        clamp(vfx_finite_or(as_float(control[12]), 0.0f), -0.35f, 0.35f));
    float rotation = clamp(
        vfx_finite_or(as_float(control[VFX_CTRL_ROTATION_F32]), 3.14159265359f),
        -6.28318530718f,
        6.28318530718f);
    float cosine = native_cos(-rotation);
    float sine = native_sin(-rotation);
    point = (float2)(
        cosine * point.x - sine * point.y,
        sine * point.x + cosine * point.y);
    float scale = clamp(
        vfx_finite_or(as_float(control[VFX_CTRL_SCALE_F32]), 0.5f),
        0.35f,
        1.55f);
    float2 local_uv = point / max(0.001f, scale) + 0.5f;

    float2 texel = (float2)(
        1.0f / max(1.0f, (float)control[VFX_CTRL_SRC_WIDTH]),
        1.0f / max(1.0f, (float)control[VFX_CTRL_SRC_HEIGHT]));
    float time = vfx_finite_or(as_float(control[VFX_CTRL_TIME_F32]), 0.0f);
    float p0 = vfx_finite_or(as_float(control[VFX_CTRL_SHADER_P0_F32]), 0.0f);
    float p1 = vfx_finite_or(as_float(control[VFX_CTRL_SHADER_P0_F32 + 1u]), 0.0f);
    float p2 = vfx_finite_or(as_float(control[VFX_CTRL_SHADER_P0_F32 + 2u]), 0.0f);
    float p3 = vfx_finite_or(as_float(control[VFX_CTRL_SHADER_P0_F32 + 3u]), 0.0f);
    float3 color_a = vfx_unpack_rgb(control[VFX_CTRL_FX_COLOR_A]);
    float3 color_b = vfx_unpack_rgb(control[VFX_CTRL_FX_COLOR_B]);
    uint requested_shader_id = control[VFX_CTRL_SHADER_ID];
    uint shader_id = requested_shader_id <= 15u ? requested_shader_id : 0u;

    float4 base = vfx_sample_sprite(src_rgba, control, local_uv);
    float4 sprite = vfx_premultiply(base);
    if (shader_id == 1u) {
        // preview.html: Aura bloom
        float radius = clamp(p0, 2.0f, 30.0f);
        float strength = clamp(p1, 0.0f, 2.5f);
        float pulse_rate = clamp(p2, 0.0f, 4.0f);
        float brighten = clamp(p3, 0.0f, 1.0f);
        float pulse = 0.78f + 0.22f * native_sin(time * VFX_TAU * 0.35f * pulse_rate);
        float blurred = vfx_blur_alpha(src_rgba, control, local_uv, radius * 0.45f) * 0.62f
            + vfx_blur_alpha(src_rgba, control, local_uv, radius) * 0.38f;
        float edge = max(0.0f, blurred - base.w * 0.25f);
        float flow = 0.5f + 0.5f * native_sin(
            (local_uv.y * 2.4f + local_uv.x) * VFX_TAU + time);
        float3 glow_color = mix(color_a, color_b, flow);
        float glow_alpha = clamp(edge * strength * pulse, 0.0f, 0.92f);
        float4 glow = vfx_layer(glow_color, glow_alpha);
        float3 brightened = mix(base.xyz, 1.0f - (1.0f - base.xyz) * 0.7f, brighten);
        sprite = vfx_over(glow, vfx_premultiply((float4)(brightened, base.w)));
    } else if (shader_id == 2u) {
        // preview.html: Neon edge
        float width = clamp(p0, 0.5f, 12.0f);
        float intensity = clamp(p1, 0.0f, 2.5f);
        float flow_speed = clamp(p2, 0.0f, 4.0f);
        float fill_tint = clamp(p3, 0.0f, 1.0f);
        float outer = max(
            0.0f,
            vfx_dilate_alpha(src_rgba, control, local_uv, width) - base.w);
        float inner = max(
            0.0f,
            base.w - vfx_erode_alpha(
                src_rgba, control, local_uv, max(0.5f, width * 0.45f)));
        float flow = 0.5f + 0.5f * native_sin(
            (local_uv.y * 2.0f - local_uv.x) * VFX_TAU
                + time * flow_speed * 2.2f);
        float3 rim = mix(color_a, color_b, flow);
        float glow = max(
            0.0f,
            vfx_blur_alpha(src_rgba, control, local_uv, width * 2.2f) - base.w)
            * 0.38f;
        float4 under = vfx_layer(rim, (outer + glow) * intensity);
        float4 body = base;
        body.xyz = mix(
            body.xyz,
            body.xyz * mix((float3)(1.0f), rim, 0.45f) * 1.18f,
            fill_tint);
        body.xyz += rim * inner * 0.18f * intensity;
        sprite = vfx_over(under, vfx_premultiply(body));
    } else if (shader_id == 3u) {
        // preview.html: Fire rim
        float rim_width = clamp(p0, 1.0f, 12.0f);
        float flame_height = clamp(p1, 2.0f, 34.0f);
        float turbulence = clamp(p2, 0.0f, 4.0f);
        float heat = clamp(p3, 0.0f, 2.5f);
        float noise = vfx_fbm((float2)(
            local_uv.x * 8.0f + time * turbulence * 0.35f,
            local_uv.y * 12.0f - time * turbulence));
        float wobble = (noise - 0.5f) * texel.x * flame_height * 0.32f;
        float lift = texel.y * flame_height * (0.35f + 0.8f * noise);
        float source = vfx_sample_sprite(
            src_rgba, control, local_uv - (float2)(wobble, lift)).w;
        float outer = max(
            0.0f,
            vfx_dilate_alpha(src_rgba, control, local_uv, rim_width) - base.w);
        float flame = max(0.0f, source - base.w) * smoothstep(0.32f, 0.82f, noise);
        float hot = vfx_clamp01(outer + flame * 1.35f);
        float3 fire_color = mix(color_a, color_b, vfx_clamp01(noise * 1.15f));
        float soft = max(
            0.0f,
            vfx_blur_alpha(src_rgba, control, local_uv, rim_width * 2.2f) - base.w)
            * 0.3f;
        float4 fire = vfx_layer(fire_color, (hot + soft) * heat);
        float4 body = base;
        body.xyz = mix(
            body.xyz,
            body.xyz * (float3)(1.22f, 0.88f, 0.72f),
            0.2f * min(1.0f, heat));
        sprite = vfx_over(fire, vfx_premultiply(body));
    } else if (shader_id == 4u) {
        // preview.html: Ice shimmer
        float edge_width = clamp(p0, 0.5f, 12.0f);
        float shimmer = clamp(p1, 0.0f, 3.0f);
        float crystal_scale = clamp(p2, 2.0f, 24.0f);
        float tint = clamp(p3, 0.0f, 1.0f);
        float outer = max(
            0.0f,
            vfx_dilate_alpha(src_rgba, control, local_uv, edge_width) - base.w);
        float inner = max(
            0.0f,
            base.w - vfx_erode_alpha(
                src_rgba, control, local_uv, max(0.5f, edge_width * 0.5f)));
        float diagonal = 0.5f + 0.5f * native_sin(
            (local_uv.x + local_uv.y) * crystal_scale * VFX_TAU
                + time * shimmer * 1.6f);
        float crystal = vfx_powi(diagonal, 10u);
        float cross = vfx_powi(
            0.5f + 0.5f * native_sin(
                (local_uv.x - local_uv.y) * crystal_scale * 1.7f * VFX_TAU
                    - time * shimmer),
            14u);
        float3 ice = mix(color_a, color_b, vfx_clamp01(crystal + cross * 0.5f));
        float4 rim = vfx_layer(
            ice,
            outer * 1.2f + inner * (0.24f + 0.5f * crystal));
        float4 body = base;
        body.xyz = mix(body.xyz, mix(body.xyz, ice, 0.35f), tint);
        sprite = vfx_over(rim, vfx_premultiply(body));
    } else if (shader_id == 5u) {
        // preview.html: Hologram
        float scan_density = clamp(p0, 20.0f, 220.0f);
        float jitter = clamp(p1, 0.0f, 12.0f);
        float flicker_rate = clamp(p2, 0.0f, 4.0f);
        float opacity = clamp(p3, 0.1f, 1.0f);
        float row = floor(local_uv.y * scan_density);
        float gate = step(
            0.76f,
            vfx_hash21f((float2)(row, floor(time * flicker_rate * 8.0f))));
        float shift = (vfx_hash21f((float2)(row, 17.0f)) - 0.5f)
            * texel.x * jitter * gate;
        float4 body = vfx_sample_sprite(
            src_rgba, control, local_uv + (float2)(shift, 0.0f));
        float scan = 0.68f + 0.32f * native_sin(local_uv.y * scan_density * VFX_TAU);
        float flicker = 0.84f + 0.16f * native_sin(
            time * (8.0f + flicker_rate * 11.0f)
                + vfx_hash21f((float2)(row, 2.0f)) * VFX_TAU);
        float drop = mix(
            1.0f,
            0.12f,
            gate * step(
                0.45f,
                vfx_hash21f((float2)(row, floor(time * 12.0f)))));
        float3 tint_color = mix(color_a, color_b, local_uv.y);
        body.xyz = mix(body.xyz, tint_color, 0.68f) * scan * 1.18f;
        body.w *= opacity * flicker * drop;
        float echo = vfx_sample_sprite(
            src_rgba,
            control,
            local_uv + (float2)(texel.x * jitter * 1.7f, 0.0f)).w
            * gate * 0.25f;
        sprite = vfx_over(
            vfx_layer(color_b, echo * opacity),
            vfx_premultiply(body));
    } else if (shader_id == 6u) {
        // preview.html: RGB glitch
        float separation = clamp(p0, 0.0f, 10.0f);
        float slice_shift = clamp(p1, 0.0f, 24.0f);
        float speed = clamp(p2, 0.0f, 8.0f);
        float chaos = clamp(p3, 0.0f, 1.0f);
        float tick = floor(time * speed * 9.0f);
        float row = floor(local_uv.y * 18.0f);
        float chance = step(
            1.0f - chaos,
            vfx_hash21f((float2)(row, tick)));
        float shift = (vfx_hash21f((float2)(row, tick + 3.0f)) - 0.5f)
            * texel.x * slice_shift * chance;
        float channel_offset = texel.x * separation;
        float4 red = vfx_sample_sprite(
            src_rgba, control, local_uv + (float2)(shift + channel_offset, 0.0f));
        float4 green = vfx_sample_sprite(
            src_rgba, control, local_uv + (float2)(shift, 0.0f));
        float4 blue = vfx_sample_sprite(
            src_rgba, control, local_uv + (float2)(shift - channel_offset, 0.0f));
        float4 glitched = (float4)(
            red.x,
            green.y,
            blue.z,
            max(red.w, max(green.w, blue.w)));
        float band = step(
            0.86f,
            vfx_hash21f((float2)(floor(local_uv.y * 38.0f), tick + 9.0f)))
            * chaos;
        glitched.xyz = mix(
            glitched.xyz,
            mix(color_a, color_b, local_uv.x),
            band * 0.28f);
        sprite = vfx_premultiply(glitched);
    } else if (shader_id == 7u) {
        // preview.html: Dissolve
        float progress = clamp(p0, 0.0f, 1.0f);
        float edge_width = clamp(p1, 0.01f, 0.28f);
        float noise_scale = clamp(p2, 2.0f, 22.0f);
        float emission = clamp(p3, 0.0f, 3.0f);
        float noise = vfx_fbm(
            local_uv * noise_scale + (float2)(time * 0.09f, -time * 0.06f));
        float visible = smoothstep(
            progress - edge_width,
            progress + edge_width,
            noise);
        float edge = 1.0f - smoothstep(
            0.0f,
            edge_width,
            max(0.0f, fabs(noise - progress) - edge_width * 0.18f));
        float4 body = base;
        body.w *= visible;
        body.xyz = mix(body.xyz, color_a, edge * 0.18f);
        float3 edge_color = mix(
            color_a,
            color_b,
            smoothstep(progress - edge_width, progress + edge_width, noise));
        float4 burn = vfx_layer(
            edge_color,
            base.w * edge * emission * (1.0f - visible * 0.25f));
        sprite = vfx_over(vfx_premultiply(body), burn);
    } else if (shader_id == 8u) {
        // preview.html: Ghost trail
        float trail_distance = clamp(p0, 1.0f, 30.0f);
        float trail_strength = clamp(p1, 0.0f, 1.8f);
        float waviness = clamp(p2, 0.0f, 4.0f);
        float body_opacity = clamp(p3, 0.05f, 1.0f);
        float wave = native_sin(
            local_uv.y * 11.0f + time * waviness * 2.0f)
            * texel.x * trail_distance * 0.25f;
        float2 offset = (float2)(
            texel.x * trail_distance + wave,
            texel.y * trail_distance * 0.12f * native_sin(time));
        float4 ghost4 = vfx_sample_sprite(src_rgba, control, local_uv + offset * 1.8f);
        ghost4.xyz = color_a;
        ghost4.w *= trail_strength * 0.12f;
        float4 ghost3 = vfx_sample_sprite(src_rgba, control, local_uv + offset * 1.3f);
        ghost3.xyz = mix(color_a, color_b, 0.3f);
        ghost3.w *= trail_strength * 0.18f;
        float4 ghost2 = vfx_sample_sprite(src_rgba, control, local_uv + offset * 0.8f);
        ghost2.xyz = mix(color_a, color_b, 0.55f);
        ghost2.w *= trail_strength * 0.25f;
        float4 body = vfx_sample_sprite(
            src_rgba, control, local_uv + (float2)(wave, 0.0f));
        body.xyz = mix(body.xyz, color_b, 0.35f);
        body.w *= body_opacity;
        float4 trails = vfx_over(vfx_premultiply(ghost4), vfx_premultiply(ghost3));
        trails = vfx_over(trails, vfx_premultiply(ghost2));
        sprite = vfx_over(trails, vfx_premultiply(body));
    } else if (shader_id == 9u) {
        // preview.html: Electric arc
        float width = clamp(p0, 1.0f, 14.0f);
        float arc_scale = clamp(p1, 2.0f, 30.0f);
        float speed = clamp(p2, 0.0f, 6.0f);
        float intensity = clamp(p3, 0.0f, 3.0f);
        float outer = max(
            0.0f,
            vfx_dilate_alpha(src_rgba, control, local_uv, width) - base.w);
        float blurred = max(
            0.0f,
            vfx_blur_alpha(src_rgba, control, local_uv, width * 2.5f) - base.w);
        float noise = vfx_fbm(
            local_uv * arc_scale
                + (float2)(time * speed * 1.3f, -time * speed * 0.8f));
        float zig = vfx_powi(
            0.5f + 0.5f * native_sin(
                (local_uv.x * 1.7f + local_uv.y) * arc_scale * VFX_TAU
                    + noise * 5.0f),
            10u);
        float arc = outer * (0.22f + 0.95f * zig);
        float3 energy_color = mix(color_a, color_b, zig);
        float sparks = max(
            0.0f,
            vfx_dilate_alpha(src_rgba, control, local_uv, width * 2.1f)
                - vfx_dilate_alpha(src_rgba, control, local_uv, width * 1.25f))
            * step(0.78f, noise) * zig;
        float4 energy = vfx_layer(
            energy_color,
            (arc + blurred * 0.28f + sparks) * intensity);
        float4 body = base;
        body.xyz += energy_color * max(
            0.0f,
            base.w - vfx_erode_alpha(
                src_rgba, control, local_uv, width * 0.45f))
            * 0.12f * intensity;
        sprite = vfx_over(energy, vfx_premultiply(body));
    } else if (shader_id == 10u) {
        // preview.html: Rainbow prism
        float hue_speed = clamp(p0, 0.0f, 3.0f);
        float band_scale = clamp(p1, 0.5f, 14.0f);
        float color_mix = clamp(p2, 0.0f, 1.0f);
        float edge_width = clamp(p3, 0.0f, 10.0f);
        float hue = vfx_fract(
            local_uv.y * band_scale + local_uv.x * 0.7f
                + time * hue_speed * 0.18f
                + vfx_fbm(local_uv * 3.0f) * 0.12f);
        float3 prism = vfx_hsv2rgb((float3)(hue, 0.78f, 1.0f));
        float4 body = base;
        body.xyz = mix(body.xyz, body.xyz * prism * 1.45f, color_mix);
        float outer = max(
            0.0f,
            vfx_dilate_alpha(src_rgba, control, local_uv, edge_width) - base.w);
        float4 rim = vfx_layer(mix(color_a, color_b, hue), outer * 0.9f);
        sprite = vfx_over(rim, vfx_premultiply(body));
    } else if (shader_id == 11u) {
        // preview.html: Hit flash
        float flash_amount = clamp(p0, 0.0f, 1.0f);
        float pulse_speed = clamp(p1, 0.0f, 8.0f);
        float rim_width = clamp(p2, 0.0f, 14.0f);
        float shake_amount = clamp(p3, 0.0f, 8.0f);
        float pulse = 0.5f + 0.5f * native_sin(time * pulse_speed * VFX_TAU);
        float2 shake = texel * shake_amount
            * (float2)(native_sin(time * 51.0f), native_cos(time * 43.0f))
            * pulse;
        float4 body = vfx_sample_sprite(src_rgba, control, local_uv + shake);
        float flash = flash_amount * (0.35f + 0.65f * pulse);
        body.xyz = mix(body.xyz, (float3)(1.0f), flash);
        float outer = max(
            0.0f,
            vfx_dilate_alpha(src_rgba, control, local_uv, rim_width) - body.w);
        float3 rim_color = mix(color_b, color_a, pulse);
        sprite = vfx_over(
            vfx_layer(rim_color, outer * (0.35f + 0.65f * pulse)),
            vfx_premultiply(body));
    } else if (shader_id == 12u) {
        // preview.html: Pixel wave
        float amplitude = clamp(p0, 0.0f, 12.0f);
        float frequency = clamp(p1, 1.0f, 30.0f);
        float speed = clamp(p2, 0.0f, 6.0f);
        float color_steps = clamp(p3, 2.0f, 16.0f);
        float2 warped = local_uv;
        warped.x += native_sin(
            warped.y * frequency + time * speed * 2.4f)
            * texel.x * amplitude;
        float4 body = vfx_sample_sprite(src_rgba, control, warped);
        body.xyz = floor(body.xyz * color_steps + 0.5f) / color_steps;
        float scan = 0.93f + 0.07f * native_sin(
            warped.y / texel.y * 3.14159265359f);
        body.xyz *= scan;
        sprite = vfx_premultiply(body);
    } else if (shader_id == 13u) {
        // preview.html: Toon ink
        float color_steps = clamp(p0, 2.0f, 16.0f);
        float ink_width = clamp(p1, 0.5f, 8.0f);
        float saturation = clamp(p2, 0.0f, 2.0f);
        float outer_width = clamp(p3, 0.0f, 8.0f);
        float3 posterized = floor(base.xyz * color_steps + 0.5f) / color_steps;
        float luminance = dot(posterized, (float3)(0.299f, 0.587f, 0.114f));
        posterized = mix((float3)(luminance), posterized, saturation);
        float inner = max(
            0.0f,
            base.w - vfx_erode_alpha(src_rgba, control, local_uv, ink_width));
        posterized = mix(posterized, color_a, inner * 0.62f);
        float4 body = (float4)(posterized, base.w);
        float outer = max(
            0.0f,
            vfx_dilate_alpha(src_rgba, control, local_uv, outer_width) - base.w);
        sprite = vfx_over(
            vfx_layer(color_b, outer * 0.85f),
            vfx_premultiply(body));
    } else if (shader_id == 14u) {
        // preview.html: Liquid warp
        float warp = clamp(p0, 0.0f, 14.0f);
        float noise_scale = clamp(p1, 1.0f, 20.0f);
        float speed = clamp(p2, 0.0f, 4.0f);
        float chroma = clamp(p3, 0.0f, 7.0f);
        float tick = time * speed;
        float2 noise = (float2)(
            vfx_fbm(local_uv * noise_scale + (float2)(tick, -tick * 0.7f)),
            vfx_fbm(local_uv * noise_scale
                + (float2)(17.0f - tick * 0.5f, 9.0f + tick)))
            - 0.5f;
        float2 warped = local_uv + noise * texel * warp;
        float channel_offset = texel.x * chroma;
        float4 red = vfx_sample_sprite(
            src_rgba, control, warped + (float2)(channel_offset, 0.0f));
        float4 green = vfx_sample_sprite(src_rgba, control, warped);
        float4 blue = vfx_sample_sprite(
            src_rgba, control, warped - (float2)(channel_offset, 0.0f));
        float4 liquid = (float4)(
            red.x,
            green.y,
            blue.z,
            max(red.w, max(green.w, blue.w)));
        liquid.xyz = mix(
            liquid.xyz,
            liquid.xyz * mix(color_a, color_b, noise.x + 0.5f) * 1.2f,
            0.12f);
        sprite = vfx_premultiply(liquid);
    } else if (shader_id == 15u) {
        // preview.html: Dream bloom
        float bloom_radius = clamp(p0, 2.0f, 30.0f);
        float softness = clamp(p1, 0.0f, 2.0f);
        float float_speed = clamp(p2, 0.0f, 3.0f);
        float pastel_mix = clamp(p3, 0.0f, 1.0f);
        float floaty = native_sin(
            time * float_speed * 1.8f + local_uv.x * VFX_TAU)
            * texel.y * 1.6f;
        float2 warped = local_uv + (float2)(0.0f, floaty);
        float4 body = vfx_sample_sprite(src_rgba, control, warped);
        float blurred = vfx_blur_alpha(
            src_rgba, control, warped, bloom_radius * 0.55f) * 0.64f
            + vfx_blur_alpha(src_rgba, control, warped, bloom_radius) * 0.36f;
        float glow = max(0.0f, blurred - body.w * 0.18f) * softness;
        float3 glow_color = mix(
            color_a,
            color_b,
            0.5f + 0.5f * native_sin(
                (local_uv.x + local_uv.y + time * 0.12f) * VFX_TAU));
        body.xyz = mix(body.xyz, mix(body.xyz, glow_color, 0.32f), pastel_mix);
        float4 soft = vfx_layer(glow_color, min(glow, 0.8f));
        float4 echo = vfx_sample_sprite(
            src_rgba,
            control,
            warped + (float2)(
                texel.x * bloom_radius * 0.32f,
                texel.y * bloom_radius * 0.2f));
        echo.xyz = color_a;
        echo.w *= 0.08f * softness;
        sprite = vfx_over(
            vfx_over(soft, vfx_premultiply(echo)),
            vfx_premultiply(body));
    }

#if defined(TRUEOS_SPIRIT_CPP_REPASS)
    if (shader_id != 0u) {
        const SpiritCppSpriteEdges cpp_edges = vfx_cpp_sprite_edges(
            src_rgba,
            control,
            local_uv,
            texel,
            base.w);
        sprite = vfx_over(
            sprite,
            vfx_cpp_sprite_dispatch(
                shader_id,
                cpp_edges,
                local_uv,
                time,
                color_a,
                color_b));
    }
#endif

    uint dst_pitch_pixels = control[VFX_CTRL_DST_PITCH] >> 2;
    uint dst_index = y * dst_pitch_pixels + x;
    // Background mode zero is the default Lilly-only path. It deliberately
    // ignores the old backbuffer contents, allowing the host to omit the
    // procedural-background walker without retaining stale pixels.
    float4 background = control[VFX_CTRL_BACKGROUND_ID] == 0u
        ? (float4)(0.0f)
        : vfx_unpack_bgra8_premultiplied(dst_bgra[dst_index]);
    float4 composed = vfx_over(background, sprite);

    // This is deliberately the final operation over the complete cursor
    // image. Premultiplied color and alpha are attenuated together, reaching
    // exact transparency at every outermost pixel without a separate blur
    // allocation or a hard 256x256 boundary.
    float fade_pixels = clamp(
        vfx_finite_or(as_float(control[VFX_CTRL_EDGE_FADE_F32]), 12.0f),
        0.0f,
        16.0f);
    if (fade_pixels > 0.0f) {
        float edge_distance = min(
            min((float)x + 0.5f, 255.5f - (float)x),
            min((float)y + 0.5f, 255.5f - (float)y));
        float edge_alpha = smoothstep(0.5f, fade_pixels + 0.5f, edge_distance);
        composed *= edge_alpha;
    }
    dst_bgra[dst_index] = vfx_pack_bgra8_premultiplied(composed);
}
