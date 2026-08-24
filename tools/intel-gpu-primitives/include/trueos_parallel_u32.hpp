#ifndef TRUEOS_INTEL_GPU_PRIMITIVES_PARALLEL_U32_HPP
#define TRUEOS_INTEL_GPU_PRIMITIVES_PARALLEL_U32_HPP

// Freestanding C++ for OpenCL helpers for the TRUEOS parallel-u32 incubator.
// Keep this header device-only: no standard library, runtime allocation,
// exceptions, RTTI, static initialization, local memory, or callable symbols.

#if !defined(__OPENCL_CPP_VERSION__)
#error "TRUEOS GPU primitives require C++ for OpenCL (-x clcpp -cl-std=CLC++)"
#endif

#define TRUEOS_PARALLEL_REQD_SUB_GROUP_SIZE_16 \
    __attribute__((intel_reqd_sub_group_size(16)))

namespace trueos::gpgpu::parallel_u32 {

inline constexpr uint subgroup_width = 16u;
inline constexpr uint tile_rows = 16u;
inline constexpr uint tile_items = subgroup_width * tile_rows;
inline constexpr uint radix_bits = 4u;
inline constexpr uint radix_bins = 1u << radix_bits;
inline constexpr uint radix_mask = radix_bins - 1u;
inline constexpr uint no_head = 0xFFFFFFFFu;

static_assert(subgroup_width == 16u, "the incubator ABI is fixed to SIMD16");
static_assert(tile_items == 256u, "the incubator tile ABI is fixed to 256 items");
static_assert(radix_bins == subgroup_width,
              "one SIMD16 lane owns one four-bit radix bin");

inline uint tile_base(uint tile)
{
    return tile * tile_items;
}

inline uint row_index(uint tile, uint row, uint lane)
{
    return tile_base(tile) + row * subgroup_width + lane;
}

inline uint digit4(uint value, uint shift)
{
    return (value >> shift) & radix_mask;
}

inline uint valid_word(bool value)
{
    return value ? 1u : 0u;
}

} // namespace trueos::gpgpu::parallel_u32

#endif // TRUEOS_INTEL_GPU_PRIMITIVES_PARALLEL_U32_HPP
