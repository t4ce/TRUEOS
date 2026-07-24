#pragma once

#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace trueos::lfm25 {

enum class native_projection_backend {
    cpu_avx2,
    intel_igc,
    intel_igc_packed,
};

struct native_decode_result {
    std::vector<std::uint32_t> next_tokens;
    std::vector<std::uint32_t> generated_tokens;
    std::string projection_device;
    std::string projection_platform;
    std::string projection_driver;
    std::string projection_il;
    std::string projection_weight_layout;
    std::string projection_program_binary_sha256;
    std::size_t projection_program_binary_bytes = 0;
    std::size_t projection_model_bytes = 0;
    std::uint64_t projection_subnormal_scales = 0;
    std::uint64_t projection_launches = 0;
    std::uint64_t projection_nanoseconds = 0;
    bool stopped = false;
};

// Verify the independently owned, fixed-shape Intel Q8_0 projection kernel
// against the sealed layer-0 checkpoints captured from llama.cpp b10075.
void verify_q8_kernel(
    const std::filesystem::path & native_image,
    const std::filesystem::path & model_contract,
    const std::filesystem::path & hi_golden,
    std::uint32_t threads);

// Verify the graph-native two-block/SIMD16 model packing independently of
// OpenCL.  This is the no-device gate for the future packed DP4A backend.
void verify_q8_packed_layout(
    const std::filesystem::path & native_image,
    const std::filesystem::path & model_contract,
    const std::filesystem::path & hi_golden,
    std::uint32_t threads);

// Run a complete fixed-model decode sequence from the sealed TRUEGA native
// image. Tokenization stays outside this math boundary.
native_decode_result run_native_decode(
    const std::filesystem::path & native_image,
    const std::filesystem::path & model_contract,
    const std::filesystem::path & f32_sidecar,
    const std::vector<std::uint32_t> & input_tokens,
    std::uint32_t max_reply_tokens,
    const std::vector<std::uint32_t> & stop_tokens,
    std::uint32_t threads,
    native_projection_backend backend,
    const std::filesystem::path & igc_spirv);

} // namespace trueos::lfm25
