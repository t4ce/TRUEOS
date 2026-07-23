// Spirit VFX procedural-alpha background pass for Intel Xe-LP / ADL-S.
//
// The dispatch is fixed at 256x256. It writes premultiplied BGRA8 bytes into
// the exact Intel cursor backbuffer. Modes 1 and 4 intentionally retain the
// names and parameter meanings of preview.html: Radial aura and Nebula smoke.

#define SPIRIT_VFX_SIZE 256u
#define SPIRIT_VFX_CONTROL_MAGIC 0x53564658u // "SVFX"
#define SPIRIT_VFX_CONTROL_VERSION 1u

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

static inline uint vfx_hash(uint value)
{
    value ^= value >> 16;
    value *= 0x7FEB352Du;
    value ^= value >> 15;
    value *= 0x846CA68Bu;
    return value ^ (value >> 16);
}

static inline float vfx_hash21(int2 point)
{
    uint x = as_uint(point.x);
    uint y = as_uint(point.y);
    return (float)(vfx_hash(x ^ (y * 0x9E3779B9u)) & 0x00FFFFFFu)
        * (1.0f / 16777215.0f);
}

static inline float vfx_noise21(float2 point)
{
    float2 floored = floor(point);
    int2 cell = (int2)((int)floored.x, (int)floored.y);
    float2 fraction = point - floored;
    fraction = fraction * fraction * (3.0f - 2.0f * fraction);
    float a = vfx_hash21(cell);
    float b = vfx_hash21(cell + (int2)(1, 0));
    float c = vfx_hash21(cell + (int2)(0, 1));
    float d = vfx_hash21(cell + (int2)(1, 1));
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
    if (background_id == 1u) {
        // preview.html: Radial aura
        float core = native_exp(-radius * radius * 8.5f);
        float ray_wave = 0.5f + 0.5f * native_sin(
            angle * 10.0f + time * speed + radius * 18.0f);
        float ray_wave2 = ray_wave * ray_wave;
        float ray_wave4 = ray_wave2 * ray_wave2;
        float rays = ray_wave4 * ray_wave4 * native_exp(-radius * 5.0f);
        float noise = vfx_fbm(point * 7.0f + time * speed * 0.08f);
        alpha = (core * (0.55f + 0.45f * noise) + rays * 0.42f) * intensity;
        color_mix = vfx_clamp01(radius * 2.4f + noise * 0.35f);
    } else if (background_id == 4u) {
        // preview.html: Nebula smoke
        float noise = vfx_fbm(
            point * 5.2f + (float2)(time * speed * 0.11f, -time * speed * 0.07f));
        float noise2 = vfx_fbm(
            point * 9.0f - (float2)(time * speed * 0.05f, time * speed * 0.09f));
        float cloud = smoothstep(0.32f, 0.82f, noise * 0.72f + noise2 * 0.38f)
            * native_exp(-radius * radius * 3.2f);
        alpha = cloud * intensity;
        color_mix = vfx_clamp01(noise2);
    }

    alpha = clamp(alpha * opacity, 0.0f, 0.96f);
    float3 color = mix(
        vfx_unpack_rgb(control[VFX_CTRL_BG_COLOR_A]),
        vfx_unpack_rgb(control[VFX_CTRL_BG_COLOR_B]),
        vfx_clamp01(color_mix));
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_bgra[y * dst_pitch_pixels + x] = vfx_pack_bgra8_premultiplied(color, alpha);
}
