#ifndef TRUEOS_GPGPU_CLCPP_HPP
#define TRUEOS_GPGPU_CLCPP_HPP

// Small, freestanding building blocks shared by TRUEOS C++ for OpenCL kernels.
//
// Keep this header device-only: no host C++ runtime, standard library,
// exceptions, RTTI, allocation, or static initialization. Everything here must
// disappear into SPIR-V during the offline bake.

#if !defined(__OPENCL_CPP_VERSION__)
#error "TRUEOS C++ kernels must be compiled as C++ for OpenCL (-x clcpp -cl-std=CLC++)"
#endif

#define TRUEOS_REQD_SUB_GROUP_SIZE_16 \
    __attribute__((intel_reqd_sub_group_size(16)))

namespace trueos::gpgpu {

inline constexpr uint rgba8_bytes_per_pixel = sizeof(uint);
inline constexpr uint copy_rect_pixels_per_work_item = 2u;

static_assert(rgba8_bytes_per_pixel == 4u,
              "TRUEOS linear RGBA8 kernels require 32-bit uint");

// Deliberately tiny proof that templates remain a source-level facility. This
// helper is inlined into the kernel and does not create a callable C++ runtime
// symbol or change the entry-point ABI.
template <typename Element>
inline void copy_element(
    __global const Element *source,
    __global Element *destination,
    uint source_index,
    uint destination_index)
{
    destination[destination_index] = source[source_index];
}

} // namespace trueos::gpgpu

#endif // TRUEOS_GPGPU_CLCPP_HPP
