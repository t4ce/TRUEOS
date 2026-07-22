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
#define LAB256_CONTROL_VERSION 5u
#define LAB256_REPORT_MAGIC 0x4C325250u // "L2RP"

#define LAB256_FLAG_WRAP        (1u << 0)
#define LAB256_FLAG_INJECT      (1u << 1)
#define LAB256_FLAG_RESET       (1u << 2)

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
#define LAB256_CTRL_FOG_RADIUS       11u // f32 normalized scene radius
#define LAB256_CTRL_FOG_WARP         12u // f32 low-order radial distortion
#define LAB256_CTRL_FOG_RIPPLE_GAIN  13u // f32 ripple intensity
#define LAB256_CTRL_FOG_PULSE_SPEED  14u // f32 animation rate
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
    float noise = (float)(lab256_hash(x + y * LAB256_SIZE + seed) & 1023u)
        * (1.0f / 1023.0f);
    // Start almost chemically quiet. Pointer injection is the only strong B
    // source, so the reaction layer cannot grow unrelated seeded colonies.
    float v = noise * 0.0015f;
    return (float2)(1.0f, v);
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

        // Drain twice as quickly as the initial trail experiment. At 60 Hz
        // this confines the visible wake to roughly the recent half second.
        center.y *= 0.970f;
    }

    if ((flags & LAB256_FLAG_INJECT) != 0u) {
        uint packed_xy = control[LAB256_CTRL_POINTER_XY];
        float pointer_x = (float)(packed_xy & 0xFFFFu);
        float pointer_y = (float)(packed_xy >> 16);
        float radius = clamp(
            lab256_finite_or(as_float(control[LAB256_CTRL_POINTER_RADIUS]), 18.0f),
            1.0f,
            48.0f);
        float strength = clamp(
            lab256_finite_or(as_float(control[LAB256_CTRL_POINTER_STRENGTH]), 0.58f),
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

    float time = lab256_finite_or(as_float(control[LAB256_CTRL_TIME_F32]), 0.0f);
    float2 state_uv = lab256_unpack_state(state[y * LAB256_SIZE + x]);
    float field = state_uv.y;
    float mean_v = report[0] == LAB256_REPORT_MAGIC ? lab256_mean_v(report) : 0.0f;
    float2 point = ((float2)((float)x + 0.5f, (float)y + 0.5f) / 256.0f - 0.5f) * 2.0f;

    float fog_radius = clamp(
        lab256_finite_or(as_float(control[LAB256_CTRL_FOG_RADIUS]), 0.72f),
        0.22f,
        1.20f);
    float fog_warp = clamp(
        lab256_finite_or(as_float(control[LAB256_CTRL_FOG_WARP]), 0.12f),
        0.0f,
        0.30f);
    float ripple_gain = clamp(
        lab256_finite_or(as_float(control[LAB256_CTRL_FOG_RIPPLE_GAIN]), 0.68f),
        0.0f,
        1.5f);
    float pulse_speed = clamp(
        lab256_finite_or(as_float(control[LAB256_CTRL_FOG_PULSE_SPEED]), 1.0f),
        0.1f,
        4.0f);

    // A centered low-contrast fog pulse replaces the hard flare core and its
    // reciprocal star rays. Pointer coordinates never enter this geometry.
    float2 fog_point = point;
    float radius2 = dot(fog_point, fog_point);
    float radius = native_sqrt(max(radius2, 1.0e-8f));
    float lobe = (fog_point.x * fog_point.x - fog_point.y * fog_point.y)
        / (radius2 + 0.04f);
    float warped_radius = radius * (1.0f + lobe * fog_warp * 0.12f);
    float radial_wave = 0.5f + 0.5f * native_sin(
        warped_radius * 15.0f
            - time * (0.82f * pulse_speed));
    float ripple = radial_wave * radial_wave * ripple_gain;
    float fog_t = lab256_clamp01(warped_radius / max(fog_radius, 0.01f));
    fog_t = fog_t * fog_t * (3.0f - 2.0f * fog_t);
    float fog_envelope = 1.0f - fog_t;
    float broad_haze = 0.10f / (radius2 + 0.22f);
    float smoke = fog_envelope * (0.22f + ripple * 0.24f);

    float3 color = (float3)(0.30f, 0.34f, 0.39f) * (smoke * 0.78f)
        + (float3)(0.24f, 0.38f, 0.52f) * (fog_envelope * ripple * 0.24f)
        + (float3)(0.48f, 0.54f, 0.60f) * (broad_haze * 0.16f);

    // The mouse-authored reaction is a second visual layer in real surface
    // coordinates. A broad low-saturation response makes an area disturbance,
    // not a bright dot, while the faster B drain keeps the wake short.
    float trail_floor = max(0.020f, min(mean_v * 1.25f, 0.08f));
    float reaction_trail = lab256_clamp01((field - trail_floor) * 1.55f);
    reaction_trail = reaction_trail * (2.0f - reaction_trail);
    float trail_hot = lab256_clamp01((field - 0.25f) * 1.35f);
    float3 trail_color = mix(
        (float3)(0.34f, 0.39f, 0.44f),
        (float3)(0.42f, 0.54f, 0.63f),
        trail_hot);
    color += trail_color * (reaction_trail * 0.38f);

    float background_alpha = clamp(
        lab256_finite_or(
            as_float(control[LAB256_CTRL_BACKGROUND_ALPHA]),
            0.08f),
        0.0f,
        1.0f);
    float content_alpha = lab256_clamp01(
        smoke * 0.44f
            + broad_haze * 0.12f
            + reaction_trail * 0.34f);
    float output_alpha = mix(background_alpha, 1.0f, content_alpha);

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
