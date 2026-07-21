#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
model_path="$script_dir/LFM2.5-350M-Q8_0.gguf"
llama_server="$script_dir/runtime/llama-b10075/llama-server"

if [[ ! -f "$model_path" || ! -x "$llama_server" ]]; then
    printf 'Model or runtime missing. Run %s/download.sh first.\n' "$script_dir" >&2
    exit 1
fi

exec "$llama_server" \
    --model "$model_path" \
    --ctx-size 32768 \
    --host 127.0.0.1 \
    --port 8080 \
    --temperature 0.1 \
    --top-k 50 \
    --repeat-penalty 1.05 \
    "$@"
