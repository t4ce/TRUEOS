// TRUEOS Lab256 multi-phase compute exploration for Intel Xe-LP / ADL-S.
//
// This is intentionally a fixed 256x256 capability, not a general image API.
// One IGC zebin contains three entry points which the host dispatches in order:
//
//   1. lab256_step       packed Gray-Scott state A -> B
//   2. lab256_reduce     B -> compact per-lane telemetry report
//   3. lab256_composite  B + report + CPU control -> RGBA8
//
// The fixed extent makes the bounds, memory cost, and maximum shader work
// auditable. The host owns all buffers, validates the control page, and places
// a stalling HDC flush/invalidate boundary between passes.

#define LAB256_SIZE 256u
#define LAB256_PIXELS (LAB256_SIZE * LAB256_SIZE)

#define LAB256_CONTROL_MAGIC 0x4C414232u // "LAB2"
#define LAB256_CONTROL_VERSION 3u
#define LAB256_REPORT_MAGIC 0x4C325250u // "L2RP"

#define LAB256_FLAG_WRAP        (1u << 0)
#define LAB256_FLAG_INJECT      (1u << 1)
#define LAB256_FLAG_RESET       (1u << 2)
#define LAB256_FLAG_POINTER_SHADE (1u << 4)

// Control page, in dwords. Floats are stored as IEEE-754 bit patterns.
#define LAB256_CTRL_MAGIC            0u
#define LAB256_CTRL_VERSION          1u
#define LAB256_CTRL_FRAME            2u
#define LAB256_CTRL_FLAGS            3u
#define LAB256_CTRL_TIME_F32         4u
#define LAB256_CTRL_POINTER_XY       5u  // unsigned 16-bit x, y pixels
#define LAB256_CTRL_POINTER_RADIUS   6u  // f32 pixels
#define LAB256_CTRL_POINTER_STRENGTH 7u  // f32 [0, 1]
#define LAB256_CTRL_FEED_F32         8u
#define LAB256_CTRL_KILL_F32         9u
#define LAB256_CTRL_DT_F32          10u
#define LAB256_CTRL_FLARE_RADIUS    11u // f32 normalized scene radius
#define LAB256_CTRL_FLARE_TURBULENCE 12u // f32 radial distortion
#define LAB256_CTRL_FLARE_RAY_GAIN  13u // f32 ray intensity
#define LAB256_CTRL_FLARE_PULSE_SPEED 14u // f32 animation rate
#define LAB256_CTRL_COLOR_SEED      15u
#define LAB256_CTRL_RESERVED_16     16u
#define LAB256_CTRL_BACKGROUND_ALPHA 17u // f32 [0, 1]
#define LAB256_CTRL_PRESENT_FPS     18u // half-second CUR_SURFLIVE estimate
#define LAB256_CONTROL_DWORDS       19u

// Report layout: 16 header words followed by one 8-dword stripe per SIMD
// lane. Each lane scans 4096 pixels, avoiding contended global atomics.
#define LAB256_REPORT_HEADER_DWORDS 16u
#define LAB256_REPORT_LANES         16u
#define LAB256_REPORT_STRIDE         8u
#define LAB256_REPORT_DWORDS \
    (LAB256_REPORT_HEADER_DWORDS + LAB256_REPORT_LANES * LAB256_REPORT_STRIDE)

static inline float lab256_finite_or(float value, float fallback)
{
    return isfinite(value) ? value : fallback;
}

static inline uint lab256_hash(uint value)
{
    value ^= value >> 16;
    value *= 0x7FEB352Du;
    value ^= value >> 15;
    value *= 0x846CA68Bu;
    return value ^ (value >> 16);
}

static inline float2 lab256_unpack_state(uint packed)
{
    return (float2)(
        (float)(packed & 0xFFFFu) * (1.0f / 65535.0f),
        (float)(packed >> 16) * (1.0f / 65535.0f));
}

static inline uint lab256_pack_state(float2 state)
{
    state = clamp(state, (float2)(0.0f), (float2)(1.0f));
    uint u = (uint)(state.x * 65535.0f + 0.5f);
    uint v = (uint)(state.y * 65535.0f + 0.5f);
    return (v << 16) | u;
}

static inline uint lab256_coord(int value, uint flags)
{
    if ((flags & LAB256_FLAG_WRAP) != 0u) {
        return (uint)value & 255u;
    }
    return (uint)clamp(value, 0, 255);
}

static inline float2 lab256_load_state(
    __global const uint *state,
    int x,
    int y,
    uint flags)
{
    uint sx = lab256_coord(x, flags);
    uint sy = lab256_coord(y, flags);
    return lab256_unpack_state(state[sy * LAB256_SIZE + sx]);
}

static inline float2 lab256_seed(uint x, uint y, uint seed)
{
    int dx0 = (int)x - 128;
    int dy0 = (int)y - 128;
    int dx1 = (int)x - 78;
    int dy1 = (int)y - 166;
    int dx2 = (int)x - 181;
    int dy2 = (int)y - 75;
    bool spot = dx0 * dx0 + dy0 * dy0 < 18 * 18
        || dx1 * dx1 + dy1 * dy1 < 11 * 11
        || dx2 * dx2 + dy2 * dy2 < 14 * 14;
    float noise = (float)(lab256_hash(x + y * LAB256_SIZE + seed) & 1023u)
        * (1.0f / 1023.0f);
    float v = spot ? 0.82f + noise * 0.16f : noise * 0.012f;
    return (float2)(1.0f - v * 0.55f, v);
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void lab256_step(
    __global const uint *state_in,
    __global uint *state_out,
    __global const uint *control)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);
    if (x >= LAB256_SIZE || y >= LAB256_SIZE) {
        return;
    }

    uint flags = control[LAB256_CTRL_FLAGS];
    uint frame = control[LAB256_CTRL_FRAME];
    bool reset = (flags & LAB256_FLAG_RESET) != 0u
        || control[LAB256_CTRL_MAGIC] != LAB256_CONTROL_MAGIC
        || control[LAB256_CTRL_VERSION] != LAB256_CONTROL_VERSION;

    float2 center = reset
        ? lab256_seed(x, y, control[LAB256_CTRL_COLOR_SEED])
        : lab256_load_state(state_in, (int)x, (int)y, flags);
    if (!reset) {
        float2 cardinal = lab256_load_state(state_in, (int)x - 1, (int)y, flags)
            + lab256_load_state(state_in, (int)x + 1, (int)y, flags)
            + lab256_load_state(state_in, (int)x, (int)y - 1, flags)
            + lab256_load_state(state_in, (int)x, (int)y + 1, flags);
        float2 diagonal = lab256_load_state(state_in, (int)x - 1, (int)y - 1, flags)
            + lab256_load_state(state_in, (int)x + 1, (int)y - 1, flags)
            + lab256_load_state(state_in, (int)x - 1, (int)y + 1, flags)
            + lab256_load_state(state_in, (int)x + 1, (int)y + 1, flags);
        float2 laplacian = cardinal * 0.20f + diagonal * 0.05f - center;

        float feed = clamp(
            lab256_finite_or(as_float(control[LAB256_CTRL_FEED_F32]), 0.0367f),
            0.005f,
            0.095f);
        float kill = clamp(
            lab256_finite_or(as_float(control[LAB256_CTRL_KILL_F32]), 0.0649f),
            0.025f,
            0.080f);
        float dt = clamp(
            lab256_finite_or(as_float(control[LAB256_CTRL_DT_F32]), 1.0f),
            0.05f,
            1.25f);
        float reaction = center.x * center.y * center.y;
        center += (float2)(
            0.16f * laplacian.x - reaction + feed * (1.0f - center.x),
            0.08f * laplacian.y + reaction - (feed + kill) * center.y) * dt;
    }

    if ((flags & LAB256_FLAG_INJECT) != 0u) {
        uint packed_xy = control[LAB256_CTRL_POINTER_XY];
        float pointer_x = (float)(packed_xy & 0xFFFFu);
        float pointer_y = (float)(packed_xy >> 16);
        float radius = clamp(
            lab256_finite_or(as_float(control[LAB256_CTRL_POINTER_RADIUS]), 12.0f),
            1.0f,
            48.0f);
        float strength = clamp(
            lab256_finite_or(as_float(control[LAB256_CTRL_POINTER_STRENGTH]), 0.8f),
            0.0f,
            1.0f);
        float2 delta = (float2)((float)x - pointer_x, (float)y - pointer_y);
        float falloff = clamp(1.0f - native_sqrt(dot(delta, delta)) / radius, 0.0f, 1.0f);
        falloff = falloff * falloff * (3.0f - 2.0f * falloff) * strength;
        center.y = mix(center.y, 1.0f, falloff);
        center.x = mix(center.x, 0.18f, falloff * 0.8f);
    }

    // Keep a tiny deterministic perturbation alive after long stable runs.
    float dither = (float)(lab256_hash(x ^ (y << 8) ^ frame) & 255u)
        * (1.0f / 255.0f) - 0.5f;
    center.y += dither * (1.0f / 65535.0f);
    state_out[y * LAB256_SIZE + x] = lab256_pack_state(center);
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void lab256_reduce(
    __global const uint *state,
    __global uint *report,
    __global const uint *control)
{
    uint lane = get_global_id(0);
    if (lane >= LAB256_REPORT_LANES) {
        return;
    }

    uint sum = 0u;
    uint sum_sq = 0u;
    uint maximum = 0u;
    uint weighted_x = 0u;
    uint weighted_y = 0u;
    uint active = 0u;
    uint checksum = 2166136261u ^ lane;

    for (uint index = lane; index < LAB256_PIXELS; index += LAB256_REPORT_LANES) {
        uint packed = state[index];
        uint value = packed >> 24;
        uint x = index & 255u;
        uint y = index >> 8;
        sum += value;
        sum_sq += value * value;
        maximum = max(maximum, value);
        weighted_x += value * x;
        weighted_y += value * y;
        active += value >= 32u;
        checksum = (checksum ^ packed) * 16777619u;
    }

    if (lane == 0u) {
        report[0] = LAB256_REPORT_MAGIC;
        report[1] = LAB256_CONTROL_VERSION;
        report[2] = control[LAB256_CTRL_FRAME];
        report[3] = control[LAB256_CTRL_FLAGS];
        report[4] = LAB256_SIZE;
        report[5] = LAB256_REPORT_DWORDS;
        report[6] = LAB256_REPORT_LANES;
        report[7] = LAB256_REPORT_STRIDE;
    }

    uint base = LAB256_REPORT_HEADER_DWORDS + lane * LAB256_REPORT_STRIDE;
    report[base + 0u] = sum;
    report[base + 1u] = sum_sq;
    report[base + 2u] = maximum;
    report[base + 3u] = weighted_x;
    report[base + 4u] = weighted_y;
    report[base + 5u] = active;
    report[base + 6u] = checksum;
    report[base + 7u] = 0xD06E0000u | lane;
}

static inline float lab256_clamp01(float value)
{
    return clamp(value, 0.0f, 1.0f);
}

static inline uint lab256_pack_rgba8(float3 color, float alpha)
{
    float a = clamp(alpha, 0.0f, 1.0f);
    color = clamp(color, (float3)(0.0f), (float3)(1.0f));
    // UI4 consumes premultiplied RGBA8: bytes in increasing memory order are
    // R, G, B, A, represented here as an AABBGGRR little-endian dword.
    uint r = (uint)(color.x * a * 255.0f + 0.5f);
    uint g = (uint)(color.y * a * 255.0f + 0.5f);
    uint b = (uint)(color.z * a * 255.0f + 0.5f);
    uint ai = (uint)(a * 255.0f + 0.5f);
    return (ai << 24) | (b << 16) | (g << 8) | r;
}

static inline float lab256_mean_v(__global const uint *report)
{
    uint sum = 0u;
    for (uint lane = 0u; lane < LAB256_REPORT_LANES; lane++) {
        uint base = LAB256_REPORT_HEADER_DWORDS + lane * LAB256_REPORT_STRIDE;
        sum += report[base];
    }
    return (float)sum * (1.0f / (255.0f * 65536.0f));
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void lab256_composite(
    __global const uint *state,
    __global const uint *report,
    __global const uint *control,
    __global uint *dst_rgba,
    uint dst_pitch_bytes)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);
    if (x >= LAB256_SIZE || y >= LAB256_SIZE || dst_pitch_bytes < LAB256_SIZE * 4u) {
        return;
    }

    uint flags = control[LAB256_CTRL_FLAGS];
    float time = lab256_finite_or(as_float(control[LAB256_CTRL_TIME_F32]), 0.0f);
    float2 state_uv = lab256_unpack_state(state[y * LAB256_SIZE + x]);
    float field = state_uv.y;
    float mean_v = report[0] == LAB256_REPORT_MAGIC ? lab256_mean_v(report) : 0.0f;
    float2 point = ((float2)((float)x + 0.5f, (float)y + 0.5f) / 256.0f - 0.5f) * 2.0f;

    // The flare stays near the avatar center while the physical cursor can
    // lean its emitter by roughly 18 pixels in either direction. This keeps
    // the hardware cursor surface spatially stable while making the scene
    // visibly responsive to the kernel cursor snapshot.
    float2 emitter = (float2)(0.0f);
    if ((flags & LAB256_FLAG_POINTER_SHADE) != 0u) {
        uint packed_xy = control[LAB256_CTRL_POINTER_XY];
        float2 pointer_point = (float2)(
            (float)(packed_xy & 0xFFFFu),
            (float)(packed_xy >> 16)) * (2.0f / 255.0f) - 1.0f;
        emitter = pointer_point * 0.14f;
    }

    float flare_radius = clamp(
        lab256_finite_or(as_float(control[LAB256_CTRL_FLARE_RADIUS]), 0.52f),
        0.22f,
        0.82f);
    float turbulence = clamp(
        lab256_finite_or(as_float(control[LAB256_CTRL_FLARE_TURBULENCE]), 0.12f),
        0.0f,
        0.30f);
    float ray_gain = clamp(
        lab256_finite_or(as_float(control[LAB256_CTRL_FLARE_RAY_GAIN]), 0.82f),
        0.0f,
        1.5f);
    float pulse_speed = clamp(
        lab256_finite_or(as_float(control[LAB256_CTRL_FLARE_PULSE_SPEED]), 1.0f),
        0.1f,
        4.0f);

    float2 flare_point = point - emitter;
    float radius2 = dot(flare_point, flare_point);
    float radius = native_sqrt(max(radius2, 1.0e-8f));
    float edge = fabs(state_uv.x - state_uv.y);
    float organic = clamp((field - mean_v) * 1.4f + edge * 0.65f, -1.0f, 1.0f);
    float radial_wave = 0.5f + 0.5f * native_sin(
        radius * 26.0f
            - time * (1.65f * pulse_speed)
            + field * 7.5f
            + edge * 4.0f);
    float distorted_radius = flare_radius
        + organic * turbulence
        + (radial_wave - 0.5f) * (turbulence * 0.55f);

    float body_t = lab256_clamp01(
        (radius - (distorted_radius - 0.10f)) * 5.0f);
    body_t = body_t * body_t * (3.0f - 2.0f * body_t);
    float body = 1.0f - body_t;

    float outer_t = lab256_clamp01(radius * (1.0f / 1.15f));
    outer_t = outer_t * outer_t * (3.0f - 2.0f * outer_t);
    float outer = 1.0f - outer_t;
    float plume = outer
        * (0.12f + radial_wave * 0.48f)
        * (0.38f + lab256_clamp01(field + edge * 1.8f) * 0.62f);

    // Four analytical flare axes replace the reference shader's texture
    // feedback. They are cheap reciprocal distance fields whose brightness is
    // modulated by the persistent reaction state and the same radial pulse.
    float axis_x = 0.008f / (fabs(flare_point.y) + 0.008f);
    float axis_y = 0.008f / (fabs(flare_point.x) + 0.008f);
    float diagonal_a = 0.010f
        / (fabs((flare_point.x - flare_point.y) * 0.70710678f) + 0.010f);
    float diagonal_b = 0.010f
        / (fabs((flare_point.x + flare_point.y) * 0.70710678f) + 0.010f);
    float ray_shape = max(max(axis_x, axis_y), max(diagonal_a, diagonal_b) * 0.64f);
    float ray_envelope = 1.0f / (1.0f + radius2 * 2.4f);
    float rays = ray_shape
        * ray_envelope
        * ray_gain
        * (0.52f + radial_wave * 0.48f)
        * (0.72f + field * 0.28f);

    float core = 0.010f / (radius2 + 0.010f);
    float halo = 0.055f / (radius2 + 0.055f);
    float3 color = (float3)(1.00f, 0.12f, 0.018f) * (body * 0.68f + plume * 0.54f)
        + (float3)(1.00f, 0.62f, 0.10f) * (body * 0.44f + halo * 0.32f)
        + (float3)(1.00f, 0.98f, 0.84f) * (core * 1.9f + rays * 1.05f)
        + (float3)(0.16f, 0.30f, 1.00f) * (halo * 0.16f + plume * 0.07f);

    float background_alpha = clamp(
        lab256_finite_or(
            as_float(control[LAB256_CTRL_BACKGROUND_ALPHA]),
            0.08f),
        0.0f,
        1.0f);
    float content_alpha = lab256_clamp01(
        core * 1.8f
            + body * 0.78f
            + plume * 0.56f
            + rays * 0.88f
            + halo * 0.20f);
    float output_alpha = mix(background_alpha, 1.0f, content_alpha);

    if ((flags & LAB256_FLAG_INJECT) != 0u) {
        uint packed_xy = control[LAB256_CTRL_POINTER_XY];
        int pointer_x = (int)(packed_xy & 0xFFFFu);
        int pointer_y = (int)(packed_xy >> 16);
        int dx = abs((int)x - pointer_x);
        int dy = abs((int)y - pointer_y);
        if ((dx <= 7 && dy == 0) || (dy <= 7 && dx == 0)) {
            color = (float3)(0.94f, 0.98f, 1.0f);
            output_alpha = 1.0f;
        }
    }

    // One fully opaque status dot reports Spirit's half-second average of
    // cursor-plane CUR_SURFLIVE completions. The host seeds it at the 60 Hz
    // target until the first complete sampling window is available.
    int fps_dot_x = (int)x - 10;
    int fps_dot_y = (int)y - 10;
    if (fps_dot_x * fps_dot_x + fps_dot_y * fps_dot_y <= 9) {
        uint present_fps = control[LAB256_CTRL_PRESENT_FPS];
        color = present_fps > 50u
            ? (float3)(0.12f, 0.95f, 0.30f)
            : present_fps >= 30u
                ? (float3)(1.00f, 0.88f, 0.08f)
                : (float3)(1.00f, 0.40f, 0.05f);
        output_alpha = 1.0f;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_rgba[y * dst_pitch_pixels + x] = lab256_pack_rgba8(color, output_alpha);
}
