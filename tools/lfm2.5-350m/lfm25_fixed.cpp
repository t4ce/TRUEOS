#include "llama.h"
#include "ggml-backend.h"
#include "lfm25_q8.hpp"

#include <openssl/evp.h>

#include <algorithm>
#include <array>
#include <cerrno>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <sys/stat.h>
#include <thread>
#include <vector>

namespace {

// This is intentionally not a generic GGUF runner. These values are the
// admitted LFM2.5-350M-Q8_0 contract shared with TRUEOS and TRUEGA.
constexpr std::uintmax_t kModelBytes = 379'217'632;
constexpr std::string_view kModelSha256 =
    "be036a757295e550098b85e13f6af2735d0fa73b41e1156a40c7d8e8e32a5766";
constexpr std::uintmax_t kNativeImageBytes = 376'701'952;
constexpr std::string_view kNativeImageSha256 =
    "051c60856786de2ac7089109354259fa29fcd57e83d585efc86afa0fb605bb86";
constexpr std::uintmax_t kModelContractBytes = 3'744;
constexpr std::string_view kModelContractSha256 =
    "6b9f15fddf6a0198b77d0e339bd7978a38881f772520a43290bbea818fabc1c4";
constexpr std::uintmax_t kHiGoldenBytes = 23'709'296;
constexpr std::string_view kHiGoldenSha256 =
    "437ddd3bd6bbb94288fe40855b341767e5cb5f803122e84854fb579ad8feb407";
constexpr std::uintmax_t kF32SidecarBytes = 262'160;
constexpr std::string_view kF32SidecarSha256 =
    "a60c0d28e5e0f4830699260fbd9c01153763261a7b132a6b44610d64919609b1";
constexpr std::uintmax_t kIgcSpirvBytes = 62'288;
constexpr std::string_view kIgcSpirvSha256 =
    "66477ba6c412e5e01fafa1ee6cfdfae0ed43056bc3527ab8cd7e702316ea597b";
constexpr int32_t kVocabulary = 65'536;
constexpr uint32_t kContext = 1'024;
constexpr uint32_t kMaxPromptBytes = 512;
constexpr uint32_t kMaxReplyTokens = 32;
constexpr llama_token kHiFirstToken = 36'309;

constexpr std::array<llama_token, 4> kUserPrefix = {
    1,      // BOS
    6,      // <|im_start|>
    6'423,  // user
    708,    // newline
};
constexpr std::array<llama_token, 5> kAssistantSuffix = {
    7,       // <|im_end|>
    708,     // newline
    6,       // <|im_start|>
    64'015,  // assistant
    708,     // newline
};
constexpr std::array<llama_token, 10> kHiPromptTokens = {
    1, 6, 6'423, 708, 6'928, 7, 708, 6, 64'015, 708,
};
constexpr std::array<std::uint32_t, 10> kHiNextTokens = {
    1, 1, 1, 1, 1'463, 708, 774, 918, 797, 36'309,
};

struct llama_model_deleter {
    void operator()(llama_model * value) const {
        if (value != nullptr) {
            llama_model_free(value);
        }
    }
};

struct llama_context_deleter {
    void operator()(llama_context * value) const {
        if (value != nullptr) {
            llama_free(value);
        }
    }
};

using model_ptr = std::unique_ptr<llama_model, llama_model_deleter>;
using context_ptr = std::unique_ptr<llama_context, llama_context_deleter>;

struct backend_guard {
    explicit backend_guard(const std::filesystem::path & runtime_path) {
        ggml_backend_load_all_from_path(runtime_path.c_str());
        llama_backend_init();
    }

    ~backend_guard() {
        llama_backend_free();
    }

    backend_guard(const backend_guard &) = delete;
    backend_guard & operator=(const backend_guard &) = delete;
};

struct options {
    std::string prompt;
    uint32_t max_reply_tokens = kMaxReplyTokens;
    int32_t threads = 1;
    bool native = false;
    bool igpu = false;
    bool parity_hi = false;
    bool parity_q8 = false;
    bool parity_native_hi = false;
    bool parity_igpu_hi = false;
};

[[noreturn]] void fail(const std::string & message) {
    throw std::runtime_error(message);
}

void quiet_log_callback(ggml_log_level, const char *, void *) {
}

std::filesystem::path executable_path() {
    std::error_code error;
    auto result = std::filesystem::canonical("/proc/self/exe", error);
    if (error) {
        fail("cannot resolve /proc/self/exe: " + error.message());
    }
    return result;
}

std::filesystem::path fixed_model_path() {
    // build_cpp.sh publishes the executable as runtime/lfm25-fixed, directly
    // below the model directory.
    return executable_path().parent_path().parent_path() / "LFM2.5-350M-Q8_0.gguf";
}

std::string hex(const unsigned char * bytes, std::size_t length) {
    constexpr char digits[] = "0123456789abcdef";
    std::string result(length * 2, '\0');
    for (std::size_t index = 0; index < length; ++index) {
        result[index * 2] = digits[bytes[index] >> 4];
        result[index * 2 + 1] = digits[bytes[index] & 0x0f];
    }
    return result;
}

std::string sha256_file(const std::filesystem::path & path) {
    std::ifstream input(path, std::ios::binary);
    if (!input) {
        fail("cannot open pinned model for hashing: " + path.string());
    }

    std::unique_ptr<EVP_MD_CTX, decltype(&EVP_MD_CTX_free)> context(
        EVP_MD_CTX_new(), EVP_MD_CTX_free);
    if (!context || EVP_DigestInit_ex(context.get(), EVP_sha256(), nullptr) != 1) {
        fail("OpenSSL SHA-256 initialization failed");
    }

    std::array<char, 1 << 20> buffer{};
    while (input) {
        input.read(buffer.data(), static_cast<std::streamsize>(buffer.size()));
        const auto bytes = input.gcount();
        if (bytes > 0 &&
            EVP_DigestUpdate(context.get(), buffer.data(), static_cast<std::size_t>(bytes)) != 1) {
            fail("OpenSSL SHA-256 update failed");
        }
    }
    if (!input.eof()) {
        fail("failed while hashing pinned model: " + path.string());
    }

    std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
    unsigned int digest_bytes = 0;
    if (EVP_DigestFinal_ex(context.get(), digest.data(), &digest_bytes) != 1 ||
        digest_bytes != 32) {
        fail("OpenSSL SHA-256 finalization failed");
    }
    return hex(digest.data(), digest_bytes);
}

void verify_file(
    const std::filesystem::path & path,
    std::uintmax_t expected_bytes,
    std::string_view expected_sha256,
    std::string_view label) {
    std::error_code error;
    const auto bytes = std::filesystem::file_size(path, error);
    if (error) {
        fail("missing " + std::string(label) + " " + path.string() + ": " + error.message());
    }
    if (bytes != expected_bytes) {
        fail(
            std::string(label) + " byte count mismatch: observed=" + std::to_string(bytes) +
            " expected=" + std::to_string(expected_bytes));
    }
    const std::string digest = sha256_file(path);
    if (digest != expected_sha256) {
        fail(
            std::string(label) + " SHA-256 mismatch: observed=" + digest +
            " expected=" + std::string(expected_sha256));
    }
}

uint32_t parse_u32(const char * text, const char * name, uint32_t maximum) {
    if (text == nullptr || *text == '\0') {
        fail(std::string("missing ") + name);
    }
    errno = 0;
    char * end = nullptr;
    const unsigned long value = std::strtoul(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0' || value == 0 || value > maximum) {
        fail(std::string("invalid ") + name + ": " + text);
    }
    return static_cast<uint32_t>(value);
}

options parse_options(int argc, char ** argv) {
    options result;
    for (int index = 1; index < argc; ++index) {
        const std::string_view argument(argv[index]);
        if (argument == "--parity-hi") {
            result.parity_hi = true;
            result.prompt = "hi";
        } else if (argument == "--parity-q8") {
            result.parity_q8 = true;
        } else if (argument == "--parity-native-hi") {
            result.parity_native_hi = true;
            result.prompt = "hi";
        } else if (argument == "--parity-igpu-hi") {
            result.parity_igpu_hi = true;
            result.prompt = "hi";
        } else if (argument == "--native") {
            result.native = true;
        } else if (argument == "--igpu") {
            result.igpu = true;
        } else if (argument == "--max-tokens") {
            if (++index == argc) {
                fail("--max-tokens needs a value");
            }
            result.max_reply_tokens = parse_u32(argv[index], "reply-token count", kMaxReplyTokens);
        } else if (argument == "--threads") {
            if (++index == argc) {
                fail("--threads needs a value");
            }
            result.threads = static_cast<int32_t>(
                parse_u32(argv[index], "thread count", std::max(1u, std::thread::hardware_concurrency())));
        } else if (!argument.empty() && argument.front() == '-') {
            fail("unknown option: " + std::string(argument));
        } else if (
            result.prompt.empty() &&
            !result.parity_hi &&
            !result.parity_q8 &&
            !result.parity_native_hi &&
            !result.parity_igpu_hi) {
            result.prompt = std::string(argument);
        } else {
            fail("expected exactly one prompt");
        }
    }
    const unsigned parity_modes =
        static_cast<unsigned>(result.parity_hi) +
        static_cast<unsigned>(result.parity_q8) +
        static_cast<unsigned>(result.parity_native_hi) +
        static_cast<unsigned>(result.parity_igpu_hi);
    if (parity_modes > 1 ||
        ((result.native || result.igpu) && parity_modes != 0) ||
        (result.native && result.igpu)) {
        fail("choose one parity mode");
    }
    if (result.prompt.empty() && !result.parity_q8) {
        fail(
            "usage: lfm25-fixed [--threads N] [--max-tokens N] PROMPT\n"
            "       lfm25-fixed --native [--threads N] [--max-tokens N] PROMPT\n"
            "       lfm25-fixed --igpu [--max-tokens N] PROMPT\n"
            "       lfm25-fixed --parity-hi\n"
            "       lfm25-fixed [--threads N] --parity-q8\n"
            "       lfm25-fixed [--threads N] --parity-native-hi\n"
            "       lfm25-fixed --parity-igpu-hi");
    }
    if (result.prompt.size() > kMaxPromptBytes) {
        fail("prompt exceeds the fixed 512-byte TRUEOS shell contract");
    }
    return result;
}

std::vector<llama_token> tokenize_text(const llama_vocab * vocabulary, std::string_view text) {
    const int32_t text_bytes = static_cast<int32_t>(text.size());
    int32_t required = llama_tokenize(
        vocabulary, text.data(), text_bytes, nullptr, 0, false, false);
    if (required == INT32_MIN) {
        fail("prompt token count overflow");
    }
    if (required < 0) {
        required = -required;
    }
    std::vector<llama_token> result(static_cast<std::size_t>(required));
    const int32_t observed = llama_tokenize(
        vocabulary,
        text.data(),
        text_bytes,
        result.data(),
        static_cast<int32_t>(result.size()),
        false,
        false);
    if (observed < 0) {
        fail("prompt tokenization failed");
    }
    result.resize(static_cast<std::size_t>(observed));
    return result;
}

std::vector<llama_token> encode_user_turn(
    const llama_vocab * vocabulary,
    std::string_view prompt) {
    const auto prompt_tokens = tokenize_text(vocabulary, prompt);
    std::vector<llama_token> result;
    result.reserve(kUserPrefix.size() + prompt_tokens.size() + kAssistantSuffix.size());
    result.insert(result.end(), kUserPrefix.begin(), kUserPrefix.end());
    result.insert(result.end(), prompt_tokens.begin(), prompt_tokens.end());
    result.insert(result.end(), kAssistantSuffix.begin(), kAssistantSuffix.end());
    return result;
}

std::string token_piece(const llama_vocab * vocabulary, llama_token token) {
    std::array<char, 256> local{};
    int32_t bytes = llama_token_to_piece(
        vocabulary, token, local.data(), static_cast<int32_t>(local.size()), 0, false);
    if (bytes >= 0) {
        return std::string(local.data(), static_cast<std::size_t>(bytes));
    }
    if (bytes == INT32_MIN) {
        fail("token piece length overflow");
    }
    std::string result(static_cast<std::size_t>(-bytes), '\0');
    bytes = llama_token_to_piece(
        vocabulary, token, result.data(), static_cast<int32_t>(result.size()), 0, false);
    if (bytes < 0) {
        fail("detokenization failed for token " + std::to_string(token));
    }
    result.resize(static_cast<std::size_t>(bytes));
    return result;
}

llama_token argmax(const llama_context * context) {
    const float * logits = llama_get_logits_ith(const_cast<llama_context *>(context), -1);
    if (logits == nullptr) {
        fail("model returned no logits");
    }
    llama_token best = 0;
    for (llama_token token = 1; token < kVocabulary; ++token) {
        if (logits[token] > logits[best]) {
            best = token;
        }
    }
    return best;
}

void decode_one(llama_context * context, llama_token token) {
    if (llama_decode(context, llama_batch_get_one(&token, 1)) != 0) {
        fail("fixed one-token llama_decode failed");
    }
}

int run(const options & arguments) {
    const auto started = std::chrono::steady_clock::now();
    const auto tool_path = executable_path().parent_path().parent_path();
    if (arguments.parity_q8) {
        const auto repository_path = tool_path.parent_path().parent_path();
        const auto native_path = tool_path / "LFM2.5-350M-Q8_0.truega.bin";
        const auto contract_path =
            repository_path /
            "crates/trueos-fpga-abi/truega/artifacts/lfm25_model.contract.bin";
        const auto golden_path =
            repository_path /
            "crates/trueos-fpga-abi/truega/artifacts/lfm25_hi_decode.golden.bin";
        verify_file(
            native_path,
            kNativeImageBytes,
            kNativeImageSha256,
            "pinned native image");
        verify_file(
            contract_path,
            kModelContractBytes,
            kModelContractSha256,
            "pinned model contract");
        verify_file(
            golden_path,
            kHiGoldenBytes,
            kHiGoldenSha256,
            "pinned hi golden");
        trueos::lfm25::verify_q8_kernel(
            native_path,
            contract_path,
            golden_path,
            static_cast<std::uint32_t>(arguments.threads));
        return 0;
    }
    const auto model_path = fixed_model_path();
    verify_file(model_path, kModelBytes, kModelSha256, "pinned GGUF");

    llama_log_set(quiet_log_callback, nullptr);
    backend_guard backend(executable_path().parent_path() / "llama-b10075");

    llama_model_params model_params = llama_model_default_params();
    model_params.vocab_only =
        arguments.parity_native_hi ||
        arguments.parity_igpu_hi ||
        arguments.native ||
        arguments.igpu;
    model_params.n_gpu_layers = 0;
    model_params.use_mmap = true;
    model_params.check_tensors = false; // Complete-file SHA-256 was checked above.
    model_params.use_extra_bufts = true; // Select the installed Intel CPU repack backend.
    model_params.no_host = false;

    model_ptr model(llama_model_load_from_file(model_path.c_str(), model_params));
    if (!model) {
        fail("llama.cpp b10075 rejected the sealed LFM2.5 model");
    }
    const llama_vocab * vocabulary = llama_model_get_vocab(model.get());
    if (vocabulary == nullptr || llama_vocab_n_tokens(vocabulary) != kVocabulary) {
        fail("model vocabulary is not the fixed 65536-token LFM2.5 contract");
    }

    const std::vector<llama_token> prompt_tokens =
        encode_user_turn(vocabulary, arguments.prompt);
    if (prompt_tokens.size() + arguments.max_reply_tokens >= kContext) {
        fail("tokenized prompt exceeds the fixed userspace context");
    }
    if (arguments.prompt == "hi" &&
        !std::equal(prompt_tokens.begin(), prompt_tokens.end(), kHiPromptTokens.begin(), kHiPromptTokens.end())) {
        fail("sealed tokenizer parity failed for the fixed hi prompt");
    }

    if (
        arguments.parity_native_hi ||
        arguments.parity_igpu_hi ||
        arguments.native ||
        arguments.igpu) {
        const auto repository_path = tool_path.parent_path().parent_path();
        const auto native_path = tool_path / "LFM2.5-350M-Q8_0.truega.bin";
        const auto contract_path =
            repository_path /
            "crates/trueos-fpga-abi/truega/artifacts/lfm25_model.contract.bin";
        const auto sidecar_path = tool_path / "LFM2.5-350M-Q8_0.cpu-f32.bin";
        const auto igc_spirv_path =
            repository_path /
            "crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/"
            "lfm25_q8_project.spv";
        verify_file(
            native_path,
            kNativeImageBytes,
            kNativeImageSha256,
            "pinned native image");
        verify_file(
            contract_path,
            kModelContractBytes,
            kModelContractSha256,
            "pinned model contract");
        verify_file(
            sidecar_path,
            kF32SidecarBytes,
            kF32SidecarSha256,
            "pinned F32 sidecar");
        if (arguments.parity_igpu_hi || arguments.igpu) {
            verify_file(
                igc_spirv_path,
                kIgcSpirvBytes,
                kIgcSpirvSha256,
                "published C++/IGC SPIR-V");
        }

        std::vector<std::uint32_t> native_tokens;
        native_tokens.reserve(prompt_tokens.size());
        for (llama_token token : prompt_tokens) {
            native_tokens.push_back(static_cast<std::uint32_t>(token));
        }
        std::vector<std::uint32_t> stop_tokens;
        for (llama_token token = 0; token < kVocabulary; ++token) {
            if (llama_vocab_is_eog(vocabulary, token)) {
                stop_tokens.push_back(static_cast<std::uint32_t>(token));
            }
        }
        if (stop_tokens.empty()) {
            fail("sealed vocabulary has no end-of-generation token");
        }
        const bool parity_custom_hi =
            arguments.parity_native_hi || arguments.parity_igpu_hi;
        const bool use_igpu =
            arguments.igpu || arguments.parity_igpu_hi;
        const auto result = trueos::lfm25::run_native_decode(
            native_path,
            contract_path,
            sidecar_path,
            native_tokens,
            parity_custom_hi ? 0 : arguments.max_reply_tokens,
            stop_tokens,
            static_cast<std::uint32_t>(arguments.threads),
            use_igpu
                ? trueos::lfm25::native_projection_backend::intel_igc
                : trueos::lfm25::native_projection_backend::cpu_avx2,
            use_igpu ? igc_spirv_path : std::filesystem::path{});
        if (parity_custom_hi &&
            !std::equal(
                result.next_tokens.begin(),
                result.next_tokens.end(),
                kHiNextTokens.begin(),
                kHiNextTokens.end())) {
            std::fprintf(stderr, "lfm25-fixed: native token trace observed=");
            for (std::uint32_t token : result.next_tokens) {
                std::fprintf(stderr, "%u,", token);
            }
            std::fputc('\n', stderr);
            fail("native C++ hi token trace differs from the sealed b10075 oracle");
        }
        std::string reply;
        if (parity_custom_hi) {
            reply =
                token_piece(vocabulary, static_cast<llama_token>(result.next_tokens.back()));
        } else {
            for (std::uint32_t token : result.generated_tokens) {
                reply += token_piece(vocabulary, static_cast<llama_token>(token));
            }
        }
        std::fwrite(reply.data(), 1, reply.size(), stdout);
        std::fputc('\n', stdout);
        const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
            std::chrono::steady_clock::now() - started);
        if (parity_custom_hi) {
            std::fprintf(
                stderr,
                "lfm25-fixed: PASS %s-hi prompt_tokens=10 decisions=10 "
                "first_reply_token=36309 decoded=Hello threads=%d elapsed_ms=%lld "
                "projection_device=\"%s\" projection_launches=%llu "
                "projection_kernel_ms=%.3f\n",
                use_igpu ? "igpu" : "native",
                arguments.threads,
                static_cast<long long>(elapsed.count()),
                result.projection_device.c_str(),
                static_cast<unsigned long long>(result.projection_launches),
                static_cast<double>(result.projection_nanoseconds) / 1'000'000.0);
        } else {
            std::fprintf(
                stderr,
                "lfm25-fixed: prompt_tokens=%zu first_token=%u reply_tokens=%zu "
                "stop=%s threads=%d elapsed_ms=%lld projection_device=\"%s\" "
                "projection_launches=%llu projection_kernel_ms=%.3f\n",
                prompt_tokens.size(),
                result.next_tokens.at(prompt_tokens.size() - 1),
                result.generated_tokens.size(),
                result.stopped ? "eot" : "limit",
                arguments.threads,
                static_cast<long long>(elapsed.count()),
                result.projection_device.c_str(),
                static_cast<unsigned long long>(result.projection_launches),
                static_cast<double>(result.projection_nanoseconds) / 1'000'000.0);
        }
        return 0;
    }

    llama_context_params context_params = llama_context_default_params();
    context_params.n_ctx = kContext;
    context_params.n_batch = 1;
    context_params.n_ubatch = 1;
    context_params.n_seq_max = 1;
    context_params.n_threads = arguments.threads;
    context_params.n_threads_batch = arguments.threads;
    context_params.type_k = GGML_TYPE_F16;
    context_params.type_v = GGML_TYPE_F16;
    context_params.offload_kqv = false;
    context_params.op_offload = false;
    context_params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_DISABLED;
    context_params.no_perf = false;

    context_ptr context(llama_init_from_model(model.get(), context_params));
    if (!context) {
        fail("failed to allocate the fixed 1024-token CPU context");
    }

    for (llama_token token : prompt_tokens) {
        decode_one(context.get(), token);
    }

    const llama_token first_token = argmax(context.get());
    std::vector<llama_token> generated;
    generated.reserve(arguments.max_reply_tokens);
    llama_token next_token = first_token;
    bool stopped = false;
    for (uint32_t index = 0; index < arguments.max_reply_tokens; ++index) {
        if (llama_vocab_is_eog(vocabulary, next_token)) {
            stopped = true;
            break;
        }
        generated.push_back(next_token);
        if (index + 1 == arguments.max_reply_tokens) {
            break;
        }
        decode_one(context.get(), next_token);
        next_token = argmax(context.get());
    }

    std::string reply;
    for (llama_token token : generated) {
        reply += token_piece(vocabulary, token);
    }
    std::fwrite(reply.data(), 1, reply.size(), stdout);
    std::fputc('\n', stdout);

    const auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now() - started);
    std::fprintf(
        stderr,
        "lfm25-fixed: prompt_tokens=%zu first_token=%d reply_tokens=%zu stop=%s "
        "threads=%d elapsed_ms=%lld backend=llama-b10075/intel-cpu\n",
        prompt_tokens.size(),
        first_token,
        generated.size(),
        stopped ? "eot" : "limit",
        arguments.threads,
        static_cast<long long>(elapsed.count()));

    if (arguments.parity_hi) {
        if (prompt_tokens.size() != kHiPromptTokens.size() ||
            first_token != kHiFirstToken ||
            !reply.starts_with("Hello")) {
            std::fprintf(
                stderr,
                "lfm25-fixed: FAIL hi expected_tokens=10 expected_first_token=36309 "
                "expected_prefix=Hello\n");
            return 1;
        }
        std::fprintf(
            stderr,
            "lfm25-fixed: PASS hi tokens=10 first_token=36309 decoded_prefix=Hello\n");
    }
    return 0;
}

} // namespace

int main(int argc, char ** argv) {
    try {
        return run(parse_options(argc, argv));
    } catch (const std::exception & error) {
        std::fprintf(stderr, "lfm25-fixed: %s\n", error.what());
        return 1;
    }
}
