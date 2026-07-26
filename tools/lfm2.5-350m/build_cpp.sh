#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source_dir="$script_dir/runtime/llama-b10075-src"
runtime_dir="$script_dir/runtime/llama-b10075"
output="$script_dir/runtime/lfm25-fixed"
llama_commit=76f46ad29d61fd8c1401e8221842934bf62a6064
opencl_include=${OPENCL_INCLUDE_DIR:-}
opencl_library=${OPENCL_LIBRARY:-}

if [[ -z "$opencl_include" ]]; then
    for candidate in \
        /usr/include \
        /opt/intel/oneapi/compiler/latest/include \
        "$script_dir/../../../blender-default-cube-toggle/lib/linux_x64/dpcpp/include"
    do
        if [[ -f "$candidate/CL/cl.h" ]]; then
            opencl_include="$candidate"
            break
        fi
    done
fi
if [[ -z "$opencl_library" ]]; then
    for candidate in \
        /usr/lib/x86_64-linux-gnu/libOpenCL.so \
        /usr/lib/x86_64-linux-gnu/libOpenCL.so.1 \
        /usr/local/lib/libOpenCL.so
    do
        if [[ -f "$candidate" ]]; then
            opencl_library="$candidate"
            break
        fi
    done
fi

if [[ ! -f "$script_dir/LFM2.5-350M-Q8_0.gguf" ||
      ! -f "$source_dir/include/llama.h" ||
      ! -f "$runtime_dir/libllama.so" ||
      ! -f "$opencl_include/CL/cl.h" ||
      ! -f "$opencl_library" ]]; then
    printf 'Pinned model, llama.cpp, or OpenCL development inputs are missing under %s.\n' \
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
    -I"$opencl_include" \
    "$script_dir/lfm25_fixed.cpp" \
    "$script_dir/lfm25_q8.cpp" \
    "$script_dir/lfm25_packed.cpp" \
    "$script_dir/lfm25_igpu.cpp" \
    -Wl,-rpath,'$ORIGIN/llama-b10075' \
    -Wl,-rpath-link,"$runtime_dir" \
    -Wl,-z,relro,-z,now \
    "$runtime_dir/libllama.so.0.0.10075" \
    "$runtime_dir/libggml.so.0.17.0" \
    -lcrypto \
    "$opencl_library" \
    -pthread \
    -o "$output"

printf 'Built fixed LFM2.5 userspace kernel: %s\n' "$output"
