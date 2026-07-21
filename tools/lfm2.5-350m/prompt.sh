#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

if [[ $# -eq 0 ]]; then
    printf 'Usage: %s PROMPT [llama.cpp options...]\n' "$0" >&2
    exit 2
fi

prompt=$1
shift
exec "$script_dir/chat.sh" --single-turn --prompt "$prompt" "$@"

