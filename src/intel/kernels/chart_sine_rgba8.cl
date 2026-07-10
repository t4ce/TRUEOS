// TRUEOS Gen12/Alder Lake 2D chart bring-up kernel.
//
// Contract:
// - Destination is a linear TRUEOS RGBA8 buffer packed as AABBGGRR in u32.
// - One 2D SIMD16 walker covers the requested chart rectangle.
// - The kernel owns the complete pixel so no CPU clear/upload or 3D tessellation is needed.
// - `flags`: bit0 minor/major grid, bit1 axes, bit2 curve glow, bit3 plot border.

#define CHART_FLAG_GRID   (1u << 0)
#define CHART_FLAG_AXES   (1u << 1)
#define CHART_FLAG_GLOW   (1u << 2)
#define CHART_FLAG_BORDER (1u << 3)

static inline float clamp01(float value)
{
    return clamp(value, 0.0f, 1.0f);
}

static inline uint mix_rgba8(uint under, uint over, float amount)
{
    float a = clamp01(amount);
    float ia = 1.0f - a;
    uint r = (uint)((float)(under & 255u) * ia + (float)(over & 255u) * a + 0.5f);
    uint g = (uint)((float)((under >> 8) & 255u) * ia
        + (float)((over >> 8) & 255u) * a + 0.5f);
    uint b = (uint)((float)((under >> 16) & 255u) * ia
        + (float)((over >> 16) & 255u) * a + 0.5f);
    return 0xFF000000u | (b << 16) | (g << 8) | r;
}

static inline float chart_curve_y(float x01, float phase, float cycles, float amplitude)
{
    const float tau = 6.28318530717958647692f;
    return 0.5f - clamp(amplitude, 0.0f, 0.48f) * native_sin(tau * cycles * x01 + phase);
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void chart_sine_rgba8(
    __global uint *dst_rgba,
    uint dst_pitch_bytes,
    uint dst_width,
    uint dst_height,
    uint rect_x,
    uint rect_y,
    uint rect_width,
    uint rect_height,
    float phase,
    float cycles,
    float amplitude,
    float line_width_px,
    uint background_rgba,
    uint minor_grid_rgba,
    uint major_grid_rgba,
    uint axis_rgba,
    uint line_rgba,
    uint glow_rgba,
    uint flags)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);

    if (x >= rect_width || y >= rect_height || rect_width == 0u || rect_height == 0u) {
        return;
    }
    if (rect_x >= dst_width || rect_y >= dst_height
        || x >= dst_width - rect_x || y >= dst_height - rect_y) {
        return;
    }

    uint color = background_rgba | 0xFF000000u;
    uint margin_x = max(24u, rect_width / 32u);
    uint margin_y = max(24u, rect_height / 18u);
    uint plot_left = min(margin_x, rect_width - 1u);
    uint plot_top = min(margin_y, rect_height - 1u);
    uint plot_right = rect_width > margin_x ? rect_width - margin_x - 1u : plot_left;
    uint plot_bottom = rect_height > margin_y ? rect_height - margin_y - 1u : plot_top;
    bool in_plot = x >= plot_left && x <= plot_right && y >= plot_top && y <= plot_bottom;

    if (in_plot) {
        uint plot_width = max(1u, plot_right - plot_left + 1u);
        uint plot_height = max(1u, plot_bottom - plot_top + 1u);
        uint px = x - plot_left;
        uint py = y - plot_top;

        if ((flags & CHART_FLAG_GRID) != 0u) {
            uint minor_x = max(16u, plot_width / 40u);
            uint minor_y = max(16u, plot_height / 20u);
            bool gx = (px % minor_x) == 0u;
            bool gy = (py % minor_y) == 0u;
            bool major_x = (px % (minor_x * 5u)) == 0u;
            bool major_y = (py % (minor_y * 5u)) == 0u;
            if (major_x || major_y) {
                color = major_grid_rgba | 0xFF000000u;
            } else if (gx || gy) {
                color = minor_grid_rgba | 0xFF000000u;
            }
        }

        if ((flags & CHART_FLAG_AXES) != 0u) {
            uint center_y = plot_height / 2u;
            uint center_x = plot_width / 2u;
            if (abs((int)py - (int)center_y) <= 1 || abs((int)px - (int)center_x) <= 1) {
                color = axis_rgba | 0xFF000000u;
            }
        }

        float x01 = ((float)px + 0.5f) / (float)plot_width;
        float curve_y = chart_curve_y(x01, phase, cycles, amplitude) * (float)(plot_height - 1u);
        float distance = fabs(((float)py + 0.5f) - curve_y);
        float width = clamp(line_width_px, 0.75f, 8.0f);

        if ((flags & CHART_FLAG_GLOW) != 0u) {
            float glow = 1.0f - clamp01((distance - width) / max(1.0f, width * 4.0f));
            color = mix_rgba8(color, glow_rgba, glow * 0.55f);
        }
        float core = 1.0f - clamp01((distance - width * 0.45f) / max(0.75f, width));
        color = mix_rgba8(color, line_rgba, core);
    }

    if ((flags & CHART_FLAG_BORDER) != 0u
        && ((x == plot_left || x == plot_right) && y >= plot_top && y <= plot_bottom
            || (y == plot_top || y == plot_bottom) && x >= plot_left && x <= plot_right)) {
        color = major_grid_rgba | 0xFF000000u;
    }

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_rgba[(rect_y + y) * dst_pitch_pixels + rect_x + x] = color;
}
