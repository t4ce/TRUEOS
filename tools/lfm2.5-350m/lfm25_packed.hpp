#pragma once

#include <cstddef>
#include <cstdint>
#include <span>
#include <string>
#include <vector>

namespace trueos::lfm25 {

// Fixed descriptor subset needed by the graph-native Q8 packer.  The packed
// image deliberately preserves every tensor's sealed offset and byte extent.
struct packed_q8_tensor_spec {
    std::uint32_t offset;
    std::uint32_t columns;
    std::uint32_t rows;
};

struct packed_q8_model {
    std::vector<std::byte> bytes;
    std::uint64_t tensor_count = 0;
    std::uint64_t block_tiles = 0;
    std::uint64_t quantized_values = 0;
    std::uint64_t subnormal_scales = 0;
    std::string sha256;
};

// Repack every admitted Q8 matrix into the two-block, sixteen-row layout used
// by lfm25_q8_project_packed.  Non-Q8 bytes and gaps are copied unchanged.
packed_q8_model pack_q8x16_model(
    std::span<const std::byte> native,
    std::span<const packed_q8_tensor_spec> tensors);

// Convert the existing 34-byte host Q8 activation blocks into the split
// graph-native ABI: uint scale_slot[blocks], uint qwords[blocks][8].
std::vector<std::uint32_t> pack_q8x16_activation(
    std::span<const std::byte> native_q8,
    std::uint32_t columns);

// CPU contract oracle for the packed image.  It retains the model's eight
// independent FMA accumulators and exact final reduction tree.
std::vector<float> project_q8x16_reference(
    std::span<const std::byte> packed_model,
    const packed_q8_tensor_spec & tensor,
    std::span<const std::uint32_t> packed_activation);

} // namespace trueos::lfm25
