// TRUEOS Lab256 multi-phase compute exploration for Intel Xe-LP / ADL-S.
//
// This is intentionally a fixed 256x256 capability, not a general image API.
// One IGC zebin contains three entry points which the host dispatches in order:
//
//   1. lab256_step       packed Gray-Scott state A -> B
//   2. lab256_reduce     B -> compact per-lane telemetry report
//   3. lab256_composite  B + report + CPU control/history -> RGBA8
//
// The fixed extent makes the bounds, memory cost, and maximum shader work
// auditable. The host owns all buffers, validates the control page, and places
// a stalling HDC flush/invalidate boundary between passes.

#define LAB256_SIZE 256u
#define LAB256_PIXELS (LAB256_SIZE * LAB256_SIZE)

#define LAB256_CONTROL_MAGIC 0x4C414232u // "LAB2"
#define LAB256_CONTROL_VERSION 1u
#define LAB256_REPORT_MAGIC 0x4C325250u // "L2RP"

#define LAB256_FLAG_WRAP        (1u << 0)
#define LAB256_FLAG_INJECT      (1u << 1)
#define LAB256_FLAG_RESET       (1u << 2)
#define LAB256_FLAG_MANDELBROT  (1u << 3)
#define LAB256_FLAG_CHART       (1u << 4)
#define LAB256_FLAG_FLOW_WARP   (1u << 5)

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
#define LAB256_CTRL_MANDEL_X_F32    11u
#define LAB256_CTRL_MANDEL_Y_F32    12u
#define LAB256_CTRL_MANDEL_SCALE    13u
#define LAB256_CTRL_MANDEL_ITERS    14u
#define LAB256_CTRL_PALETTE_SEED    15u
#define LAB256_CTRL_HISTORY_HEAD    16u
#define LAB256_CTRL_BACKGROUND_ALPHA 17u // f32 [0, 1]
#define LAB256_CTRL_HISTORY_BASE    32u // 256 unsigned Q0.16 samples
#define LAB256_CONTROL_DWORDS      288u

// Report layout: 16 header words followed by one 24-dword stripe per SIMD
// lane. Each lane scans 4096 pixels, avoiding contended global atomics.
#define LAB256_REPORT_HEADER_DWORDS 16u
#define LAB256_REPORT_LANES         16u
#define LAB256_REPORT_STRIDE        24u
#define LAB256_REPORT_DWORDS \
    (LAB256_REPORT_HEADER_DWORDS + LAB256_REPORT_LANES * LAB256_REPORT_STRIDE)
#define LAB256_REPORT_HIST_BASE      7u

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
        ? lab256_seed(x, y, control[LAB256_CTRL_PALETTE_SEED])
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
    // A private uint[16] histogram makes this IGC build request a stateless
    // private-scratch base. The current direct-RCS lane deliberately has no
    // such implicit allocation. Fixed 256x256 work makes repeated scans a
    // clean trade: 16 lanes perform 16 bounded 4096-byte classification
    // passes and the host contract remains five explicit host-owned buffers.
    for (uint bin = 0u; bin < 16u; bin++) {
        uint count = 0u;
        for (uint index = lane; index < LAB256_PIXELS; index += LAB256_REPORT_LANES) {
            uint value = state[index] >> 24;
            count += (value >> 4) == bin;
        }
        report[base + LAB256_REPORT_HIST_BASE + bin] = count;
    }
    report[base + 23u] = 0xD06E0000u | lane;
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

static inline float3 lab256_palette(float value, float seed)
{
    const float tau = 6.28318530717958647692f;
    float3 phase = (float3)(0.00f, 0.33f, 0.67f) + seed;
    return 0.52f + 0.48f * native_cos(tau * (value + phase));
}

static inline uint lab256_histogram_count(__global const uint *report, uint bin)
{
    uint count = 0u;
    for (uint lane = 0u; lane < LAB256_REPORT_LANES; lane++) {
        uint base = LAB256_REPORT_HEADER_DWORDS + lane * LAB256_REPORT_STRIDE;
        count += report[base + LAB256_REPORT_HIST_BASE + bin];
    }
    return count;
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

static inline float lab256_mandel(
    float2 point,
    float2 center,
    float scale,
    uint iteration_cap)
{
    float2 c = center + point * scale;
    float2 z = (float2)(0.0f);
    uint iteration = 0u;
    for (; iteration < 96u; iteration++) {
        if (iteration >= iteration_cap) {
            break;
        }
        float xx = z.x * z.x;
        float yy = z.y * z.y;
        if (xx + yy > 16.0f) {
            break;
        }
        z = (float2)(xx - yy + c.x, 2.0f * z.x * z.y + c.y);
    }
    return iteration >= iteration_cap
        ? 0.0f
        : (float)iteration / (float)iteration_cap;
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

    float flow = native_sin(point.x * 8.1f + time * 0.73f + field * 5.0f)
        + native_sin(point.y * 9.7f - time * 0.51f - field * 3.0f)
        + native_sin((point.x + point.y) * 6.3f + time * 0.29f);
    flow = 0.5f + flow * (1.0f / 6.0f);
    float palette_seed = (float)(control[LAB256_CTRL_PALETTE_SEED] & 255u) * (1.0f / 255.0f);
    float color_value = lab256_clamp01(flow * 0.55f + field * 0.75f - mean_v * 0.20f);
    float3 color = lab256_palette(color_value, palette_seed);

    float edge = fabs(state_uv.x - state_uv.y);
    color *= 0.42f + field * 1.05f + lab256_clamp01(edge * 2.5f) * 0.42f;
    float fractal_presence = 0.0f;

    if ((flags & LAB256_FLAG_MANDELBROT) != 0u) {
        float center_x = lab256_finite_or(as_float(control[LAB256_CTRL_MANDEL_X_F32]), -0.62f);
        float center_y = lab256_finite_or(as_float(control[LAB256_CTRL_MANDEL_Y_F32]), 0.0f);
        float scale = clamp(
            lab256_finite_or(as_float(control[LAB256_CTRL_MANDEL_SCALE]), 1.55f),
            0.0002f,
            2.5f);
        uint iteration_cap = clamp(control[LAB256_CTRL_MANDEL_ITERS], 12u, 96u);
        float2 fractal_point = point;
        if ((flags & LAB256_FLAG_FLOW_WARP) != 0u) {
            fractal_point += (float2)(
                native_sin(field * 12.0f + time),
                native_cos(field * 9.0f - time * 0.7f)) * 0.018f;
        }
        float mandel = lab256_mandel(
            fractal_point,
            (float2)(center_x, center_y),
            scale,
            iteration_cap);
        float3 fractal_color = lab256_palette(mandel * 1.4f + field * 0.22f, palette_seed + 0.18f);
        float escaped = mandel > 0.0f ? 1.0f : 0.0f;
        fractal_presence = escaped * (0.28f + mandel * 0.72f);
        color = mix(color * (0.18f + field * 0.55f), fractal_color, escaped * 0.72f);
    }

    float radius2 = dot(point, point);
    color *= clamp(1.12f - radius2 * 0.34f, 0.42f, 1.0f);
    float background_alpha = clamp(
        lab256_finite_or(
            as_float(control[LAB256_CTRL_BACKGROUND_ALPHA]),
            0.08f),
        0.0f,
        1.0f);
    float content_alpha = lab256_clamp01(
        field * 1.12f
            + lab256_clamp01(edge * 2.4f) * 0.42f
            + fractal_presence * 0.72f);
    float output_alpha = mix(background_alpha, 1.0f, content_alpha);

    if ((flags & LAB256_FLAG_CHART) != 0u && y >= 192u) {
        output_alpha = max(output_alpha, 0.82f);
        float3 panel = (float3)(0.018f, 0.026f, 0.055f);
        color = mix(color, panel, 0.82f);
        if ((x & 31u) == 0u || ((y - 192u) & 15u) == 0u) {
            color = mix(color, (float3)(0.12f, 0.24f, 0.32f), 0.62f);
        }

        if (report[0] == LAB256_REPORT_MAGIC) {
            uint bin = x >> 4;
            uint count = lab256_histogram_count(report, bin);
            uint bar_height = min(52u, (count * 52u + 32767u) / 65536u);
            if (y + bar_height >= 250u) {
                float3 bar = lab256_palette((float)bin * (1.0f / 15.0f), palette_seed);
                color = mix(color, bar, 0.64f);
                output_alpha = max(output_alpha, 0.90f);
            }
        }

        uint history_head = control[LAB256_CTRL_HISTORY_HEAD] & 255u;
        uint history_index = (history_head + x) & 255u;
        float sample = (float)(control[LAB256_CTRL_HISTORY_BASE + history_index] & 0xFFFFu)
            * (1.0f / 65535.0f);
        float curve_y = 247.0f - sample * 48.0f;
        float curve_distance = fabs(((float)y + 0.5f) - curve_y);
        float glow = lab256_clamp01(1.0f - curve_distance / 5.0f);
        color = mix(color, (float3)(0.05f, 0.78f, 1.0f), glow * 0.48f);
        output_alpha = max(output_alpha, 0.82f + glow * 0.18f);
        if (curve_distance < 1.25f) {
            color = (float3)(0.82f, 0.98f, 1.0f);
            output_alpha = 1.0f;
        }
    }

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

    // Three pixels-as-LEDs make pass liveness visible without CPU text.
    if (y >= 7u && y < 13u && x >= 7u && x < 37u) {
        uint led = (x - 7u) / 10u;
        uint local_x = (x - 7u) % 10u;
        if (led < 3u && local_x < 6u) {
            float3 led_color = led == 0u
                ? (float3)(0.18f, 0.95f, 0.48f)
                : led == 1u
                    ? (float3)(0.18f, 0.72f, 1.0f)
                    : (float3)(0.95f, 0.34f, 0.78f);
            color = led_color;
            output_alpha = 1.0f;
        }
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_rgba[y * dst_pitch_pixels + x] = lab256_pack_rgba8(color, output_alpha);
}
