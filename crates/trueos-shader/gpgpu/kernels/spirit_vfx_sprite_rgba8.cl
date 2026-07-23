// Spirit VFX resident-sprite pass for Intel Xe-LP / ADL-S.
//
// A Lilly RGBA8 frame is transformed and source-over composited onto the
// procedural background. The first bounded artifact supports Original/clean
// and Aura bloom; other UI effect IDs deliberately fall back to clean.

#define SPIRIT_VFX_SIZE 256u
#define SPIRIT_VFX_CONTROL_MAGIC 0x53564658u // "SVFX"
#define SPIRIT_VFX_CONTROL_VERSION 1u

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

    float4 base = vfx_sample_sprite(src_rgba, control, local_uv);
    float4 sprite = (float4)(base.xyz * base.w, base.w);
    if (control[VFX_CTRL_SHADER_ID] == 1u) {
        // preview.html: Aura bloom
        float radius = clamp(
            vfx_finite_or(as_float(control[VFX_CTRL_SHADER_P0_F32]), 12.0f),
            2.0f,
            30.0f);
        float strength = clamp(
            vfx_finite_or(as_float(control[VFX_CTRL_SHADER_P0_F32 + 1u]), 1.15f),
            0.0f,
            2.5f);
        float pulse_rate = clamp(
            vfx_finite_or(as_float(control[VFX_CTRL_SHADER_P0_F32 + 2u]), 1.2f),
            0.0f,
            4.0f);
        float brighten = clamp(
            vfx_finite_or(as_float(control[VFX_CTRL_SHADER_P0_F32 + 3u]), 0.18f),
            0.0f,
            1.0f);
        float time = vfx_finite_or(as_float(control[VFX_CTRL_TIME_F32]), 0.0f);
        float pulse = 0.78f + 0.22f * native_sin(time * 6.28318530718f * 0.35f * pulse_rate);
        float blurred = vfx_blur_alpha(src_rgba, control, local_uv, radius * 0.45f) * 0.62f
            + vfx_blur_alpha(src_rgba, control, local_uv, radius) * 0.38f;
        float edge = max(0.0f, blurred - base.w * 0.25f);
        float flow = 0.5f + 0.5f * native_sin(
            (local_uv.y * 2.4f + local_uv.x) * 6.28318530718f + time);
        float3 glow_color = mix(
            vfx_unpack_rgb(control[VFX_CTRL_FX_COLOR_A]),
            vfx_unpack_rgb(control[VFX_CTRL_FX_COLOR_B]),
            flow);
        float glow_alpha = clamp(edge * strength * pulse, 0.0f, 0.92f);
        float4 glow = (float4)(glow_color * glow_alpha, glow_alpha);
        float3 brightened = mix(base.xyz, 1.0f - (1.0f - base.xyz) * 0.7f, brighten);
        sprite = vfx_over(glow, (float4)(brightened * base.w, base.w));
    }

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
