#pragma once

#include <cstdint>
#include <filesystem>
#include <vector>

namespace trueos::lfm25 {

struct native_decode_result {
    std::vector<std::uint32_t> next_tokens;
};

// Verify the independently owned, fixed-shape Intel Q8_0 projection kernel
// against the sealed layer-0 checkpoints captured from llama.cpp b10075.
void verify_q8_kernel(
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
    std::uint32_t generated_tokens,
    std::uint32_t threads);

} // namespace trueos::lfm25
