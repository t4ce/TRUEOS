#!/usr/bin/env bash
set -euo pipefail

EXP_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
PYTHON="$EXP_ROOT/.venv/bin/python"

if [[ ! -x "$PYTHON" ]]; then
    echo "FILM runtime is missing. Run $EXP_ROOT/setup.sh first." >&2
    exit 1
fi

exec "$PYTHON" -m lilly_film.cli "$@"

