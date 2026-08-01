#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TRUEOS_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
HELIO_REPO="$TRUEOS_ROOT/../Helio"
CAPTURE_MANIFEST="$TRUEOS_ROOT/tools/helio-churn-forward-capture/Cargo.toml"
ASSET_DIR="$TRUEOS_ROOT/assets/helio"
PUBLISHED="$ASSET_DIR/churn-forward.trueos.intel.helio"
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
    "$CAPTURE_MANIFEST" \
    "$TRUEOS_ROOT/tools/helio-intel-bake/bake.py" \
    "$VALIDATOR"
do
    if [ ! -f "$required" ]; then
        echo "missing required input: $required" >&2
        exit 1
    fi
done

mkdir -p "$ASSET_DIR"
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/trueos-helio-churn-build.XXXXXX")
STAGED=$(mktemp "$ASSET_DIR/.churn-forward.trueos.intel.helio.XXXXXX")
cleanup() {
    rm -rf -- "$WORK_DIR"
    rm -f -- "$STAGED"
}
trap cleanup EXIT HUP INT TERM

FRONTEND="$WORK_DIR/churn-forward.trueos.helio"
echo "==> Capturing Helio's instanced Churn forward path through vendored wgpu"
(
    # Keep TRUEOS's kernel-only Cargo config out of this native host tool.
    cd "$HELIO_REPO"
    cargo run -q --release \
        --target x86_64-unknown-linux-gnu \
        --manifest-path "$CAPTURE_MANIFEST" -- "$FRONTEND"
)

echo "==> Compiling the captured Churn shaders to the TRUEOS Intel package"
set -- "$FRONTEND" --out "$STAGED" --work-dir "$WORK_DIR/intel"
if [ -n "${INTEL_DEVICE_ID:-}" ]; then
    set -- "$@" --device-id "$INTEL_DEVICE_ID"
fi
python3 "$TRUEOS_ROOT/tools/helio-intel-bake/bake.py" "$@"

echo "==> Validating Helio ABIs, Intel state, and native-stage hashes"
python3 "$VALIDATOR" "$STAGED"

# STAGED lives beside PUBLISHED, so the last rename is atomic and a failed
# capture or native compile leaves the previously working program untouched.
chmod 0644 "$STAGED"
mv -f -- "$STAGED" "$PUBLISHED"
echo "published $PUBLISHED"
