#include "lfm25_q8.hpp"
#include "lfm25_igpu.hpp"
#include "lfm25_packed.hpp"

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
constexpr std::string_view kPackedImageSha256 =
    "90876f02e0cc224fe23e01c8739dcbb94d7bcc8fbfa3d36204c6267a440f5fd8";
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
        // Intel's unified-memory OpenCL path needs a writable host virtual
        // mapping for CL_MEM_USE_HOST_PTR pinning. MAP_PRIVATE preserves the
        // sealed on-disk image while allowing the driver to register pages.
        void * mapping =
            mmap(nullptr, bytes, PROT_READ | PROT_WRITE, MAP_PRIVATE, descriptor, 0);
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
        !magic_is(bytes, 0, "LFMAOT25") ||
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
    if (!magic_is(bytes, 0, std::string_view("LFMADE2\0", 8)) ||
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

const descriptor & tensor_at(
    const std::map<std::pair<std::uint8_t, tensor_role>, descriptor> & contract,
    std::uint8_t layer,
    tensor_role role) {
    const auto found = contract.find(std::make_pair(layer, role));
    if (found == contract.end()) {
        throw std::runtime_error(
            "missing tensor layer=" + std::to_string(layer) +
            " role=" + std::to_string(static_cast<std::uint8_t>(role)));
    }
    return found->second;
}

class f32_sidecar {
  public:
    explicit f32_sidecar(const std::filesystem::path & path) {
        const auto storage = read_file(path);
        const std::span<const std::byte> bytes(storage);
        constexpr std::size_t header_bytes = 160;
        constexpr std::size_t entries = 55;
        constexpr std::size_t entry_bytes = 16;
        constexpr std::size_t elements = 65'280;
        constexpr std::size_t payload_offset = header_bytes + entries * entry_bytes;
        if (bytes.size() != payload_offset + elements * sizeof(float) ||
            !magic_is(bytes, 0, "LFMF32V1") ||
            u32(bytes, 8) != 1 ||
            u32(bytes, 12) != header_bytes ||
            u32(bytes, 16) != entries ||
            u32(bytes, 20) != entry_bytes ||
            u32(bytes, 24) != elements ||
            u32(bytes, 28) != payload_offset) {
            throw std::runtime_error("fixed F32 sidecar rejected");
        }
        std::size_t expected_offset = payload_offset;
        for (std::size_t index = 0; index < entries; ++index) {
            const std::size_t entry = header_bytes + index * entry_bytes;
            const std::uint16_t tensor_id = u16(bytes, entry);
            const std::size_t tensor_elements = u32(bytes, entry + 4);
            const std::size_t tensor_offset = u32(bytes, entry + 8);
            const std::size_t tensor_bytes = u32(bytes, entry + 12);
            if (u16(bytes, entry + 2) != 0 ||
                tensor_offset != expected_offset ||
                tensor_bytes != tensor_elements * sizeof(float) ||
                tensor_offset + tensor_bytes > bytes.size()) {
                throw std::runtime_error("fixed F32 sidecar entry rejected");
            }
            std::vector<float> values(tensor_elements);
            for (std::size_t element = 0; element < tensor_elements; ++element) {
                values[element] = std::bit_cast<float>(
                    u32(bytes, tensor_offset + element * sizeof(float)));
                if (!std::isfinite(values[element])) {
                    throw std::runtime_error("non-finite fixed F32 sidecar value");
                }
            }
            if (!values_.emplace(tensor_id, std::move(values)).second) {
                throw std::runtime_error("duplicate fixed F32 sidecar tensor");
            }
            expected_offset += tensor_bytes;
        }
        if (expected_offset != bytes.size() || values_.size() != entries) {
            throw std::runtime_error("fixed F32 sidecar catalogue rejected");
        }
    }

    std::span<const float> tensor_values(const descriptor & tensor) const {
        const auto found = values_.find(tensor.tensor_id);
        if (found == values_.end() ||
            found->second.size() !=
                static_cast<std::size_t>(tensor.columns) * tensor.rows) {
            throw std::runtime_error(
                "missing F32 tensor id=" + std::to_string(tensor.tensor_id));
        }
        return found->second;
    }

  private:
    std::map<std::uint16_t, std::vector<float>> values_;
};

std::vector<float> rms_norm(
    std::span<const float> input,
    std::span<const float> weights) {
    if (input.empty() || input.size() != weights.size()) {
        throw std::runtime_error("RMS norm shape rejected");
    }
    double sum_squares = 0.0;
    for (float value : input) {
        sum_squares += static_cast<double>(value * value);
    }
    const float mean =
        static_cast<float>(sum_squares / static_cast<double>(input.size()));
    const float inverse = 1.0F / std::sqrt(mean + kRmsEpsilon);
    std::vector<float> output(input.size());
    for (std::size_t index = 0; index < input.size(); ++index) {
        output[index] = input[index] * inverse * weights[index];
    }
    return output;
}

void rms_norm_head(
    std::span<float, kHeadDimension> head,
    std::span<const float> weights) {
    if (weights.size() != kHeadDimension) {
        throw std::runtime_error("attention RMS norm shape rejected");
    }
    double sum_squares = 0.0;
    for (float value : head) {
        sum_squares += static_cast<double>(value * value);
    }
    const float mean =
        static_cast<float>(sum_squares / static_cast<double>(kHeadDimension));
    const float inverse = 1.0F / std::sqrt(mean + kRmsEpsilon);
    for (std::size_t index = 0; index < kHeadDimension; ++index) {
        head[index] = head[index] * inverse * weights[index];
    }
}

void rope_neox(std::span<float, kHeadDimension> head, std::uint32_t position) {
    constexpr std::size_t half = kHeadDimension / 2;
    const float theta_scale =
        ::powf(kRopeFrequencyBase, -2.0F / static_cast<float>(kHeadDimension));
    float angle = static_cast<float>(position);
    for (std::size_t pair = 0; pair < half; ++pair) {
        const float cosine = static_cast<float>(::cos(static_cast<double>(angle)));
        const float sine = static_cast<float>(::sin(static_cast<double>(angle)));
        const float low = head[pair];
        const float high = head[pair + half];
        head[pair] = low * cosine - high * sine;
        head[pair + half] = low * sine + high * cosine;
        angle *= theta_scale;
    }
}

float f32_dot_pinned(std::span<const float> left, std::span<const float> right) {
    if (left.size() != right.size()) {
        throw std::runtime_error("F32 dot shape rejected");
    }
    const std::size_t vectorized = left.size() & ~std::size_t{31};
    std::array<std::array<float, 8>, 4> lanes{};
    for (std::size_t base = 0; base < vectorized; base += 32) {
        for (std::size_t reg = 0; reg < 4; ++reg) {
            for (std::size_t lane = 0; lane < 8; ++lane) {
                const std::size_t index = base + reg * 8 + lane;
                lanes[reg][lane] =
                    std::fma(left[index], right[index], lanes[reg][lane]);
            }
        }
    }
    for (std::size_t lane = 0; lane < 8; ++lane) {
        lanes[0][lane] += lanes[2][lane];
        lanes[1][lane] += lanes[3][lane];
        lanes[0][lane] += lanes[1][lane];
    }
    const std::array<float, 4> low_high = {
        lanes[0][0] + lanes[0][4],
        lanes[0][1] + lanes[0][5],
        lanes[0][2] + lanes[0][6],
        lanes[0][3] + lanes[0][7],
    };
    const std::array<float, 2> pair = {
        low_high[0] + low_high[1],
        low_high[2] + low_high[3],
    };
    float result = pair[0] + pair[1];
    for (std::size_t index = vectorized; index < left.size(); ++index) {
        result += left[index] * right[index];
    }
    return result;
}

void softmax(std::span<float> values) {
    if (values.empty()) {
        throw std::runtime_error("softmax shape rejected");
    }
    const float maximum = *std::max_element(values.begin(), values.end());
    double sum = 0.0;
    for (float & value : values) {
        value = ::expf(value - maximum);
        sum += static_cast<double>(value);
    }
    const float inverse = static_cast<float>(1.0 / sum);
    for (float & value : values) {
        value *= inverse;
    }
}

float f32_from_bits(std::uint32_t bits) {
    return std::bit_cast<float>(bits);
}

float pinned_expf(float value) {
    const float r = f32_from_bits(0x4b40'0000);
    const float z = std::fma(value, f32_from_bits(0x3fb8'aa3b), r);
    const float n = z - r;
    if (!std::isfinite(n) || std::abs(n) > 126.0F) {
        throw std::runtime_error("SwiGLU exponent outside fixed branch");
    }
    const float inner =
        std::fma(-n, f32_from_bits(0x3f31'7200), value);
    const float b =
        std::fma(-n, f32_from_bits(0x35bf'be8e), inner);
    const std::uint32_t exponent_bits = std::bit_cast<std::uint32_t>(z) << 23;
    const float k =
        f32_from_bits(exponent_bits + std::bit_cast<std::uint32_t>(1.0F));
    const float u = b * b;
    const float left =
        std::fma(f32_from_bits(0x3c07'2010), b, f32_from_bits(0x3d2b'9f17));
    const float right =
        std::fma(f32_from_bits(0x3e2a'af33), b, f32_from_bits(0x3eff'fedb));
    const float polynomial = std::fma(left, u, right);
    const float j =
        std::fma(polynomial, u, f32_from_bits(0x3f7f'fff6) * b);
    return std::fma(j, k, k);
}

float silu_multiply(float gate, float up) {
    return (gate / (1.0F + pinned_expf(-gate))) * up;
}

void add_in_place(std::vector<float> & destination, std::span<const float> source) {
    if (destination.size() != source.size()) {
        throw std::runtime_error("residual shape rejected");
    }
    for (std::size_t index = 0; index < destination.size(); ++index) {
        destination[index] += source[index];
    }
}

std::vector<float> dequantize_embedding(
    std::span<const std::byte> native,
    const descriptor & embedding,
    std::uint32_t token) {
    if (embedding.format != 2 ||
        embedding.columns != kHidden ||
        embedding.rows != kVocabulary ||
        token >= kVocabulary) {
        throw std::runtime_error("fixed token embedding rejected");
    }
    const std::size_t row_bytes = kHidden / kQ8Values * kQ8Bytes;
    const std::size_t row_offset =
        static_cast<std::size_t>(embedding.offset) + token * row_bytes;
    if (row_offset + row_bytes > native.size()) {
        throw std::runtime_error("fixed token embedding storage rejected");
    }
    std::vector<float> output(kHidden);
    const std::byte * row = native.data() + row_offset;
    for (std::size_t block = 0; block < kHidden / kQ8Values; ++block) {
        const std::byte * raw = row + block * kQ8Bytes;
        const std::uint16_t scale_bits =
            static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(raw[0])) |
            static_cast<std::uint16_t>(std::to_integer<std::uint8_t>(raw[1])) << 8;
        const float scale = f16_to_f32(scale_bits);
        for (std::size_t element = 0; element < kQ8Values; ++element) {
            output[block * kQ8Values + element] =
                scale * static_cast<float>(
                    reinterpret_cast<const std::int8_t *>(raw + 2)[element]);
        }
    }
    return output;
}

std::uint32_t argmax(std::span<const float> values) {
    if (values.size() != kVocabulary) {
        throw std::runtime_error("fixed tied-head output shape rejected");
    }
    std::size_t best = 0;
    for (std::size_t index = 1; index < values.size(); ++index) {
        if (values[index] > values[best]) {
            best = index;
        }
    }
    return static_cast<std::uint32_t>(best);
}

class native_decoder {
  public:
    native_decoder(
        const std::filesystem::path & native_path,
        const std::filesystem::path & contract_path,
        const std::filesystem::path & sidecar_path,
        std::uint32_t threads,
        native_projection_backend backend,
        const std::filesystem::path & igc_spirv)
        : native_(native_path),
          contract_(read_contract(contract_path)),
          sidecar_(sidecar_path),
          threads_(std::max(1U, threads)) {
        const bool use_packed =
            backend == native_projection_backend::cpu_packed_reference ||
            backend == native_projection_backend::intel_igc_packed;
        std::vector<packed_q8_tensor_spec> packed_tensors;
        if (use_packed) {
            packed_tensors.reserve(contract_.size());
            for (const auto & [key, value] : contract_) {
                static_cast<void>(key);
                if (value.format == 2 && value.rank == 2) {
                    packed_tensors.push_back({
                        .offset = value.offset,
                        .columns = value.columns,
                        .rows = value.rows,
                    });
                }
            }
        }
        if (backend == native_projection_backend::cpu_packed_reference) {
            packed_cpu_ = std::make_unique<packed_q8_model>(
                pack_q8x16_model(native_.view(), packed_tensors));
        }
        if (backend == native_projection_backend::intel_igc ||
            backend == native_projection_backend::intel_igc_packed) {
            igpu_ = std::make_unique<intel_igc_projector>(
                igc_spirv,
                native_.view().data(),
                native_.view().size(),
                use_packed
                    ? intel_igc_weight_layout::packed_q8x16_pair
                    : intel_igc_weight_layout::native_q8_0,
                packed_tensors);
        }
        for (std::size_t layer = 0; layer < kLayers; ++layer) {
            if (kLayerSchedule[layer] == 0) {
                shortconv_[layer].resize(kHidden);
            }
        }
    }

    std::uint32_t decode(std::uint32_t token) {
        std::vector<float> hidden = dequantize_embedding(
            native_.view(),
            tensor_at(contract_, 0xff, tensor_role::token_embedding),
            token);

        for (std::uint8_t layer = 0; layer < kLayers; ++layer) {
            std::vector<float> operator_residual = hidden;
            hidden = rms_norm(
                hidden,
                sidecar_.tensor_values(
                    tensor_at(contract_, layer, tensor_role::operator_norm)));
            std::vector<float> branch =
                kLayerSchedule[layer] == 0
                    ? shortconv_layer(layer, hidden)
                    : attention_layer(layer, hidden);
            hidden = std::move(operator_residual);
            add_in_place(hidden, branch);

            std::vector<float> ffn_residual = hidden;
            const auto ffn_input = rms_norm(
                hidden,
                sidecar_.tensor_values(
                    tensor_at(contract_, layer, tensor_role::ffn_norm)));
            const auto up = project_tensor(
                tensor_at(contract_, layer, tensor_role::ffn_up),
                ffn_input);
            const auto gate = project_tensor(
                tensor_at(contract_, layer, tensor_role::ffn_gate),
                ffn_input);
            std::vector<float> activated(kFfn);
            for (std::size_t index = 0; index < kFfn; ++index) {
                activated[index] = silu_multiply(gate[index], up[index]);
            }
            const auto ffn_output = project_tensor(
                tensor_at(contract_, layer, tensor_role::ffn_down),
                activated);
            hidden = std::move(ffn_residual);
            add_in_place(hidden, ffn_output);
        }

        const auto normalized = rms_norm(
            hidden,
            sidecar_.tensor_values(
                tensor_at(contract_, 0xff, tensor_role::token_embedding_norm)));
        const auto logits = project_tensor(
            tensor_at(contract_, 0xff, tensor_role::token_embedding),
            normalized);
        ++position_;
        return argmax(logits);
    }

  private:
    struct kv_cache {
        std::vector<std::uint16_t> keys;
        std::vector<std::uint16_t> values;
    };

    std::vector<float> project_tensor(
        const descriptor & tensor,
        std::span<const float> input) {
        if (packed_cpu_) {
            if (tensor.format != 2 ||
                tensor.rank != 2 ||
                tensor.columns != input.size() ||
                tensor.columns % kQ8Values != 0) {
                throw std::runtime_error(
                    "fixed packed CPU Q8 tensor descriptor rejected");
            }
            const auto activation = quantize(input);
            const auto packed_activation = pack_q8x16_activation(
                std::as_bytes(std::span<const q8_block>(activation)),
                tensor.columns);
            ++packed_cpu_launches_;
            packed_cpu_weight_bytes_ +=
                static_cast<std::uint64_t>(tensor.rows)
                * (static_cast<std::uint64_t>(tensor.columns) / kQ8Values)
                * kQ8Bytes;
            return project_q8x16_reference(
                packed_cpu_->bytes,
                {
                    .offset = tensor.offset,
                    .columns = tensor.columns,
                    .rows = tensor.rows,
                },
                packed_activation);
        }
        if (!igpu_) {
            return project(native_.view(), tensor, input, threads_);
        }
        if (tensor.format != 2 ||
            tensor.rank != 2 ||
            tensor.columns != input.size() ||
            tensor.columns % kQ8Values != 0) {
            throw std::runtime_error("fixed IGC Q8 tensor descriptor rejected");
        }
        const auto activation = quantize(input);
        const auto bytes = std::as_bytes(std::span<const q8_block>(activation));
        return igpu_->project(
            tensor.offset,
            tensor.columns,
            tensor.rows,
            bytes);
    }

    std::vector<float> shortconv_layer(
        std::uint8_t layer,
        std::span<const float> input) {
        const auto projected = project_tensor(
            tensor_at(contract_, layer, tensor_role::shortconv_input),
            input);
        if (projected.size() != 3 * kHidden ||
            shortconv_[layer].size() != kHidden) {
            throw std::runtime_error("fixed short-convolution shape rejected");
        }
        const auto kernel = sidecar_.tensor_values(
            tensor_at(contract_, layer, tensor_role::shortconv_kernel));
        if (kernel.size() != 3 * kHidden) {
            throw std::runtime_error("fixed short-convolution kernel rejected");
        }
        std::vector<float> mixed(kHidden);
        for (std::size_t channel = 0; channel < kHidden; ++channel) {
            const float b = projected[channel];
            const float c = projected[kHidden + channel];
            const float x = projected[2 * kHidden + channel];
            const float bx = b * x;
            const std::size_t base = channel * 3;
            const float convolution =
                kernel[base] * shortconv_[layer][channel][0] +
                kernel[base + 1] * shortconv_[layer][channel][1] +
                kernel[base + 2] * bx;
            shortconv_[layer][channel] = {
                shortconv_[layer][channel][1],
                bx,
            };
            mixed[channel] = c * convolution;
        }
        return project_tensor(
            tensor_at(contract_, layer, tensor_role::shortconv_output),
            mixed);
    }

    std::vector<float> attention_layer(
        std::uint8_t layer,
        std::span<const float> input) {
        auto query = project_tensor(
            tensor_at(contract_, layer, tensor_role::query),
            input);
        auto key = project_tensor(
            tensor_at(contract_, layer, tensor_role::key),
            input);
        const auto value = project_tensor(
            tensor_at(contract_, layer, tensor_role::value),
            input);
        if (query.size() != kHidden ||
            key.size() != kKvElements ||
            value.size() != kKvElements) {
            throw std::runtime_error("fixed attention projection shape rejected");
        }
        const auto query_norm = sidecar_.tensor_values(
            tensor_at(contract_, layer, tensor_role::query_norm));
        const auto key_norm = sidecar_.tensor_values(
            tensor_at(contract_, layer, tensor_role::key_norm));
        for (std::size_t head = 0; head < kHeads; ++head) {
            std::span<float, kHeadDimension> values(
                query.data() + head * kHeadDimension,
                kHeadDimension);
            rms_norm_head(values, query_norm);
            rope_neox(values, position_);
        }
        for (std::size_t head = 0; head < kKvHeads; ++head) {
            std::span<float, kHeadDimension> values(
                key.data() + head * kHeadDimension,
                kHeadDimension);
            rms_norm_head(values, key_norm);
            rope_neox(values, position_);
        }

        kv_cache & cache = kv_[layer];
        const std::size_t expected_cache = position_ * kKvElements;
        if (cache.keys.size() != expected_cache ||
            cache.values.size() != expected_cache) {
            throw std::runtime_error("fixed attention cache position rejected");
        }
        for (float element : key) {
            cache.keys.push_back(f32_to_f16(element));
        }
        for (float element : value) {
            cache.values.push_back(f32_to_f16(element));
        }

        const std::size_t positions = position_ + 1;
        const float scale = 1.0F / std::sqrt(static_cast<float>(kHeadDimension));
        std::vector<float> context(kHidden);
        std::vector<float> scores(positions);
        for (std::size_t query_head = 0; query_head < kHeads; ++query_head) {
            std::array<float, kHeadDimension> rounded_query{};
            for (std::size_t dimension = 0;
                 dimension < kHeadDimension;
                 ++dimension) {
                rounded_query[dimension] = f16_to_f32(f32_to_f16(
                    query[query_head * kHeadDimension + dimension]));
            }
            const std::size_t kv_head = query_head * kKvHeads / kHeads;
            for (std::size_t cache_position = 0;
                 cache_position < positions;
                 ++cache_position) {
                std::array<float, kHeadDimension> key_values{};
                const std::size_t start =
                    cache_position * kKvElements + kv_head * kHeadDimension;
                for (std::size_t dimension = 0;
                     dimension < kHeadDimension;
                     ++dimension) {
                    key_values[dimension] =
                        f16_to_f32(cache.keys[start + dimension]);
                }
                scores[cache_position] =
                    f32_dot_pinned(rounded_query, key_values) * scale;
            }
            softmax(scores);
            for (std::size_t dimension = 0;
                 dimension < kHeadDimension;
                 ++dimension) {
                std::array<float, kAttentionSlots> weights{};
                std::array<float, kAttentionSlots> values{};
                for (std::size_t cache_position = 0;
                     cache_position < positions;
                     ++cache_position) {
                    const std::size_t index =
                        cache_position * kKvElements +
                        kv_head * kHeadDimension +
                        dimension;
                    weights[cache_position] =
                        f16_to_f32(f32_to_f16(scores[cache_position]));
                    values[cache_position] =
                        f16_to_f32(cache.values[index]);
                }
                context[query_head * kHeadDimension + dimension] =
                    f32_dot_pinned(values, weights);
            }
        }
        return project_tensor(
            tensor_at(contract_, layer, tensor_role::attention_output),
            context);
    }

  public:
    const intel_igc_projector * igpu() const {
        return igpu_.get();
    }

    const packed_q8_model * packed_cpu() const {
        return packed_cpu_.get();
    }

    std::uint64_t packed_cpu_launches() const {
        return packed_cpu_launches_;
    }

    std::uint64_t packed_cpu_weight_bytes() const {
        return packed_cpu_weight_bytes_;
    }

  private:
    mapped_file native_;
    std::map<std::pair<std::uint8_t, tensor_role>, descriptor> contract_;
    f32_sidecar sidecar_;
    std::uint32_t threads_;
    std::unique_ptr<intel_igc_projector> igpu_;
    std::unique_ptr<packed_q8_model> packed_cpu_;
    std::uint64_t packed_cpu_launches_ = 0;
    std::uint64_t packed_cpu_weight_bytes_ = 0;
    std::uint32_t position_ = 0;
    std::array<std::vector<std::array<float, 2>>, kLayers> shortconv_;
    std::array<kv_cache, kLayers> kv_;
};

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

void verify_q8_packed_layout(
    const std::filesystem::path & native_image,
    const std::filesystem::path & model_contract,
    const std::filesystem::path & hi_golden,
    std::uint32_t threads)
{
    const mapped_file native(native_image);
    const auto contract = read_contract(model_contract);
    const auto checkpoints = read_target_checkpoints(hi_golden);
    std::vector<packed_q8_tensor_spec> tensors;
    tensors.reserve(contract.size());
    for (const auto & [key, value] : contract) {
        static_cast<void>(key);
        if (value.format == 2 && value.rank == 2) {
            tensors.push_back({
                .offset = value.offset,
                .columns = value.columns,
                .rows = value.rows,
            });
        }
    }

    const packed_q8_model packed =
        pack_q8x16_model(native.view(), tensors);
    if (packed.bytes.size() != native.view().size() ||
        packed.tensor_count != 93 ||
        packed.block_tiles != 692'224 ||
        packed.quantized_values != 354'418'688 ||
        packed.subnormal_scales != 25'994 ||
        packed.sha256 != kPackedImageSha256) {
        throw std::runtime_error("fixed packed Q8 model census rejected");
    }

    const auto verify_projection = [&](
        tensor_role role,
        std::string_view name,
        std::span<const float> input,
        std::span<const float> expected)
    {
        const descriptor & value = tensor(contract, role);
        const auto activation = quantize(input);
        const auto native_activation =
            std::as_bytes(std::span<const q8_block>(activation));
        const auto packed_activation =
            pack_q8x16_activation(native_activation, value.columns);
        const auto observed = project_q8x16_reference(
            packed.bytes,
            {
                .offset = value.offset,
                .columns = value.columns,
                .rows = value.rows,
            },
            packed_activation);
        return compare(name, observed, expected);
    };

    const std::string ffn_input_name = "model.layers.{}.ffn_norm-0";
    const std::string swiglu_name = "ffn_swiglu-0";
    const auto & ffn_input =
        checkpoint(checkpoints, ffn_input_name);
    const auto & swiglu = checkpoint(checkpoints, swiglu_name);
    if (ffn_input.size() != kHidden || swiglu.size() != kFfn) {
        throw std::runtime_error("fixed packed FFN checkpoint shape rejected");
    }
    const float up_error = verify_projection(
        tensor_role::ffn_up,
        "packed-ffn_up-0",
        ffn_input,
        checkpoint(checkpoints, "ffn_up-0"));
    const float gate_error = verify_projection(
        tensor_role::ffn_gate,
        "packed-ffn_gate-0",
        ffn_input,
        checkpoint(checkpoints, "ffn_gate-0"));
    const float down_error = verify_projection(
        tensor_role::ffn_down,
        "packed-ffn_out-0",
        swiglu,
        checkpoint(checkpoints, "model.layers.{}.ffn_out-0"));

    std::fprintf(
        stderr,
        "lfm25-q8-packed: PASS layout=pair1088-x16 "
        "tensors=%llu block_tiles=%llu quantized_values=%llu "
        "subnormal_scales=%llu image_bytes=%zu "
        "image_sha256=%s "
        "max_abs=%.9g,%.9g,%.9g bound=%.9g threads=%u "
        "backend=trueos-cpp-packed-reference\n",
        static_cast<unsigned long long>(packed.tensor_count),
        static_cast<unsigned long long>(packed.block_tiles),
        static_cast<unsigned long long>(packed.quantized_values),
        static_cast<unsigned long long>(packed.subnormal_scales),
        packed.bytes.size(),
        packed.sha256.c_str(),
        gate_error,
        up_error,
        down_error,
        kProjectionBound,
        threads);
}

native_decode_result run_native_decode(
    const std::filesystem::path & native_image,
    const std::filesystem::path & model_contract,
    const std::filesystem::path & f32_sidecar_path,
    const std::vector<std::uint32_t> & input_tokens,
    std::uint32_t max_reply_tokens,
    const std::vector<std::uint32_t> & stop_tokens,
    std::uint32_t threads,
    native_projection_backend backend,
    const std::filesystem::path & igc_spirv) {
    if (input_tokens.empty() ||
        input_tokens.size() + max_reply_tokens > kAttentionSlots) {
        throw std::runtime_error("fixed native decode context rejected");
    }
    native_decoder decoder(
        native_image,
        model_contract,
        f32_sidecar_path,
        threads,
        backend,
        igc_spirv);
    native_decode_result result;
    result.next_tokens.reserve(input_tokens.size() + max_reply_tokens);
    result.generated_tokens.reserve(max_reply_tokens);
    std::uint32_t next = 0;
    for (std::uint32_t token : input_tokens) {
        next = decoder.decode(token);
        result.next_tokens.push_back(next);
    }
    for (std::uint32_t index = 0; index < max_reply_tokens; ++index) {
        if (std::find(stop_tokens.begin(), stop_tokens.end(), next) !=
            stop_tokens.end()) {
            result.stopped = true;
            break;
        }
        result.generated_tokens.push_back(next);
        if (index + 1 == max_reply_tokens) {
            break;
        }
        next = decoder.decode(next);
        result.next_tokens.push_back(next);
    }
    if (const auto * igpu = decoder.igpu()) {
        result.projection_device = igpu->device_name();
        result.projection_platform = igpu->platform_name();
        result.projection_driver = igpu->driver_version();
        result.projection_il = igpu->il_version();
        result.projection_weight_layout = igpu->weight_layout();
        result.projection_program_binary_bytes = igpu->program_binary_bytes();
        result.projection_program_binary_sha256 = igpu->program_binary_sha256();
        result.projection_model_bytes = igpu->resident_model_bytes();
        result.projection_subnormal_scales =
            igpu->packed_subnormal_scales();
        result.projection_model_sha256 =
            igpu->packed_model_sha256();
        result.projection_launches = igpu->launches();
        result.projection_nanoseconds = igpu->kernel_nanoseconds();
        result.projection_weight_bytes =
            igpu->projected_weight_bytes();
    } else if (const auto * packed = decoder.packed_cpu()) {
        result.projection_device = "CPU packed Q8x16 reference";
        result.projection_weight_layout = "pair1088-x16-reference";
        result.projection_model_sha256 = packed->sha256;
        result.projection_model_bytes = packed->bytes.size();
        result.projection_subnormal_scales = packed->subnormal_scales;
        result.projection_launches = decoder.packed_cpu_launches();
        result.projection_weight_bytes =
            decoder.packed_cpu_weight_bytes();
    } else {
        result.projection_device = "CPU AVX2/F16C/FMA";
    }
    return result;
}

} // namespace trueos::lfm25
