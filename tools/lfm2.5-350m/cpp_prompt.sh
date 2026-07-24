#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
binary="$script_dir/runtime/lfm25-fixed"

if [[ ! -x "$binary" ]]; then
    "$script_dir/build_cpp.sh"
fi

exec "$binary" "$@"
