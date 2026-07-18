// TRUEOS Gen12/Alder Lake GPGPU skybox sampler.
//
// Contract:
// - Source is an equirectangular RGB565 skybox, one ushort per pixel.
// - Destination is a linear TRUEOS RGBA8 buffer packed as AABBGGRR in u32.
// - One SIMD16 work item covers up to 16 adjacent output pixels.
// - A single 2D walker covers the whole target rect.

static inline uint rgba_from_rgb565(ushort pixel)
{
    uint r5 = ((uint)pixel >> 11) & 31u;
    uint g6 = ((uint)pixel >> 5) & 63u;
    uint b5 = (uint)pixel & 31u;
    uint r = (r5 << 3) | (r5 >> 2);
    uint g = (g6 << 2) | (g6 >> 4);
    uint b = (b5 << 3) | (b5 >> 2);
    return 0xFF000000u | (b << 16) | (g << 8) | r;
}

static inline float abs_f32(float value)
{
    return value < 0.0f ? -value : value;
}

static inline float clamp_f32(float value, float lo, float hi)
{
    return value < lo ? lo : (value > hi ? hi : value);
}

static inline uint clamp_u32_255(uint value)
{
    return value > 255u ? 255u : value;
}

static inline int floor_i32(float value)
{
    int i = (int)value;
    return ((float)i > value) ? i - 1 : i;
}

static inline float fast_atan_unit(float z)
{
    float az = abs_f32(z);
    return z * (0.7853981633974483f + 0.273f * (1.0f - az));
}

static inline float fast_atan2_f32(float y, float x)
{
    float ay = abs_f32(y);
    float ax = abs_f32(x);
    if (ax + ay < 0.0000001f) {
        return 0.0f;
    }

    float angle;
    if (ax >= ay) {
        angle = fast_atan_unit(y / (x + (x >= 0.0f ? 0.0000001f : -0.0000001f)));
        if (x < 0.0f) {
            angle += y >= 0.0f ? 3.1415926535897932f : -3.1415926535897932f;
        }
    } else {
        angle = 1.5707963267948966f - fast_atan_unit(x / (y + (y >= 0.0f ? 0.0000001f : -0.0000001f)));
        if (y < 0.0f) {
            angle -= 3.1415926535897932f;
        }
    }
    return angle;
}

static inline float fast_asin_f32(float z)
{
    float cz = clamp_f32(z, -1.0f, 1.0f);
    float one_minus = 1.0f - cz * cz;
    float root = one_minus <= 0.0f ? 0.0f : one_minus * rsqrt(one_minus);
    return fast_atan2_f32(cz, root);
}

static inline uint bilerp_rgb565(
    __global const ushort *skybox,
    uint pitch_pixels,
    uint width,
    uint height,
    float u,
    float v)
{
    float fx = u * (float)width - 0.5f;
    float fy = v * (float)height - 0.5f;
    int x0 = floor_i32(fx);
    int y0 = floor_i32(fy);
    float tx = fx - (float)x0;
    float ty = fy - (float)y0;

    int x0w = x0 % (int)width;
    if (x0w < 0) {
        x0w += (int)width;
    }
    int x1w = x0w + 1;
    if (x1w >= (int)width) {
        x1w = 0;
    }
    int y0c = clamp(y0, 0, (int)height - 1);
    int y1c = clamp(y0 + 1, 0, (int)height - 1);

    ushort c00 = skybox[(uint)y0c * pitch_pixels + (uint)x0w];
    ushort c10 = skybox[(uint)y0c * pitch_pixels + (uint)x1w];
    ushort c01 = skybox[(uint)y1c * pitch_pixels + (uint)x0w];
    ushort c11 = skybox[(uint)y1c * pitch_pixels + (uint)x1w];

    float r00 = (float)(((uint)c00 >> 11) & 31u) * (255.0f / 31.0f);
    float g00 = (float)(((uint)c00 >> 5) & 63u) * (255.0f / 63.0f);
    float b00 = (float)((uint)c00 & 31u) * (255.0f / 31.0f);
    float r10 = (float)(((uint)c10 >> 11) & 31u) * (255.0f / 31.0f);
    float g10 = (float)(((uint)c10 >> 5) & 63u) * (255.0f / 63.0f);
    float b10 = (float)((uint)c10 & 31u) * (255.0f / 31.0f);
    float r01 = (float)(((uint)c01 >> 11) & 31u) * (255.0f / 31.0f);
    float g01 = (float)(((uint)c01 >> 5) & 63u) * (255.0f / 63.0f);
    float b01 = (float)((uint)c01 & 31u) * (255.0f / 31.0f);
    float r11 = (float)(((uint)c11 >> 11) & 31u) * (255.0f / 31.0f);
    float g11 = (float)(((uint)c11 >> 5) & 63u) * (255.0f / 63.0f);
    float b11 = (float)((uint)c11 & 31u) * (255.0f / 31.0f);

    float omt_x = 1.0f - tx;
    float omt_y = 1.0f - ty;
    float r = (r00 * omt_x + r10 * tx) * omt_y + (r01 * omt_x + r11 * tx) * ty;
    float g = (g00 * omt_x + g10 * tx) * omt_y + (g01 * omt_x + g11 * tx) * ty;
    float b = (b00 * omt_x + b10 * tx) * omt_y + (b01 * omt_x + b11 * tx) * ty;

    uint ru = clamp_u32_255((uint)(r + 0.5f));
    uint gu = clamp_u32_255((uint)(g + 0.5f));
    uint bu = clamp_u32_255((uint)(b + 0.5f));
    return 0xFF000000u | (bu << 16) | (gu << 8) | ru;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void skybox_sample_rgb565(
    __global const ushort *skybox_rgb565,
    __global uint *dst_rgba,
    uint sky_pitch_bytes,
    uint sky_width,
    uint sky_height,
    uint dst_pitch_bytes,
    uint dst_width,
    uint dst_height,
    uint rect_x,
    uint rect_y,
    uint rect_width,
    uint rect_height,
    float right_x,
    float right_y,
    float right_z,
    float up_x,
    float up_y,
    float up_z,
    float forward_x,
    float forward_y,
    float forward_z,
    float aspect_tan_half_fov_y,
    float tan_half_fov_y)
{
    uint x = get_global_id(0);
    uint y = get_global_id(1);

    if (x >= rect_width || y >= rect_height || sky_width == 0u || sky_height == 0u) {
        return;
    }
    if (rect_x >= dst_width || rect_y >= dst_height) {
        return;
    }
    if (x >= dst_width - rect_x || y >= dst_height - rect_y) {
        return;
    }

    float camera_x = (2.0f * (((float)x + 0.5f) / (float)rect_width) - 1.0f)
        * aspect_tan_half_fov_y;
    float camera_y = (1.0f - 2.0f * (((float)y + 0.5f) / (float)rect_height))
        * tan_half_fov_y;

    float dx = forward_x + right_x * camera_x + up_x * camera_y;
    float dy = forward_y + right_y * camera_x + up_y * camera_y;
    float dz = forward_z + right_z * camera_x + up_z * camera_y;
    float inv_len = rsqrt(dx * dx + dy * dy + dz * dz);
    dx *= inv_len;
    dy *= inv_len;
    dz *= inv_len;

    float u = fast_atan2_f32(dx, dy) * (0.5f / 3.14159265358979323846f) + 0.5f;
    float v = 0.5f - fast_asin_f32(dz) * (1.0f / 3.14159265358979323846f);

    uint sky_pitch_pixels = sky_pitch_bytes >> 1;
    uint pixel = bilerp_rgb565(skybox_rgb565, sky_pitch_pixels, sky_width, sky_height, u, v);
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    dst_rgba[(rect_y + y) * dst_pitch_pixels + rect_x + x] = pixel;
}
