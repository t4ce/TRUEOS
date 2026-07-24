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

hi_ai=$("$script_dir/runtime/lfm25-fixed" "hi ai" --max-tokens 32 --threads 1)
if [[ "$hi_ai" != "Hello! How can I help you today?" ]]; then
    printf 'lfm25-fixed hi-ai output mismatch: observed=%q\n' "$hi_ai" >&2
    exit 1
fi

printf 'PASS fixed C++ LFM2.5 userspace parity: hi -> token 36309 -> Hello\n'
printf 'PASS fixed C++ Q8_0 projections: layer-0 gate/up/down match sealed b10075 checkpoints\n'
printf 'PASS fixed C++ greedy reply: hi ai -> Hello! How can I help you today?\n'
