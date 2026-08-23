#ifndef TRUEOS_GPGPU_CLOUD_VOLUME_SAMPLING_HPP
#define TRUEOS_GPGPU_CLOUD_VOLUME_SAMPLING_HPP

#include "trueos_clcpp.hpp"

#pragma OPENCL EXTENSION cl_khr_fp16 : enable

namespace trueos::gpgpu::cloud_volume {

/// The browser reference uses RGBA16F 3D textures.  Keep the linear fallback
/// byte-for-byte compatible with that storage so enabling Intel sampler
/// messages later changes only how a sample is fetched, not scene ownership,
/// precision, JSON-derived parameters, or ping-pong allocation size.
inline constexpr uint rgba16_float_bytes_per_voxel = 8u;

static_assert(sizeof(half4) == rgba16_float_bytes_per_voxel,
              "cloud volume storage must remain RGBA16F/half4");

inline uint clamp_index(int value, uint extent)
{
    if (value <= 0) {
        return 0u;
    }
    const uint converted = static_cast<uint>(value);
    return min(converted, extent - 1u);
}

inline uint linear_index(uint x, uint y, uint z, uint width, uint height)
{
    return (z * height + y) * width + x;
}

inline float4 load_rgba16f_clamped(
    __global const half4 *volume,
    uint width,
    uint height,
    uint depth,
    int x,
    int y,
    int z)
{
    const uint cx = clamp_index(x, width);
    const uint cy = clamp_index(y, height);
    const uint cz = clamp_index(z, depth);
    return convert_float4(volume[linear_index(cx, cy, cz, width, height)]);
}

/// Software equivalent of normalized, clamp-to-edge, linear 3D texture
/// sampling.  The `coord * extent - 0.5` convention matches normalized texture
/// sampling at texel centers rather than interpolating between voxel corners.
///
/// This function is the compatibility bridge for the first TRUEOS cloud port:
/// direct-RCS can call it over a stateful/stateless linear buffer today.  A
/// later `image3d_t` path can replace this call with one sampler message while
/// preserving the same normalized coordinate contract.
inline float4 sample_rgba16f_linear_clamp(
    __global const half4 *volume,
    uint width,
    uint height,
    uint depth,
    float3 normalized)
{
    if (width == 0u || height == 0u || depth == 0u) {
        return (float4)(0.0f);
    }

    const float3 coord = clamp(normalized, (float3)(0.0f), (float3)(1.0f));
    const float3 texel = coord * convert_float3((uint3)(width, height, depth)) - 0.5f;
    const float3 base_f = floor(texel);
    const int3 base = convert_int3_rtn(base_f);
    const float3 fraction = texel - base_f;

    const float4 c000 = load_rgba16f_clamped(volume, width, height, depth, base.x, base.y, base.z);
    const float4 c100 = load_rgba16f_clamped(volume, width, height, depth, base.x + 1, base.y, base.z);
    const float4 c010 = load_rgba16f_clamped(volume, width, height, depth, base.x, base.y + 1, base.z);
    const float4 c110 = load_rgba16f_clamped(volume, width, height, depth, base.x + 1, base.y + 1, base.z);
    const float4 c001 = load_rgba16f_clamped(volume, width, height, depth, base.x, base.y, base.z + 1);
    const float4 c101 = load_rgba16f_clamped(volume, width, height, depth, base.x + 1, base.y, base.z + 1);
    const float4 c011 = load_rgba16f_clamped(volume, width, height, depth, base.x, base.y + 1, base.z + 1);
    const float4 c111 = load_rgba16f_clamped(volume, width, height, depth, base.x + 1, base.y + 1, base.z + 1);

    const float4 x00 = mix(c000, c100, fraction.x);
    const float4 x10 = mix(c010, c110, fraction.x);
    const float4 x01 = mix(c001, c101, fraction.x);
    const float4 x11 = mix(c011, c111, fraction.x);
    const float4 y0 = mix(x00, x10, fraction.y);
    const float4 y1 = mix(x01, x11, fraction.y);
    return mix(y0, y1, fraction.z);
}

} // namespace trueos::gpgpu::cloud_volume

#endif // TRUEOS_GPGPU_CLOUD_VOLUME_SAMPLING_HPP
