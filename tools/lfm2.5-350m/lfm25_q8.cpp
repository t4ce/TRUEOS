#include "lfm25_q8.hpp"

#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <fcntl.h>
#include <fstream>
#include <immintrin.h>
#include <limits>
#include <map>
#include <memory>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <sys/mman.h>
#include <sys/stat.h>
#include <thread>
#include <unistd.h>
#include <utility>
#include <vector>

namespace trueos::lfm25 {
namespace {

constexpr std::size_t kHidden = 1'024;
constexpr std::size_t kFfn = 4'608;
constexpr std::size_t kVocabulary = 65'536;
constexpr std::size_t kLayers = 16;
constexpr std::size_t kHeads = 16;
constexpr std::size_t kKvHeads = 8;
constexpr std::size_t kHeadDimension = 64;
constexpr std::size_t kKvElements = kKvHeads * kHeadDimension;
constexpr std::size_t kAttentionSlots = 256;
constexpr std::size_t kQ8Values = 32;
constexpr std::size_t kQ8Bytes = 34;
constexpr std::size_t kContractHeaderBytes = 192;
constexpr std::size_t kDescriptorBytes = 24;
constexpr std::size_t kTensorCount = 148;
constexpr std::size_t kGoldenHeaderBytes = 256;
constexpr std::size_t kGoldenTokens = 10;
constexpr std::size_t kGoldenCheckpointsPerToken = 275;
constexpr std::size_t kGoldenTargetToken = 9;
constexpr float kProjectionBound = 1.0e-5F;
constexpr float kRmsEpsilon = 1.0e-5F;
constexpr float kRopeFrequencyBase = 1'000'000.0F;
constexpr std::array<std::uint8_t, kLayers> kLayerSchedule = {
    0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0,
};

enum class tensor_role : std::uint8_t {
    token_embedding = 0,
    token_embedding_norm = 1,
    ffn_norm = 2,
    ffn_gate = 3,
    ffn_down = 4,
    ffn_up = 5,
    operator_norm = 6,
    shortconv_kernel = 7,
    shortconv_input = 8,
    shortconv_output = 9,
    query_norm = 10,
    key_norm = 11,
    query = 12,
    key = 13,
    value = 14,
    attention_output = 15,
};

struct descriptor {
    std::uint16_t tensor_id;
    std::uint8_t layer;
    tensor_role role;
    std::uint8_t format;
    std::uint8_t rank;
    std::uint16_t flags;
    std::uint32_t columns;
    std::uint32_t rows;
    std::uint32_t offset;
    std::uint32_t bytes;
};

struct mapped_file {
    int descriptor = -1;
    const std::byte * data = nullptr;
    std::size_t bytes = 0;

    explicit mapped_file(const std::filesystem::path & path) {
        descriptor = open(path.c_str(), O_RDONLY | O_CLOEXEC);
        if (descriptor < 0) {
            throw std::runtime_error("cannot open " + path.string());
        }
        struct stat status {};
        if (fstat(descriptor, &status) != 0 || status.st_size <= 0) {
            close(descriptor);
            descriptor = -1;
            throw std::runtime_error("cannot stat " + path.string());
        }
        bytes = static_cast<std::size_t>(status.st_size);
        void * mapping = mmap(nullptr, bytes, PROT_READ, MAP_PRIVATE, descriptor, 0);
        if (mapping == MAP_FAILED) {
            close(descriptor);
            descriptor = -1;
            throw std::runtime_error("cannot mmap " + path.string());
        }
        data = static_cast<const std::byte *>(mapping);
    }

    ~mapped_file() {
        if (data != nullptr) {
            munmap(const_cast<std::byte *>(data), bytes);
        }
        if (descriptor >= 0) {
            close(descriptor);
        }
    }

    mapped_file(const mapped_file &) = delete;
    mapped_file & operator=(const mapped_file &) = delete;

    std::span<const std::byte> view() const {
        return {data, bytes};
    }
};

std::uint16_t u16(std::span<const std::byte> bytes, std::size_t offset) {
    if (offset + 2 > bytes.size()) {
        throw std::runtime_error("truncated little-endian u16");
    }
    return static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(bytes[offset])) |
           static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(bytes[offset + 1])) << 8;
}

std::uint32_t u32(std::span<const std::byte> bytes, std::size_t offset) {
    if (offset + 4 > bytes.size()) {
        throw std::runtime_error("truncated little-endian u32");
    }
    return static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[offset])) |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[offset + 1])) << 8 |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[offset + 2])) << 16 |
           static_cast<std::uint32_t>(std::to_integer<std::uint8_t>(bytes[offset + 3])) << 24;
}

bool magic_is(
    std::span<const std::byte> bytes,
    std::size_t offset,
    std::string_view expected) {
    return offset + expected.size() <= bytes.size() &&
           std::memcmp(bytes.data() + offset, expected.data(), expected.size()) == 0;
}

std::vector<std::byte> read_file(const std::filesystem::path & path) {
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    if (!input) {
        throw std::runtime_error("cannot open " + path.string());
    }
    const auto end = input.tellg();
    if (end <= 0) {
        throw std::runtime_error("empty artifact " + path.string());
    }
    std::vector<std::byte> result(static_cast<std::size_t>(end));
    input.seekg(0);
    input.read(
        reinterpret_cast<char *>(result.data()),
        static_cast<std::streamsize>(result.size()));
    if (!input) {
        throw std::runtime_error("cannot read " + path.string());
    }
    return result;
}

std::map<std::pair<std::uint8_t, tensor_role>, descriptor> read_contract(
    const std::filesystem::path & path) {
    const auto storage = read_file(path);
    const std::span<const std::byte> bytes(storage);
    if (bytes.size() != kContractHeaderBytes + kTensorCount * kDescriptorBytes ||
        !magic_is(bytes, 0, "TGALFM25") ||
        u16(bytes, 8) != 1 ||
        u16(bytes, 10) != kContractHeaderBytes ||
        u16(bytes, 12) != kDescriptorBytes ||
        u16(bytes, 14) != kTensorCount ||
        u32(bytes, 36) != kHidden ||
        u32(bytes, 40) != kFfn ||
        u32(bytes, 44) != 65'536 ||
        u16(bytes, 48) != 16) {
        throw std::runtime_error("fixed LFM2.5 model contract rejected");
    }

    std::map<std::pair<std::uint8_t, tensor_role>, descriptor> result;
    for (std::size_t index = 0; index < kTensorCount; ++index) {
        const std::size_t offset = kContractHeaderBytes + index * kDescriptorBytes;
        descriptor value{
            .tensor_id = u16(bytes, offset),
            .layer = std::to_integer<std::uint8_t>(bytes[offset + 2]),
            .role = static_cast<tensor_role>(std::to_integer<std::uint8_t>(bytes[offset + 3])),
            .format = std::to_integer<std::uint8_t>(bytes[offset + 4]),
            .rank = std::to_integer<std::uint8_t>(bytes[offset + 5]),
            .flags = u16(bytes, offset + 6),
            .columns = u32(bytes, offset + 8),
            .rows = u32(bytes, offset + 12),
            .offset = u32(bytes, offset + 16),
            .bytes = u32(bytes, offset + 20),
        };
        result.emplace(std::make_pair(value.layer, value.role), value);
    }
    return result;
}

std::map<std::string, std::vector<float>> read_target_checkpoints(
    const std::filesystem::path & path) {
    const auto storage = read_file(path);
    const std::span<const std::byte> bytes(storage);
    if (!magic_is(bytes, 0, std::string_view("TGALDE2\0", 8)) ||
        u32(bytes, 8) != 2 ||
        u32(bytes, 12) != kGoldenHeaderBytes ||
        u32(bytes, 16) != kGoldenTokens ||
        u32(bytes, 20) != kGoldenCheckpointsPerToken ||
        u32(bytes, 24) != kGoldenTokens * kGoldenCheckpointsPerToken ||
        u32(bytes, 32 + kGoldenTargetToken * 4) != 708 ||
        u32(bytes, 72 + kGoldenTargetToken * 4) != 36'309) {
        throw std::runtime_error("fixed hi decode golden rejected");
    }

    const std::array<std::string_view, 5> wanted = {
        "model.layers.{}.ffn_norm-0",
        "ffn_up-0",
        "ffn_gate-0",
        "ffn_swiglu-0",
        "model.layers.{}.ffn_out-0",
    };
    std::map<std::string, std::vector<float>> result;
    std::size_t offset = kGoldenHeaderBytes;
    for (std::size_t token = 0; token < kGoldenTokens; ++token) {
        for (std::size_t checkpoint = 0;
             checkpoint < kGoldenCheckpointsPerToken;
             ++checkpoint) {
            if (offset + 72 > bytes.size()) {
                throw std::runtime_error("truncated hi checkpoint descriptor");
            }
            const char * raw_name = reinterpret_cast<const char *>(bytes.data() + offset);
            std::size_t name_bytes = 0;
            while (name_bytes < 64 && raw_name[name_bytes] != '\0') {
                ++name_bytes;
            }
            const std::string name(raw_name, name_bytes);
            const std::size_t elements = u32(bytes, offset + 64);
            const std::size_t payload_bytes = u32(bytes, offset + 68);
            offset += 72;
            if (payload_bytes != elements * sizeof(float) ||
                offset + payload_bytes > bytes.size()) {
                throw std::runtime_error("bad hi checkpoint payload");
            }
            const bool selected =
                token == kGoldenTargetToken &&
                std::find(wanted.begin(), wanted.end(), name) != wanted.end();
            if (selected) {
                std::vector<float> values(elements);
                for (std::size_t element = 0; element < elements; ++element) {
                    values[element] =
                        std::bit_cast<float>(u32(bytes, offset + element * sizeof(float)));
                }
                result.emplace(name, std::move(values));
            }
            offset += payload_bytes;
        }
    }
    if (offset != bytes.size() || result.size() != wanted.size()) {
        throw std::runtime_error("hi checkpoint catalogue mismatch");
    }
    return result;
}

std::uint16_t f32_to_f16(float value) {
    return _cvtss_sh(value, _MM_FROUND_TO_NEAREST_INT);
}

float f16_to_f32(std::uint16_t value) {
    return _cvtsh_ss(value);
}

struct q8_block {
    std::uint16_t scale;
    std::array<std::int8_t, kQ8Values> values;
};

static_assert(sizeof(q8_block) == kQ8Bytes);

std::vector<q8_block> quantize(std::span<const float> input) {
    if (input.empty() || input.size() % kQ8Values != 0) {
        throw std::runtime_error("Q8 activation shape rejected");
    }
    std::vector<q8_block> result(input.size() / kQ8Values);
    for (std::size_t block_index = 0; block_index < result.size(); ++block_index) {
        const auto values = input.subspan(block_index * kQ8Values, kQ8Values);
        float maximum = 0.0F;
        for (float value : values) {
            if (!std::isfinite(value)) {
                throw std::runtime_error("non-finite Q8 activation");
            }
            maximum = std::max(maximum, std::abs(value));
        }
        const float scale = maximum / 127.0F;
        const float inverse = maximum == 0.0F ? 0.0F : 127.0F / maximum;
        result[block_index].scale = f32_to_f16(scale);
        for (std::size_t element = 0; element < kQ8Values; ++element) {
            result[block_index].values[element] =
                static_cast<std::int8_t>(std::rint(values[element] * inverse));
        }
    }
    return result;
}

std::int32_t saturated_pair(
    const std::int8_t * weights,
    const std::int8_t * activation) {
    const std::int32_t sum =
        static_cast<std::int32_t>(weights[0]) * static_cast<std::int32_t>(activation[0]) +
        static_cast<std::int32_t>(weights[1]) * static_cast<std::int32_t>(activation[1]);
    return std::clamp(
        sum,
        static_cast<std::int32_t>(std::numeric_limits<std::int16_t>::min()),
        static_cast<std::int32_t>(std::numeric_limits<std::int16_t>::max()));
}

float dot_q8(
    const std::byte * row,
    std::span<const q8_block> activation) {
    std::array<float, 8> lanes{};
    for (std::size_t block_index = 0; block_index < activation.size(); ++block_index) {
        const std::byte * raw = row + block_index * kQ8Bytes;
        const std::uint16_t weight_scale =
            static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(raw[0])) |
            static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(raw[1])) << 8;
        const float scale =
            f16_to_f32(weight_scale) * f16_to_f32(activation[block_index].scale);
        const auto * weights = reinterpret_cast<const std::int8_t *>(raw + 2);
        for (std::size_t lane = 0; lane < lanes.size(); ++lane) {
            const std::size_t start = lane * 4;
            const std::int32_t dot =
                saturated_pair(weights + start, activation[block_index].values.data() + start) +
                saturated_pair(weights + start + 2, activation[block_index].values.data() + start + 2);
            lanes[lane] = std::fma(scale, static_cast<float>(dot), lanes[lane]);
        }
    }
    const std::array<float, 4> low_high = {
        lanes[0] + lanes[4],
        lanes[1] + lanes[5],
        lanes[2] + lanes[6],
        lanes[3] + lanes[7],
    };
    const std::array<float, 2> quarters = {
        low_high[0] + low_high[2],
        low_high[1] + low_high[3],
    };
    return quarters[0] + quarters[1];
}

std::vector<float> project(
    std::span<const std::byte> native,
    const descriptor & tensor,
    std::span<const float> input,
    std::uint32_t threads) {
    if (tensor.format != 2 ||
        tensor.rank != 2 ||
        tensor.columns != input.size() ||
        tensor.columns % kQ8Values != 0) {
        throw std::runtime_error("fixed Q8 tensor descriptor rejected");
    }
    const std::size_t row_bytes = tensor.columns / kQ8Values * kQ8Bytes;
    if (tensor.bytes != tensor.rows * row_bytes ||
        static_cast<std::size_t>(tensor.offset) + tensor.bytes > native.size()) {
        throw std::runtime_error("fixed Q8 tensor storage rejected");
    }
    const auto activation = quantize(input);
    std::vector<float> output(tensor.rows);
    const std::uint32_t worker_count =
        std::max(1U, std::min<std::uint32_t>(threads, tensor.rows));
    std::vector<std::thread> workers;
    workers.reserve(worker_count);
    for (std::uint32_t worker = 0; worker < worker_count; ++worker) {
        workers.emplace_back([&, worker] {
            const std::size_t begin = tensor.rows * worker / worker_count;
            const std::size_t end = tensor.rows * (worker + 1) / worker_count;
            const std::byte * matrix = native.data() + tensor.offset;
            for (std::size_t row = begin; row < end; ++row) {
                output[row] = dot_q8(matrix + row * row_bytes, activation);
            }
        });
    }
    for (auto & worker : workers) {
        worker.join();
    }
    return output;
}

float compare(
    std::string_view name,
    std::span<const float> observed,
    std::span<const float> expected) {
    if (observed.size() != expected.size()) {
        throw std::runtime_error(std::string(name) + " output shape mismatch");
    }
    float maximum = 0.0F;
    for (std::size_t index = 0; index < observed.size(); ++index) {
        maximum = std::max(maximum, std::abs(observed[index] - expected[index]));
    }
    if (maximum > kProjectionBound) {
        throw std::runtime_error(
            std::string(name) + " parity max_abs=" + std::to_string(maximum) +
            " bound=" + std::to_string(kProjectionBound));
    }
    return maximum;
}

const descriptor & tensor(
    const std::map<std::pair<std::uint8_t, tensor_role>, descriptor> & contract,
    tensor_role role) {
    const auto found = contract.find(std::make_pair(0, role));
    if (found == contract.end()) {
        throw std::runtime_error("missing fixed layer-0 tensor");
    }
    return found->second;
}

const std::vector<float> & checkpoint(
    const std::map<std::string, std::vector<float>> & checkpoints,
    const std::string & name) {
    const auto found = checkpoints.find(name);
    if (found == checkpoints.end()) {
        throw std::runtime_error("missing checkpoint " + name);
    }
    return found->second;
}

} // namespace

void verify_q8_kernel(
    const std::filesystem::path & native_image,
    const std::filesystem::path & model_contract,
    const std::filesystem::path & hi_golden,
    std::uint32_t threads) {
    const mapped_file native(native_image);
    const auto contract = read_contract(model_contract);
    const auto checkpoints = read_target_checkpoints(hi_golden);

    const std::string ffn_input_name = "model.layers.{}.ffn_norm-0";
    const std::string swiglu_name = "ffn_swiglu-0";
    const auto & ffn_input =
        checkpoint(checkpoints, ffn_input_name);
    const auto & swiglu = checkpoint(checkpoints, swiglu_name);
    if (ffn_input.size() != kHidden || swiglu.size() != kFfn) {
        throw std::runtime_error("fixed FFN checkpoint shape rejected");
    }

    const auto up = project(
        native.view(),
        tensor(contract, tensor_role::ffn_up),
        ffn_input,
        threads);
    const auto gate = project(
        native.view(),
        tensor(contract, tensor_role::ffn_gate),
        ffn_input,
        threads);
    const auto down = project(
        native.view(),
        tensor(contract, tensor_role::ffn_down),
        swiglu,
        threads);

    const float up_error = compare("ffn_up-0", up, checkpoint(checkpoints, "ffn_up-0"));
    const float gate_error =
        compare("ffn_gate-0", gate, checkpoint(checkpoints, "ffn_gate-0"));
    const float down_error = compare(
        "ffn_out-0",
        down,
        checkpoint(checkpoints, "model.layers.{}.ffn_out-0"));

    std::fprintf(
        stderr,
        "lfm25-q8: PASS layer=0 projections=gate,up,down "
        "shapes=1024x4608,4608x1024 max_abs=%.9g,%.9g,%.9g "
        "bound=%.9g threads=%u backend=trueos-cpp-avx2\n",
        gate_error,
        up_error,
        down_error,
        kProjectionBound,
        threads);
}

} // namespace trueos::lfm25
