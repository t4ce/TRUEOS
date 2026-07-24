#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source_dir="$script_dir/runtime/llama-b10075-src"
runtime_dir="$script_dir/runtime/llama-b10075"
output="$script_dir/runtime/lfm25-fixed"
llama_commit=76f46ad29d61fd8c1401e8221842934bf62a6064

if [[ ! -f "$script_dir/LFM2.5-350M-Q8_0.gguf" ||
      ! -f "$source_dir/include/llama.h" ||
      ! -f "$runtime_dir/libllama.so" ]]; then
    printf 'Pinned model, llama.cpp headers, or runtime libraries are missing under %s.\n' \
        "$script_dir" >&2
    exit 1
fi

actual_commit=$(git -C "$source_dir" rev-parse HEAD 2>/dev/null || true)
if [[ "$actual_commit" != "$llama_commit" ]]; then
    printf 'llama.cpp headers are commit %s, expected pinned %s.\n' \
        "${actual_commit:-missing}" "$llama_commit" >&2
    exit 1
fi
if ! git -C "$source_dir" diff --quiet -- include ggml/include; then
    printf 'Pinned llama.cpp public headers have local modifications.\n' >&2
    exit 1
fi

cxx=${CXX:-g++}
"$cxx" \
    -std=c++20 \
    -O3 \
    -DNDEBUG \
    -Wall \
    -Wextra \
    -Wpedantic \
    -Werror \
    -mavx2 \
    -mf16c \
    -mfma \
    -I"$source_dir/include" \
    -I"$source_dir/ggml/include" \
    "$script_dir/lfm25_fixed.cpp" \
    "$script_dir/lfm25_q8.cpp" \
    -Wl,-rpath,'$ORIGIN/llama-b10075' \
    -Wl,-rpath-link,"$runtime_dir" \
    -Wl,-z,relro,-z,now \
    "$runtime_dir/libllama.so.0.0.10075" \
    "$runtime_dir/libggml.so.0.17.0" \
    -lcrypto \
    -pthread \
    -o "$output"

printf 'Built fixed LFM2.5 userspace kernel: %s\n' "$output"
