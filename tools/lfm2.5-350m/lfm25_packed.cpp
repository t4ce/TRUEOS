#include "lfm25_packed.hpp"

#include <openssl/evp.h>

#include <array>
#include <cmath>
#include <cstring>
#include <immintrin.h>
#include <limits>
#include <memory>
#include <stdexcept>
#include <string>

namespace trueos::lfm25 {
namespace {

constexpr std::size_t kHidden = 1'024;
constexpr std::size_t kFfn = 4'608;
constexpr std::size_t kVocabulary = 65'536;
constexpr std::size_t kQ8Values = 32;
constexpr std::size_t kNativeBlockBytes = 34;
constexpr std::size_t kRowsPerTile = 16;
constexpr std::size_t kBlocksPerPair = 2;
constexpr std::size_t kScaleBytesPerBlockTile =
    kRowsPerTile * sizeof(std::uint16_t);
constexpr std::size_t kWeightBytesPerBlockTile =
    kRowsPerTile * kQ8Values;
constexpr std::size_t kPackedPairBytes =
    kBlocksPerPair
    * (kScaleBytesPerBlockTile + kWeightBytesPerBlockTile);
constexpr std::size_t kDotWords = 8;

static_assert(kPackedPairBytes == 1'088);
static_assert(kPackedPairBytes % 64 == 0);

bool admitted_shape(std::uint32_t columns, std::uint32_t rows) {
    if (columns == kHidden) {
        return rows == 512 ||
               rows == kHidden ||
               rows == 3'072 ||
               rows == kFfn ||
               rows == kVocabulary;
    }
    return columns == kFfn && rows == kHidden;
}

std::uint16_t load_u16(const std::byte * source) {
    std::uint16_t value = 0;
    std::memcpy(&value, source, sizeof(value));
    return value;
}

std::uint32_t load_u32(const std::byte * source) {
    std::uint32_t value = 0;
    std::memcpy(&value, source, sizeof(value));
    return value;
}

void store_u16(std::byte * destination, std::uint16_t value) {
    std::memcpy(destination, &value, sizeof(value));
}

void store_u32(std::byte * destination, std::uint32_t value) {
    std::memcpy(destination, &value, sizeof(value));
}

float f16_to_f32(std::uint16_t value) {
    return _cvtsh_ss(value);
}

std::int32_t signed_dot4(std::uint32_t left, std::uint32_t right) {
    std::int32_t result = 0;
    for (unsigned byte = 0; byte < 4; ++byte) {
        const auto left_value = static_cast<std::int8_t>(
            static_cast<std::uint8_t>(left >> (byte * 8)));
        const auto right_value = static_cast<std::int8_t>(
            static_cast<std::uint8_t>(right >> (byte * 8)));
        result +=
            static_cast<std::int32_t>(left_value)
            * static_cast<std::int32_t>(right_value);
    }
    return result;
}

std::string sha256(std::span<const std::byte> bytes) {
    std::unique_ptr<EVP_MD_CTX, decltype(&EVP_MD_CTX_free)> context(
        EVP_MD_CTX_new(), EVP_MD_CTX_free);
    if (!context ||
        EVP_DigestInit_ex(context.get(), EVP_sha256(), nullptr) != 1 ||
        EVP_DigestUpdate(context.get(), bytes.data(), bytes.size()) != 1) {
        throw std::runtime_error("cannot hash packed Q8 model");
    }
    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_bytes = 0;
    if (EVP_DigestFinal_ex(context.get(), digest.data(), &digest_bytes) != 1 ||
        digest_bytes != 32) {
        throw std::runtime_error("cannot finalize packed Q8 model hash");
    }
    constexpr char digits[] = "0123456789abcdef";
    std::string result(digest_bytes * 2, '\0');
    for (unsigned int index = 0; index < digest_bytes; ++index) {
        result[index * 2] = digits[digest[index] >> 4];
        result[index * 2 + 1] = digits[digest[index] & 0x0f];
    }
    return result;
}

void validate_tensor(
    std::size_t model_bytes,
    const packed_q8_tensor_spec & tensor)
{
    if (!admitted_shape(tensor.columns, tensor.rows) ||
        tensor.rows % kRowsPerTile != 0 ||
        tensor.columns % (kQ8Values * kBlocksPerPair) != 0 ||
        tensor.offset % 64 != 0) {
        throw std::runtime_error("packed Q8 tensor shape or alignment rejected");
    }
    const std::size_t blocks = tensor.columns / kQ8Values;
    const std::size_t tensor_bytes =
        static_cast<std::size_t>(tensor.rows)
        * blocks
        * kNativeBlockBytes;
    if (tensor.offset > model_bytes ||
        tensor_bytes > model_bytes - tensor.offset) {
        throw std::runtime_error("packed Q8 tensor storage rejected");
    }
}

} // namespace

packed_q8_model pack_q8x16_model(
    std::span<const std::byte> native,
    std::span<const packed_q8_tensor_spec> tensors)
{
    if (native.empty() || tensors.empty()) {
        throw std::runtime_error("empty packed Q8 model input");
    }

    packed_q8_model result;
    result.bytes.assign(native.begin(), native.end());
    for (const packed_q8_tensor_spec & tensor : tensors) {
        validate_tensor(native.size(), tensor);
        const std::size_t blocks = tensor.columns / kQ8Values;
        const std::size_t pairs = blocks / kBlocksPerPair;
        const std::size_t row_bytes = blocks * kNativeBlockBytes;
        const std::size_t row_tiles = tensor.rows / kRowsPerTile;

        for (std::size_t row_tile = 0; row_tile < row_tiles; ++row_tile) {
            for (std::size_t pair = 0; pair < pairs; ++pair) {
                const std::size_t destination_pair =
                    tensor.offset
                    + (row_tile * pairs + pair) * kPackedPairBytes;
                for (
                    std::size_t block_in_pair = 0;
                    block_in_pair < kBlocksPerPair;
                    ++block_in_pair)
                {
                    const std::size_t block =
                        pair * kBlocksPerPair + block_in_pair;
                    const std::size_t scale_destination =
                        destination_pair
                        + block_in_pair * kScaleBytesPerBlockTile;
                    const std::size_t weight_destination =
                        destination_pair
                        + kBlocksPerPair * kScaleBytesPerBlockTile
                        + block_in_pair * kWeightBytesPerBlockTile;

                    for (std::size_t lane = 0; lane < kRowsPerTile; ++lane) {
                        const std::size_t row = row_tile * kRowsPerTile + lane;
                        const std::size_t source_block =
                            tensor.offset
                            + row * row_bytes
                            + block * kNativeBlockBytes;
                        const std::uint16_t scale =
                            load_u16(native.data() + source_block);
                        if ((scale & 0x8000U) != 0 ||
                            (scale & 0x7c00U) == 0x7c00U) {
                            throw std::runtime_error(
                                "packed Q8 model scale rejected");
                        }
                        if ((scale & 0x7c00U) == 0 &&
                            (scale & 0x03ffU) != 0) {
                            ++result.subnormal_scales;
                        }
                        store_u16(
                            result.bytes.data()
                                + scale_destination
                                + lane * sizeof(std::uint16_t),
                            scale);

                        for (std::size_t word = 0; word < kDotWords; ++word) {
                            const std::uint32_t values = load_u32(
                                native.data()
                                    + source_block
                                    + sizeof(std::uint16_t)
                                    + word * sizeof(std::uint32_t));
                            for (unsigned byte = 0; byte < 4; ++byte) {
                                if (static_cast<std::uint8_t>(
                                        values >> (byte * 8)) == 0x80U) {
                                    throw std::runtime_error(
                                        "packed Q8 model contains -128");
                                }
                            }
                            store_u32(
                                result.bytes.data()
                                    + weight_destination
                                    + word * kRowsPerTile
                                        * sizeof(std::uint32_t)
                                    + lane * sizeof(std::uint32_t),
                                values);
                        }
                    }
                }
            }
        }

        ++result.tensor_count;
        result.block_tiles += row_tiles * blocks;
        result.quantized_values +=
            static_cast<std::uint64_t>(tensor.rows)
            * tensor.columns;
    }
    result.sha256 = sha256(result.bytes);
    return result;
}

std::vector<std::uint32_t> pack_q8x16_activation(
    std::span<const std::byte> native_q8,
    std::uint32_t columns)
{
    if (columns != kHidden && columns != kFfn) {
        throw std::runtime_error("packed Q8 activation columns rejected");
    }
    const std::size_t blocks = columns / kQ8Values;
    if (native_q8.size() != blocks * kNativeBlockBytes) {
        throw std::runtime_error("packed Q8 activation storage rejected");
    }

    std::vector<std::uint32_t> result(
        blocks * (1 + kDotWords),
        0);
    for (std::size_t block = 0; block < blocks; ++block) {
        const std::byte * source =
            native_q8.data() + block * kNativeBlockBytes;
        const std::uint16_t scale = load_u16(source);
        if ((scale & 0x8000U) != 0 ||
            (scale & 0x7c00U) == 0x7c00U) {
            throw std::runtime_error("packed Q8 activation scale rejected");
        }
        result[block] = scale;
        for (std::size_t word = 0; word < kDotWords; ++word) {
            const std::uint32_t values = load_u32(
                source
                + sizeof(std::uint16_t)
                + word * sizeof(std::uint32_t));
            for (unsigned byte = 0; byte < 4; ++byte) {
                if (static_cast<std::uint8_t>(
                        values >> (byte * 8)) == 0x80U) {
                    throw std::runtime_error(
                        "packed Q8 activation contains -128");
                }
            }
            result[blocks + block * kDotWords + word] = values;
        }
    }
    return result;
}

std::vector<float> project_q8x16_reference(
    std::span<const std::byte> packed_model,
    const packed_q8_tensor_spec & tensor,
    std::span<const std::uint32_t> packed_activation)
{
    validate_tensor(packed_model.size(), tensor);
    const std::size_t blocks = tensor.columns / kQ8Values;
    const std::size_t pairs = blocks / kBlocksPerPair;
    if (packed_activation.size() != blocks * (1 + kDotWords)) {
        throw std::runtime_error("packed Q8 reference activation rejected");
    }

    std::vector<float> output(tensor.rows);
    for (std::size_t row = 0; row < tensor.rows; ++row) {
        const std::size_t lane = row % kRowsPerTile;
        const std::size_t row_tile = row / kRowsPerTile;
        std::array<float, kDotWords> sums{};
        for (std::size_t block = 0; block < blocks; ++block) {
            const std::size_t block_in_pair = block % kBlocksPerPair;
            const std::size_t pair =
                tensor.offset
                + (row_tile * pairs + block / kBlocksPerPair)
                    * kPackedPairBytes;
            const std::uint16_t weight_scale = load_u16(
                packed_model.data()
                    + pair
                    + block_in_pair * kScaleBytesPerBlockTile
                    + lane * sizeof(std::uint16_t));
            const std::uint16_t activation_scale =
                static_cast<std::uint16_t>(packed_activation[block]);
            const float scale =
                f16_to_f32(weight_scale) * f16_to_f32(activation_scale);
            const std::size_t weights =
                pair
                + kBlocksPerPair * kScaleBytesPerBlockTile
                + block_in_pair * kWeightBytesPerBlockTile
                + lane * sizeof(std::uint32_t);
            const std::size_t activation =
                blocks + block * kDotWords;
            for (std::size_t word = 0; word < kDotWords; ++word) {
                const std::uint32_t weight_values = load_u32(
                    packed_model.data()
                        + weights
                        + word * kRowsPerTile * sizeof(std::uint32_t));
                const std::int32_t dot = signed_dot4(
                    weight_values,
                    packed_activation[activation + word]);
                sums[word] = std::fma(
                    scale,
                    static_cast<float>(dot),
                    sums[word]);
            }
        }
        const std::array<float, 4> low_high = {
            sums[0] + sums[4],
            sums[1] + sums[5],
            sums[2] + sums[6],
            sums[3] + sums[7],
        };
        const std::array<float, 2> quarters = {
            low_high[0] + low_high[2],
            low_high[1] + low_high[3],
        };
        output[row] = quarters[0] + quarters[1];
    }
    return output;
}

} // namespace trueos::lfm25
