#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TRUEOS_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
HELIO_REPO=${HELIO_REPO:-"$TRUEOS_ROOT/../Helio"}
PUBLISHED="$TRUEOS_ROOT/picasso/sprite-dig-atlas.trueos.rgba"
VALIDATOR="$SCRIPT_DIR/validate_sprite_dig_atlas.py"
MANIFEST="$TRUEOS_ROOT/tools/helio-sprite-atlas-bake/Cargo.toml"

if [ "${1:-}" = "--validate-only" ]; then
    if [ "$#" -gt 2 ]; then
        echo "usage: $0 [--validate-only [atlas.rgba]]" >&2
        exit 2
    fi
    exec python3 -B "$VALIDATOR" "${2:-$PUBLISHED}"
fi
if [ "$#" -ne 0 ]; then
    echo "usage: $0 [--validate-only [atlas.rgba]]" >&2
    exit 2
fi

for required in "$HELIO_REPO/assets/sprites/Assets/Tiles.png" "$MANIFEST" "$VALIDATOR"; do
    if [ ! -f "$required" ]; then
        echo "missing required input: $required" >&2
        exit 1
    fi
done

mkdir -p "$(dirname -- "$PUBLISHED")"
STAGED=$(mktemp "$(dirname -- "$PUBLISHED")/.sprite-dig-atlas.trueos.rgba.XXXXXX")
cleanup() {
    rm -f -- "$STAGED"
}
trap cleanup EXIT HUP INT TERM

(
    cd "${TMPDIR:-/tmp}"
    cargo run -q --locked --manifest-path "$MANIFEST" -- "$HELIO_REPO" "$STAGED"
)
python3 -B "$VALIDATOR" "$STAGED"
chmod 0644 "$STAGED"
mv -f -- "$STAGED" "$PUBLISHED"
echo "published $PUBLISHED"
