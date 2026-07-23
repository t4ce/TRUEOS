#!/usr/bin/env bash
set -euo pipefail

EXP_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
CLI="$EXP_ROOT/.venv/bin/lilly-exp"

if [[ ! -x "$CLI" ]]; then
    echo "Lilly_Exp is not bootstrapped. Run $EXP_ROOT/setup.sh first." >&2
    exit 1
fi

exec "$CLI" "$@"

