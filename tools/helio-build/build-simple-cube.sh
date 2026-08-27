#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TRUEOS_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
HELIO_REPO=${HELIO_REPO:-"$TRUEOS_ROOT/../Helio"}
ASSET_DIR="$TRUEOS_ROOT/picasso"
PUBLISHED="$ASSET_DIR/simple-cube.trueos.intel.helio"
VALIDATOR="$SCRIPT_DIR/validate_artifact.py"

if [ "${1:-}" = "--validate-only" ]; then
    if [ "$#" -gt 2 ]; then
        echo "usage: $0 [--validate-only [artifact.helio]]" >&2
        exit 2
    fi
    exec python3 "$VALIDATOR" "${2:-$PUBLISHED}"
fi
if [ "$#" -ne 0 ]; then
    echo "usage: $0 [--validate-only [artifact.helio]]" >&2
    exit 2
fi

for required in \
    "$HELIO_REPO/Cargo.toml" \
    "$TRUEOS_ROOT/tools/helio-intel-bake/bake.py" \
    "$VALIDATOR"
do
    if [ ! -f "$required" ]; then
        echo "missing required input: $required" >&2
        exit 1
    fi
done

mkdir -p "$ASSET_DIR"
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/trueos-helio-build.XXXXXX")
STAGED=$(mktemp "$ASSET_DIR/.simple-cube.trueos.intel.helio.XXXXXX")
cleanup() {
    rm -rf -- "$WORK_DIR"
    rm -f -- "$STAGED"
}
trap cleanup EXIT HUP INT TERM

FRONTEND="$WORK_DIR/simple-cube.trueos.helio"
echo "==> Capturing and lowering Helio's real SimpleCube graph"
(
    # Keep TRUEOS's custom-target .cargo/config.toml out of this host tool.
    cd "$HELIO_REPO"
    cargo run -q \
        --manifest-path "$HELIO_REPO/Cargo.toml" \
        -p examples --bin bake_simple_cube -- "$FRONTEND"
)

echo "==> Compiling the captured shaders to the TRUEOS Intel package"
set -- "$FRONTEND" --out "$STAGED" --work-dir "$WORK_DIR/intel"
if [ -n "${INTEL_DEVICE_ID:-}" ]; then
    set -- "$@" --device-id "$INTEL_DEVICE_ID"
fi
python3 "$TRUEOS_ROOT/tools/helio-intel-bake/bake.py" "$@"

echo "==> Validating HELIOA, HELIOIR, and native-stage hashes"
python3 "$VALIDATOR" "$STAGED"

# STAGED is created in ASSET_DIR so rename(2) is the final atomic publication.
chmod 0644 "$STAGED"
mv -f -- "$STAGED" "$PUBLISHED"
echo "published $PUBLISHED"
