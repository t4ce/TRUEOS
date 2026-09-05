#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TRUEOS_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
HELIO_REPO=${HELIO_REPO:-"$TRUEOS_ROOT/../helio"}
BAKER="$TRUEOS_ROOT/tools/helio-gbuffer-shader-bake/bake.py"
PUBLISHED="$TRUEOS_ROOT/picasso/helio-gbuffer"

if [ "${1:-}" = "--validate-only" ]; then
    if [ "$#" -ne 1 ]; then
        echo "usage: $0 [--validate-only]" >&2
        exit 2
    fi
    exec python3 "$BAKER" --out "$PUBLISHED" --validate-only
fi
if [ "$#" -ne 0 ]; then
    echo "usage: $0 [--validate-only]" >&2
    exit 2
fi

set -- "$BAKER" --out "$PUBLISHED"
if [ -n "${INTEL_DEVICE_ID:-}" ]; then
    set -- "$@" --device-id "$INTEL_DEVICE_ID"
fi
exec env HELIO_REPO="$HELIO_REPO" python3 "$@"
