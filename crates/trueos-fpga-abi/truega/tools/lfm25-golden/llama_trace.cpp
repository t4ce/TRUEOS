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

constexpr std::array<const char *, 5> kTensorNames = {
    "model.layers.{}.ffn_norm-0",
    "ffn_gate-0",
    "ffn_up-0",
    "ffn_swiglu-0",
    // LFM2 gives the post-down tensor its architecture-level name after build_ffn.
    "model.layers.{}.ffn_out-0",
};

constexpr std::array<const char *, 5> kArtifactNames = {
    "normalized_input",
    "gate_projection",
    "up_projection",
    "silu_gate_mul_up",
    "down_projection",
};

constexpr std::array<uint32_t, 5> kElementCounts = { 1024, 4608, 4608, 4608, 1024 };

struct capture_state {
    std::array<std::vector<float>, 5> vectors;
    std::array<bool, 5> seen = {};
    std::string error;
};

int target_index(const char * name) {
    for (size_t i = 0; i < kTensorNames.size(); ++i) {
        if (std::strcmp(name, kTensorNames[i]) == 0) {
            return static_cast<int>(i);
        }
    }
    return -1;
}

bool capture_callback(ggml_tensor * tensor, bool ask, void * opaque) {
    auto & state = *static_cast<capture_state *>(opaque);
    const int index = target_index(tensor->name);

    if (ask) {
        return index >= 0;
    }
    if (index < 0 || !state.error.empty()) {
        return true;
    }
    if (state.seen[index]) {
        state.error = std::string("tensor was evaluated more than once: ") + tensor->name;
        return true;
    }
    if (tensor->type != GGML_TYPE_F32 || !ggml_is_contiguous(tensor)) {
        state.error = std::string("capture tensor is not contiguous F32: ") + tensor->name;
        return true;
    }
    if (ggml_nelements(tensor) != kElementCounts[index]) {
        state.error = std::string("unexpected element count for ") + tensor->name;
        return true;
    }

    auto & output = state.vectors[index];
    output.resize(kElementCounts[index]);
    ggml_backend_tensor_get(tensor, output.data(), 0, output.size() * sizeof(float));
    state.seen[index] = true;
    return true;
}

bool write_bytes(FILE * file, const void * data, size_t bytes) {
    return std::fwrite(data, 1, bytes, file) == bytes;
}

bool write_u32_le(FILE * file, uint32_t value) {
    const uint8_t bytes[4] = {
        static_cast<uint8_t>(value),
        static_cast<uint8_t>(value >> 8),
        static_cast<uint8_t>(value >> 16),
        static_cast<uint8_t>(value >> 24),
    };
    return write_bytes(file, bytes, sizeof(bytes));
}

bool write_trace(const char * path, const capture_state & state, int32_t token) {
    FILE * file = std::fopen(path, "wb");
    if (file == nullptr) {
        std::fprintf(stderr, "cannot create %s: %s\n", path, std::strerror(errno));
        return false;
    }

    bool ok = write_bytes(file, "TGALRAW1", 8) &&
              write_u32_le(file, 1) &&
              write_u32_le(file, static_cast<uint32_t>(token)) &&
              write_u32_le(file, static_cast<uint32_t>(state.vectors.size())) &&
              write_u32_le(file, 0);

    for (size_t i = 0; ok && i < state.vectors.size(); ++i) {
        std::array<char, 32> name = {};
        std::strncpy(name.data(), kArtifactNames[i], name.size() - 1);
        ok = write_bytes(file, name.data(), name.size()) &&
             write_u32_le(file, kElementCounts[i]) &&
             write_u32_le(file, 0);
        for (float value : state.vectors[i]) {
            uint32_t bits = 0;
            static_assert(sizeof(bits) == sizeof(value), "F32 size");
            std::memcpy(&bits, &value, sizeof(bits));
            if (!write_u32_le(file, bits)) {
                ok = false;
                break;
            }
        }
    }

    if (std::fclose(file) != 0) {
        ok = false;
    }
    if (!ok) {
        std::fprintf(stderr, "failed while writing %s\n", path);
    }
    return ok;
}

void quiet_log_callback(ggml_log_level, const char *, void *) {
}

} // namespace

int main(int argc, char ** argv) {
    if (argc != 3) {
        std::fprintf(stderr, "usage: %s MODEL.gguf TRACE.raw\n", argv[0]);
        return 2;
    }

    llama_log_set(quiet_log_callback, nullptr);
    llama_backend_init();

    llama_model_params model_params = llama_model_default_params();
    model_params.n_gpu_layers = 0;
    model_params.use_mmap = true;
    model_params.use_mlock = false;
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
    if (context == nullptr) {
        std::fprintf(stderr, "failed to create deterministic CPU context\n");
        llama_model_free(model);
        llama_backend_free();
        return 1;
    }

    llama_token token = 1;
    const int32_t decode_result = llama_decode(context, llama_batch_get_one(&token, 1));
    bool ok = decode_result == 0;
    if (!ok) {
        std::fprintf(stderr, "llama_decode failed: %d\n", decode_result);
    }
    if (!state.error.empty()) {
        std::fprintf(stderr, "capture failed: %s\n", state.error.c_str());
        ok = false;
    }
    for (size_t i = 0; i < state.seen.size(); ++i) {
        if (!state.seen[i]) {
            std::fprintf(stderr, "capture missed %s\n", kTensorNames[i]);
            ok = false;
        }
    }
    if (ok) {
        ok = write_trace(argv[2], state, token);
    }

    llama_free(context);
    llama_model_free(model);
    llama_backend_free();
    return ok ? 0 : 1;
}
