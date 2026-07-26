#pragma once

#include "lfm25_packed.hpp"

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <span>
#include <string>
#include <vector>

namespace trueos::lfm25 {

enum class intel_igc_weight_layout {
    native_q8_0,
    packed_q8x16_pair,
};

// Host owner for the published C++ -> SPIR-V -> Intel IGC projection kernel.
// The implementation deliberately selects an Intel GPU and exposes no generic
// OpenCL program/source compilation surface.
class intel_igc_projector {
  public:
    intel_igc_projector(
        const std::filesystem::path & spirv_path,
        const void * native_weights,
        std::size_t native_weight_bytes,
        intel_igc_weight_layout layout = intel_igc_weight_layout::native_q8_0,
        std::span<const packed_q8_tensor_spec> packed_tensors = {});
    ~intel_igc_projector();

    intel_igc_projector(const intel_igc_projector &) = delete;
    intel_igc_projector & operator=(const intel_igc_projector &) = delete;

    std::vector<float> project(
        std::uint32_t weight_offset,
        std::uint32_t columns,
        std::uint32_t rows,
        std::span<const std::byte> q8_activation);

    const std::string & device_name() const;
    const std::string & platform_name() const;
    const std::string & driver_version() const;
    const std::string & il_version() const;
    std::size_t program_binary_bytes() const;
    const std::string & program_binary_sha256() const;
    std::uint64_t launches() const;
    std::uint64_t kernel_nanoseconds() const;
    std::uint64_t projected_weight_bytes() const;
    const std::string & weight_layout() const;
    std::size_t resident_model_bytes() const;
    std::uint64_t packed_subnormal_scales() const;
    const std::string & packed_model_sha256() const;

  private:
    struct implementation;
    std::unique_ptr<implementation> implementation_;
};

} // namespace trueos::lfm25
