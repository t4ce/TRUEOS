#include "llama.h"
#include "ggml.h"
#include "ggml-backend.h"

#include <array>
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <limits>
#include <string>
#include <vector>

namespace {

constexpr uint32_t kLayers = 16;
constexpr uint32_t kHidden = 1024;
constexpr uint32_t kVocabulary = 65536;
constexpr uint32_t kCheckpointsPerLayer = 6;
constexpr uint32_t kCheckpointCount = 1 + kLayers * kCheckpointsPerLayer + 2;
constexpr uint32_t kHeaderBytes = 256;
constexpr std::array<uint8_t, kLayers> kAttention = {
    0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0,
};
constexpr const char * kCommit = "76f46ad29d61fd8c1401e8221842934bf62a6064";
constexpr std::array<uint8_t, 32> kGgufSha256 = {
    0xbe, 0x03, 0x6a, 0x75, 0x72, 0x95, 0xe5, 0x50, 0x09, 0x8b, 0x85, 0xe1, 0x3f, 0x6a, 0xf2, 0x73,
    0x5d, 0x0f, 0xa7, 0x3b, 0x41, 0xe1, 0x15, 0x6a, 0x40, 0xc7, 0xd8, 0xe8, 0xe3, 0x2a, 0x57, 0x66,
};
constexpr std::array<uint8_t, 32> kNativeSha256 = {
    0x05, 0x1c, 0x60, 0x85, 0x67, 0x86, 0xde, 0x2a, 0xc7, 0x08, 0x91, 0x09, 0x35, 0x42, 0x59, 0xfa,
    0x29, 0xfc, 0xd5, 0x7e, 0x83, 0xd5, 0x85, 0xef, 0xc8, 0x6a, 0xfa, 0x0f, 0xb6, 0x05, 0xbb, 0x86,
};
constexpr std::array<uint8_t, 32> kContractSha256 = {
    0x6b, 0x9f, 0x15, 0xfd, 0xdf, 0x6a, 0x01, 0x98, 0xb7, 0x7d, 0x0e, 0x33, 0x9b, 0xd7, 0x97, 0x8a,
    0x38, 0x88, 0x1f, 0x77, 0x25, 0x20, 0xa4, 0x32, 0x90, 0xbb, 0xea, 0x81, 0x8f, 0xab, 0xc1, 0xc4,
};

struct checkpoint {
    std::array<char, 64> name = {};
    std::vector<float> values;
    bool seen = false;
};

struct capture_state {
    std::array<checkpoint, kCheckpointCount> checkpoints;
    std::string error;
};

void expected_name(uint32_t index, char * output, size_t capacity) {
    if (index == 0) {
        std::snprintf(output, capacity, "model.embed_tokens");
        return;
    }
    if (index == kCheckpointCount - 2) {
        std::snprintf(output, capacity, "result_norm");
        return;
    }
    if (index == kCheckpointCount - 1) {
        std::snprintf(output, capacity, "result_output");
        return;
    }

    const uint32_t relative = index - 1;
    const uint32_t layer = relative / kCheckpointsPerLayer;
    switch (relative % kCheckpointsPerLayer) {
        case 0:
            std::snprintf(output, capacity, "model.layers.{}.operator_norm-%u", layer);
            break;
        case 1:
            std::snprintf(output, capacity,
                          kAttention[layer] ? "model.layers.{}.self_attn.out_proj-%u"
                                            : "model.layers.{}.conv.out_proj-%u",
                          layer);
            break;
        case 2:
            std::snprintf(output, capacity, "model.layers.{}.operator_residual-%u", layer);
            break;
        case 3:
            std::snprintf(output, capacity, "model.layers.{}.ffn_norm-%u", layer);
            break;
        case 4:
            std::snprintf(output, capacity, "model.layers.{}.ffn_out-%u", layer);
            break;
        default:
            std::snprintf(output, capacity, "l_out-%u", layer);
            break;
    }
}

uint32_t expected_elements(uint32_t index) {
    return index == kCheckpointCount - 1 ? kVocabulary : kHidden;
}

int checkpoint_index(const char * name) {
    std::array<char, 64> expected = {};
    for (uint32_t index = 0; index < kCheckpointCount; ++index) {
        expected_name(index, expected.data(), expected.size());
        if (std::strcmp(name, expected.data()) == 0) {
            return static_cast<int>(index);
        }
    }
    return -1;
}

bool capture_callback(ggml_tensor * tensor, bool ask, void * opaque) {
    auto & state = *static_cast<capture_state *>(opaque);
    const int index = checkpoint_index(tensor->name);
    if (ask) {
        return index >= 0;
    }
    if (index < 0 || !state.error.empty()) {
        return true;
    }

    auto & checkpoint = state.checkpoints[static_cast<size_t>(index)];
    if (checkpoint.seen) {
        state.error = std::string("checkpoint evaluated twice: ") + tensor->name;
        return true;
    }
    if (tensor->type != GGML_TYPE_F32 || !ggml_is_contiguous(tensor) ||
        ggml_nelements(tensor) != expected_elements(static_cast<uint32_t>(index))) {
        state.error = std::string("checkpoint shape/type mismatch: ") + tensor->name;
        return true;
    }
    expected_name(static_cast<uint32_t>(index), checkpoint.name.data(), checkpoint.name.size());
    checkpoint.values.resize(expected_elements(static_cast<uint32_t>(index)));
    ggml_backend_tensor_get(tensor, checkpoint.values.data(), 0,
                            checkpoint.values.size() * sizeof(float));
    checkpoint.seen = true;
    return true;
}

bool write_bytes(FILE * file, const void * bytes, size_t length) {
    return std::fwrite(bytes, 1, length, file) == length;
}

bool write_u32(FILE * file, uint32_t value) {
    const uint8_t bytes[4] = {
        static_cast<uint8_t>(value), static_cast<uint8_t>(value >> 8),
        static_cast<uint8_t>(value >> 16), static_cast<uint8_t>(value >> 24),
    };
    return write_bytes(file, bytes, sizeof(bytes));
}

uint32_t argmax_token(const std::vector<float> & logits) {
    uint32_t best = 0;
    for (uint32_t token = 1; token < logits.size(); ++token) {
        if (logits[token] > logits[best]) {
            best = token;
        }
    }
    return best;
}

bool write_trace(const char * path, const capture_state & state, uint32_t input_token) {
    FILE * file = std::fopen(path, "wb");
    if (file == nullptr) {
        std::fprintf(stderr, "cannot create %s: %s\n", path, std::strerror(errno));
        return false;
    }

    const uint32_t output_token = argmax_token(state.checkpoints.back().values);
    std::array<uint8_t, kHeaderBytes> header = {};
    std::memcpy(header.data(), "TGALDEC1", 8);
    auto set_u32 = [&header](size_t offset, uint32_t value) {
        header[offset + 0] = static_cast<uint8_t>(value);
        header[offset + 1] = static_cast<uint8_t>(value >> 8);
        header[offset + 2] = static_cast<uint8_t>(value >> 16);
        header[offset + 3] = static_cast<uint8_t>(value >> 24);
    };
    set_u32(8, 1);
    set_u32(12, kHeaderBytes);
    set_u32(16, input_token);
    set_u32(20, kCheckpointCount);
    set_u32(24, output_token);
    std::memcpy(header.data() + 32, kCommit, 40);
    std::memcpy(header.data() + 72, kGgufSha256.data(), kGgufSha256.size());
    std::memcpy(header.data() + 104, kNativeSha256.data(), kNativeSha256.size());
    std::memcpy(header.data() + 136, kContractSha256.data(), kContractSha256.size());

    bool ok = write_bytes(file, header.data(), header.size());
    for (const auto & checkpoint : state.checkpoints) {
        ok = ok && write_bytes(file, checkpoint.name.data(), checkpoint.name.size()) &&
             write_u32(file, static_cast<uint32_t>(checkpoint.values.size())) &&
             write_u32(file, static_cast<uint32_t>(checkpoint.values.size() * sizeof(float)));
        for (float value : checkpoint.values) {
            uint32_t bits = 0;
            std::memcpy(&bits, &value, sizeof(bits));
            ok = ok && write_u32(file, bits);
        }
    }
    ok = std::fclose(file) == 0 && ok;
    if (ok) {
        std::fprintf(stderr,
                     "truega-decode-trace token=%u checkpoints=%u output_token=%u path=%s\n",
                     input_token, kCheckpointCount, output_token, path);
    }
    return ok;
}

void quiet_log_callback(ggml_log_level, const char *, void *) {
}

} // namespace

int main(int argc, char ** argv) {
    if (argc != 3) {
        std::fprintf(stderr, "usage: %s MODEL.gguf TRACE.bin\n", argv[0]);
        return 2;
    }

    llama_log_set(quiet_log_callback, nullptr);
    llama_backend_init();
    llama_model_params model_params = llama_model_default_params();
    model_params.n_gpu_layers = 0;
    model_params.use_mmap = true;
    model_params.check_tensors = true;
    model_params.use_extra_bufts = false;
    model_params.no_host = false;

    llama_model * model = llama_model_load_from_file(argv[1], model_params);
    if (model == nullptr) {
        std::fprintf(stderr, "failed to load pinned model %s\n", argv[1]);
        llama_backend_free();
        return 1;
    }

    capture_state state;
    llama_context_params context_params = llama_context_default_params();
    context_params.n_ctx = 128;
    context_params.n_batch = 1;
    context_params.n_ubatch = 1;
    context_params.n_seq_max = 1;
    context_params.n_threads = 1;
    context_params.n_threads_batch = 1;
    context_params.cb_eval = capture_callback;
    context_params.cb_eval_user_data = &state;
    context_params.offload_kqv = false;
    context_params.op_offload = false;
    context_params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;

    llama_context * context = llama_init_from_model(model, context_params);
    bool ok = context != nullptr;
    llama_token token = 1;
    if (ok) {
        ok = llama_decode(context, llama_batch_get_one(&token, 1)) == 0;
    }
    if (!state.error.empty()) {
        std::fprintf(stderr, "capture failed: %s\n", state.error.c_str());
        ok = false;
    }
    for (uint32_t index = 0; index < kCheckpointCount; ++index) {
        if (!state.checkpoints[index].seen) {
            std::array<char, 64> name = {};
            expected_name(index, name.data(), name.size());
            std::fprintf(stderr, "capture missed %s\n", name.data());
            ok = false;
        }
    }
    if (ok) {
        ok = write_trace(argv[2], state, static_cast<uint32_t>(token));
    }

    if (context != nullptr) {
        llama_free(context);
    }
    llama_model_free(model);
    llama_backend_free();
    return ok ? 0 : 1;
}
