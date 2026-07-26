#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
model_path="$script_dir/LFM2.5-350M-Q8_0.gguf"
llama_cli="$script_dir/runtime/llama-b10075/llama-cli"

if [[ ! -f "$model_path" || ! -x "$llama_cli" ]]; then
    printf 'Model or runtime missing. Run %s/download.sh first.\n' "$script_dir" >&2
    exit 1
fi

exec "$llama_cli" \
    --model "$model_path" \
    --ctx-size 32768 \
    --conversation \
    --temperature 0.1 \
    --top-k 50 \
    --repeat-penalty 1.05 \
    "$@"

