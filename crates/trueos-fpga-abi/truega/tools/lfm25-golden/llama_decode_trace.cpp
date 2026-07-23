#include "llama.h"
#include "ggml.h"
#include "ggml-backend.h"

#include <array>
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {

constexpr uint32_t kLayers = 16;
constexpr uint32_t kHidden = 1024;
constexpr uint32_t kFfn = 4608;
constexpr uint32_t kKv = 512;
constexpr uint32_t kVocabulary = 65536;
constexpr uint32_t kHeaderBytes = 256;
constexpr std::array<uint32_t, 10> kTokens = {
    1, 6, 6423, 708, 6928, 7, 708, 6, 64015, 708,
};
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

struct checkpoint_spec {
    std::array<char, 64> name = {};
    uint32_t elements = 0;
};

struct checkpoint {
    checkpoint_spec spec;
    std::vector<float> values;
    bool seen = false;
};

struct capture_state {
    std::vector<std::vector<checkpoint>> tokens;
    size_t current_token = 0;
    std::string error;
};

checkpoint_spec spec(const char * format, int layer, uint32_t elements) {
    checkpoint_spec result;
    if (layer < 0) {
        std::snprintf(result.name.data(), result.name.size(), "%s", format);
    } else {
        std::snprintf(result.name.data(), result.name.size(), format, static_cast<unsigned>(layer));
    }
    result.elements = elements;
    return result;
}

std::vector<checkpoint_spec> checkpoint_specs() {
    std::vector<checkpoint_spec> result;
    result.push_back(spec("model.embed_tokens", -1, kHidden));
    for (uint32_t layer = 0; layer < kLayers; ++layer) {
        result.push_back(spec("model.layers.{}.operator_norm-%u", layer, kHidden));
        if (kAttention[layer]) {
            result.push_back(spec("Qcur-%u", layer, kHidden));
            result.push_back(spec("Kcur-%u", layer, kKv));
            result.push_back(spec("Vcur-%u", layer, kKv));
            result.push_back(
                spec("model.layers.{}.self_attn.q_layernorm-%u", layer, kHidden));
            result.push_back(
                spec("model.layers.{}.self_attn.k_layernorm-%u", layer, kKv));
            result.push_back(spec("model.layers.{}.self_attn.q_rope-%u", layer, kHidden));
            result.push_back(spec("model.layers.{}.self_attn.k_rope-%u", layer, kKv));
            result.push_back(spec("kqv_out-%u", layer, kHidden));
            result.push_back(
                spec("model.layers.{}.self_attn.out_proj-%u", layer, kHidden));
        } else {
            result.push_back(spec("model.layers.{}.conv.in_proj-%u", layer, 3 * kHidden));
            result.push_back(spec("model.layers.{}.conv.bx-%u", layer, 3 * kHidden));
            result.push_back(spec("model.layers.{}.conv.state-%u", layer, 2 * kHidden));
            result.push_back(spec("model.layers.{}.conv.conv-%u", layer, kHidden));
            result.push_back(spec("model.layers.{}.conv.mix-%u", layer, kHidden));
            result.push_back(spec("model.layers.{}.conv.out_proj-%u", layer, kHidden));
        }
        result.push_back(spec("model.layers.{}.operator_residual-%u", layer, kHidden));
        result.push_back(spec("model.layers.{}.ffn_norm-%u", layer, kHidden));
        result.push_back(spec("ffn_up-%u", layer, kFfn));
        result.push_back(spec("ffn_gate-%u", layer, kFfn));
        result.push_back(spec("ffn_silu-%u", layer, kFfn));
        result.push_back(spec("ffn_gate_par-%u", layer, kFfn));
        result.push_back(spec("model.layers.{}.ffn_out-%u", layer, kHidden));
        result.push_back(spec("l_out-%u", layer, kHidden));
    }
    result.push_back(spec("result_norm", -1, kHidden));
    result.push_back(spec("result_output", -1, kVocabulary));
    return result;
}

capture_state make_capture_state() {
    capture_state state;
    const auto specs = checkpoint_specs();
    state.tokens.resize(kTokens.size());
    for (auto & token : state.tokens) {
        token.reserve(specs.size());
        for (const auto & item : specs) {
            checkpoint value;
            value.spec = item;
            token.push_back(std::move(value));
        }
    }
    return state;
}

bool capture_callback(ggml_tensor * tensor, bool ask, void * opaque) {
    auto & state = *static_cast<capture_state *>(opaque);
    auto & checkpoints = state.tokens[state.current_token];
    auto found = checkpoints.end();
    for (auto it = checkpoints.begin(); it != checkpoints.end(); ++it) {
        if (std::strcmp(tensor->name, it->spec.name.data()) == 0) {
            found = it;
            break;
        }
    }
    if (ask) {
        return found != checkpoints.end();
    }
    if (found == checkpoints.end() || !state.error.empty()) {
        return true;
    }
    if (found->seen) {
        state.error = std::string("checkpoint evaluated twice: ") + tensor->name;
        return true;
    }
    if (tensor->type != GGML_TYPE_F32 || !ggml_is_contiguous(tensor) ||
        ggml_nelements(tensor) != found->spec.elements) {
        state.error = std::string("checkpoint shape/type mismatch: ") + tensor->name;
        return true;
    }
    found->values.resize(found->spec.elements);
    ggml_backend_tensor_get(
        tensor, found->values.data(), 0, found->values.size() * sizeof(float));
    found->seen = true;
    return true;
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

void header_u32(std::array<uint8_t, kHeaderBytes> & header, size_t offset, uint32_t value) {
    header[offset + 0] = static_cast<uint8_t>(value);
    header[offset + 1] = static_cast<uint8_t>(value >> 8);
    header[offset + 2] = static_cast<uint8_t>(value >> 16);
    header[offset + 3] = static_cast<uint8_t>(value >> 24);
}

bool write_trace(const char * path, const capture_state & state) {
    FILE * file = std::fopen(path, "wb");
    if (file == nullptr) {
        std::fprintf(stderr, "cannot create %s: %s\n", path, std::strerror(errno));
        return false;
    }
    const uint32_t checkpoints_per_token =
        static_cast<uint32_t>(state.tokens.front().size());
    std::array<uint8_t, kHeaderBytes> header = {};
    std::memcpy(header.data(), "TGALDE2\0", 8);
    header_u32(header, 8, 2);
    header_u32(header, 12, kHeaderBytes);
    header_u32(header, 16, static_cast<uint32_t>(kTokens.size()));
    header_u32(header, 20, checkpoints_per_token);
    header_u32(header, 24, checkpoints_per_token * kTokens.size());
    for (size_t index = 0; index < kTokens.size(); ++index) {
        header_u32(header, 32 + index * 4, kTokens[index]);
        header_u32(
            header, 72 + index * 4, argmax_token(state.tokens[index].back().values));
    }
    std::memcpy(header.data() + 112, kCommit, 40);
    std::memcpy(header.data() + 152, kGgufSha256.data(), kGgufSha256.size());
    std::memcpy(header.data() + 184, kNativeSha256.data(), kNativeSha256.size());
    std::memcpy(header.data() + 216, kContractSha256.data(), kContractSha256.size());

    bool ok = write_bytes(file, header.data(), header.size());
    for (const auto & token : state.tokens) {
        for (const auto & value : token) {
            ok = ok && write_bytes(file, value.spec.name.data(), value.spec.name.size()) &&
                 write_u32(file, static_cast<uint32_t>(value.values.size())) &&
                 write_u32(file, static_cast<uint32_t>(value.values.size() * sizeof(float)));
            for (float element : value.values) {
                uint32_t bits = 0;
                std::memcpy(&bits, &element, sizeof(bits));
                ok = ok && write_u32(file, bits);
            }
        }
    }
    ok = std::fclose(file) == 0 && ok;
    if (ok) {
        std::fprintf(
            stderr,
            "truega-decode-trace tokens=%zu checkpoints_per_token=%u total=%u final_token=%u path=%s\n",
            kTokens.size(), checkpoints_per_token, checkpoints_per_token * kTokens.size(),
            argmax_token(state.tokens.back().back().values), path);
    }
    return ok;
}

bool all_seen(const capture_state & state, size_t token) {
    bool ok = true;
    for (const auto & checkpoint : state.tokens[token]) {
        if (!checkpoint.seen) {
            std::fprintf(
                stderr, "capture token=%zu missed %s\n", token, checkpoint.spec.name.data());
            ok = false;
        }
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

    capture_state state = make_capture_state();
    llama_context_params context_params = llama_context_default_params();
    context_params.n_ctx = 128;
    context_params.n_batch = 1;
    context_params.n_ubatch = 1;
    context_params.n_seq_max = 1;
    context_params.n_threads = 1;
    context_params.n_threads_batch = 1;
    context_params.type_k = GGML_TYPE_F16;
    context_params.type_v = GGML_TYPE_F16;
    context_params.cb_eval = capture_callback;
    context_params.cb_eval_user_data = &state;
    context_params.offload_kqv = false;
    context_params.op_offload = false;
    context_params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;

    llama_context * context = llama_init_from_model(model, context_params);
    bool ok = context != nullptr;
    for (size_t index = 0; ok && index < kTokens.size(); ++index) {
        state.current_token = index;
        llama_token token = static_cast<llama_token>(kTokens[index]);
        ok = llama_decode(context, llama_batch_get_one(&token, 1)) == 0;
        if (!state.error.empty()) {
            std::fprintf(stderr, "capture token=%zu failed: %s\n", index, state.error.c_str());
            ok = false;
        }
        ok = all_seen(state, index) && ok;
    }
    if (ok) {
        ok = write_trace(argv[2], state);
    }

    if (context != nullptr) {
        llama_free(context);
    }
    llama_model_free(model);
    llama_backend_free();
    return ok ? 0 : 1;
}
