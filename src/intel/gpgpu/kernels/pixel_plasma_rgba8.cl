// TRUEOS Gen12/Alder Lake procedural pixel-shader bring-up kernel.
//
// Contract:
// - Palette arguments use TRUEOS AABBGGRR u32 packing.
// - Destination is native premultiplied ARGB8888 plane memory (LE B, G, R, A).
// - One 2D SIMD16 walker covers the requested rectangle.
// - Every work-item owns one complete pixel; no tessellation or 3D pipeline is used.
// - `flags`: bit0 vignette, bit1 radial interference, bit2 subtle scanlines,
//   bit3 FluidX3D-style scalar-field palette, bit4 premultiplied plane alpha.

#define PIXEL_FLAG_VIGNETTE (1u << 0)
#define PIXEL_FLAG_RINGS    (1u << 1)
#define PIXEL_FLAG_SCANLINE (1u << 2)
#define PIXEL_FLAG_FIELD_PALETTE (1u << 3)
#define PIXEL_FLAG_ALPHA         (1u << 4)

static inline float clamp01(float value)
{
    return clamp(value, 0.0f, 1.0f);
}

static inline uchar color_channel(uint rgba, uint shift)
{
    return (uchar)((rgba >> shift) & 255u);
}

static inline uint palette_mix(uint low_rgba, uint high_rgba, float amount)
{
    float a = clamp01(amount);
    float ia = 1.0f - a;
    uint r = (uint)((float)color_channel(low_rgba, 0u) * ia
        + (float)color_channel(high_rgba, 0u) * a + 0.5f);
    uint g = (uint)((float)color_channel(low_rgba, 8u) * ia
        + (float)color_channel(high_rgba, 8u) * a + 0.5f);
    uint b = (uint)((float)color_channel(low_rgba, 16u) * ia
        + (float)color_channel(high_rgba, 16u) * a + 0.5f);
    return 0xFF000000u | (b << 16) | (g << 8) | r;
}

static inline uint shade_rgb(uint rgba, float amount)
{
    float a = clamp(amount, 0.0f, 2.0f);
    uint r = min(255u, (uint)((float)color_channel(rgba, 0u) * a + 0.5f));
    uint g = min(255u, (uint)((float)color_channel(rgba, 8u) * a + 0.5f));
    uint b = min(255u, (uint)((float)color_channel(rgba, 16u) * a + 0.5f));
    return 0xFF000000u | (b << 16) | (g << 8) | r;
}

// Piecewise scientific scalar-field palette, shaped after the compact
// velocity visualization used by FluidX3D's OpenCL graphics kernels.
static inline uint field_palette(float value)
{
    float x = clamp(6.0f * (1.0f - value), 0.0f, 6.0f);
    float r = 0.0f;
    float g = 0.0f;
    float b = 0.0f;
    if (x < 1.2f) {
        r = 1.0f;
        g = x * 0.83333333f;
    } else if (x < 2.0f) {
        r = 2.5f - x * 1.25f;
        g = 1.0f;
    } else if (x < 3.0f) {
        g = 1.0f;
        b = x - 2.0f;
    } else if (x < 4.0f) {
        g = 4.0f - x;
        b = 1.0f;
    } else if (x < 5.0f) {
        r = x * 0.4f - 1.6f;
        b = 3.0f - x * 0.5f;
    } else {
        r = 2.4f - x * 0.4f;
        b = 3.0f - x * 0.5f;
    }
    uint ri = (uint)(clamp01(r) * 255.0f + 0.5f);
    uint gi = (uint)(clamp01(g) * 255.0f + 0.5f);
    uint bi = (uint)(clamp01(b) * 255.0f + 0.5f);
    return 0xFF000000u | (bi << 16) | (gi << 8) | ri;
}

static inline uint plane_argb_premultiplied(uint rgba, float opacity)
{
    float a = clamp01(opacity);
    uint ai = (uint)(a * 255.0f + 0.5f);
    uint r = (uint)((float)color_channel(rgba, 0u) * a + 0.5f);
    uint g = (uint)((float)color_channel(rgba, 8u) * a + 0.5f);
    uint b = (uint)((float)color_channel(rgba, 16u) * a + 0.5f);
    // Native ARGB8888 plane word; little-endian memory is B, G, R, A.
    return (ai << 24) | (r << 16) | (g << 8) | b;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void pixel_plasma_rgba8(
    __global uint *dst_rgba,
    uint dst_pitch_bytes,
    uint dst_width,
    uint dst_height,
    uint rect_x,
    uint rect_y,
    uint rect_width,
    uint rect_height,
    float time,
    float spatial_scale,
    float intensity,
    uint low_rgba,
    uint mid_rgba,
    uint high_rgba,
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

    float width = (float)max(rect_width, 1u);
    float height = (float)max(rect_height, 1u);
    float aspect = width / height;
    float px = (((float)x + 0.5f) / width - 0.5f) * 2.0f * aspect;
    float py = (((float)y + 0.5f) / height - 0.5f) * 2.0f;
    float scale = clamp(spatial_scale, 0.25f, 8.0f);

    float wave = native_sin((px * 2.7f + time * 0.85f) * scale);
    wave += native_sin((py * 3.1f - time * 0.63f) * scale);
    wave += native_sin(((px + py) * 2.2f + time * 0.47f) * scale);
    if ((flags & PIXEL_FLAG_RINGS) != 0u) {
        float radius = native_sqrt(px * px + py * py);
        wave += native_sin((radius * 5.3f - time * 1.15f) * scale);
        wave *= 0.25f;
    } else {
        wave *= 0.33333334f;
    }

    float value = clamp01(0.5f + 0.5f * wave);
    uint color;
    if ((flags & PIXEL_FLAG_FIELD_PALETTE) != 0u) {
        color = field_palette(value);
    } else {
        color = value < 0.5f
            ? palette_mix(low_rgba, mid_rgba, value * 2.0f)
            : palette_mix(mid_rgba, high_rgba, (value - 0.5f) * 2.0f);
    }

    float shade = clamp(intensity, 0.25f, 2.0f);
    if ((flags & PIXEL_FLAG_VIGNETTE) != 0u) {
        float radius2 = px * px * 0.32f + py * py * 0.55f;
        shade *= clamp(1.12f - radius2 * 0.42f, 0.35f, 1.0f);
    }
    if ((flags & PIXEL_FLAG_SCANLINE) != 0u && (y & 3u) == 0u) {
        shade *= 0.88f;
    }
    color = shade_rgb(color, shade);
    float opacity = (flags & PIXEL_FLAG_ALPHA) != 0u
        ? 0.72f + 0.28f * value
        : 1.0f;
    color = plane_argb_premultiplied(color, opacity);

    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_rgba[(rect_y + y) * dst_pitch_pixels + rect_x + x] = color;
}
