#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
"$script_dir/build_cpp.sh"

output=$("$script_dir/runtime/lfm25-fixed" --parity-hi --max-tokens 1)
if [[ "$output" != "Hello" ]]; then
    printf 'lfm25-fixed parity output mismatch: observed=%q expected=Hello\n' "$output" >&2
    exit 1
fi

"$script_dir/runtime/lfm25-fixed" --parity-q8 --threads 1
"$script_dir/runtime/lfm25-fixed" --parity-q8-packed --threads 1

native_output=$("$script_dir/runtime/lfm25-fixed" --parity-native-hi --threads 1)
if [[ "$native_output" != "Hello" ]]; then
    printf 'lfm25-fixed native parity output mismatch: observed=%q expected=Hello\n' \
        "$native_output" >&2
    exit 1
fi

expected_hi_ai='Hello! How can I help you today?'
native_hi_ai=$(
    "$script_dir/runtime/lfm25-fixed" --native "hi ai" --max-tokens 32 --threads 1
)
if [[ "$native_hi_ai" != "$expected_hi_ai" ]]; then
    printf 'lfm25-fixed native hi-ai output mismatch: observed=%q\n' "$native_hi_ai" >&2
    exit 1
fi

oracle_hi_ai=$("$script_dir/runtime/lfm25-fixed" "hi ai" --max-tokens 32 --threads 1)
if [[ "$oracle_hi_ai" != "$expected_hi_ai" ]]; then
    printf 'lfm25-fixed oracle hi-ai output mismatch: observed=%q\n' "$oracle_hi_ai" >&2
    exit 1
fi
if [[ "$native_hi_ai" != "$oracle_hi_ai" ]]; then
    printf 'lfm25-fixed native/oracle hi-ai mismatch\n' >&2
    exit 1
fi

printf 'PASS fixed C++ LFM2.5 userspace parity: hi -> token 36309 -> Hello\n'
printf 'PASS fixed C++ Q8_0 projections: layer-0 gate/up/down match sealed b10075 checkpoints\n'
printf 'PASS graph-native packed Q8_0 layout: all 93 tensors admitted and layer-0 FFN matches\n'
printf 'PASS native C++ full-model prefill: all 10 hi token decisions match sealed b10075\n'
printf 'PASS native C++ greedy reply equals b10075: hi ai -> %s\n' "$expected_hi_ai"
