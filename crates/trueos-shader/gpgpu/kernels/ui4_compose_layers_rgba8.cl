// TRUEOS UI4 single-dispatch layer compositor for Gen12/Alder Lake.
//
// Contract:
// - Base and destination are linear XRGB8888/RGBA8 surfaces.
// - Layer sources are linear premultiplied RGBA8 (AABBGGRR in a u32).
// - One 2D SIMD16 walker owns every pixel in the damage rectangle exactly once.
// - Every source GPU virtual address is carried by the immutable layer table.
//   The host therefore never changes a bindful source between ordered walkers.
// - Layers are axis-aligned, nearest sampled, and applied in table/z order.

#define UI4_LAYER_DWORDS 12u
#define UI4_COMPOSE_FLAG_BASE_XRGB (1u << 0)
#define UI4_COMPOSE_FLAG_DEST_XRGB (1u << 1)

static inline uint div255(uint value)
{
    return (value + 127u) / 255u;
}

static inline uint xrgb_to_rgba(uint xrgb)
{
    return 0xFF000000u
        | ((xrgb >> 16) & 0xFFu)
        | (xrgb & 0x0000FF00u)
        | ((xrgb & 0xFFu) << 16);
}

static inline uint rgba_to_xrgb(uint rgba)
{
    return ((rgba & 0xFFu) << 16)
        | (rgba & 0x0000FF00u)
        | ((rgba >> 16) & 0xFFu);
}

static inline uint apply_opacity(uint src, uint opacity)
{
    if (opacity >= 255u) {
        return src;
    }
    uint r = div255((src & 0xFFu) * opacity);
    uint g = div255(((src >> 8) & 0xFFu) * opacity);
    uint b = div255(((src >> 16) & 0xFFu) * opacity);
    uint a = div255(((src >> 24) & 0xFFu) * opacity);
    return (a << 24) | (b << 16) | (g << 8) | r;
}

static inline uint premul_src_over(uint src, uint dst)
{
    uint sa = (src >> 24) & 0xFFu;
    if (sa == 0u) {
        return dst;
    }
    if (sa == 255u) {
        return src | 0xFF000000u;
    }

    uint inv = 255u - sa;
    uint r = min((src & 0xFFu) + div255((dst & 0xFFu) * inv), 255u);
    uint g = min(((src >> 8) & 0xFFu) + div255(((dst >> 8) & 0xFFu) * inv), 255u);
    uint b = min(((src >> 16) & 0xFFu) + div255(((dst >> 16) & 0xFFu) * inv), 255u);
    uint a = min(sa + div255(((dst >> 24) & 0xFFu) * inv), 255u);
    return (a << 24) | (b << 16) | (g << 8) | r;
}

__attribute__((intel_reqd_sub_group_size(16)))
__kernel void ui4_compose_layers_rgba8(
    __global const uint *base_xrgb,
    __global uint *dst_rgba,
    __global const uint *layers,
    uint base_pitch_bytes,
    uint dst_pitch_bytes,
    uint dst_width,
    uint dst_height,
    uint damage_x,
    uint damage_y,
    uint damage_width,
    uint damage_height,
    uint layer_count,
    uint flags)
{
    uint local_x = get_global_id(0);
    uint local_y = get_global_id(1);
    if (local_x >= damage_width || local_y >= damage_height
        || damage_x >= dst_width || damage_y >= dst_height
        || local_x >= dst_width - damage_x || local_y >= dst_height - damage_y) {
        return;
    }

    uint x = damage_x + local_x;
    uint y = damage_y + local_y;
    uint dst_pitch_pixels = dst_pitch_bytes >> 2;
    uint color = 0u;
    if ((flags & UI4_COMPOSE_FLAG_BASE_XRGB) != 0u) {
        uint base_pitch_pixels = base_pitch_bytes >> 2;
        color = xrgb_to_rgba(base_xrgb[y * base_pitch_pixels + x]);
    }

    __attribute__((opencl_unroll_hint(1)))
    for (uint layer_index = 0u; layer_index < layer_count; ++layer_index) {
        uint d = layer_index * UI4_LAYER_DWORDS;
        ulong src_gpu = ((ulong)layers[d + 1u] << 32) | (ulong)layers[d + 0u];
        uint src_pitch_pixels = layers[d + 2u] >> 2;
        uint src_width = layers[d + 3u];
        uint src_height = layers[d + 4u];
        int dst_x = as_int(layers[d + 5u]);
        int dst_y = as_int(layers[d + 6u]);
        uint layer_width = layers[d + 7u];
        uint layer_height = layers[d + 8u];
        uint opacity = min(layers[d + 9u], 255u);

        int rel_x = (int)x - dst_x;
        int rel_y = (int)y - dst_y;
        if (rel_x < 0 || rel_y < 0 || (uint)rel_x >= layer_width
            || (uint)rel_y >= layer_height || src_width == 0u || src_height == 0u
            || layer_width == 0u || layer_height == 0u) {
            continue;
        }

        uint sx = min(((uint)rel_x * src_width) / layer_width, src_width - 1u);
        uint sy = min(((uint)rel_y * src_height) / layer_height, src_height - 1u);
        __global const uint *src_rgba = (__global const uint *)src_gpu;
        uint src = apply_opacity(src_rgba[sy * src_pitch_pixels + sx], opacity);
        color = premul_src_over(src, color);
    }

    uint output = (flags & UI4_COMPOSE_FLAG_DEST_XRGB) != 0u
        ? rgba_to_xrgb(color)
        : color;
    dst_rgba[y * dst_pitch_pixels + x] = output;
}
