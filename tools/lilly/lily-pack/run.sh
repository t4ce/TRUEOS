#!/bin/sh
set -eu
# Cargo reads configuration from the invocation directory, even for an excluded
# standalone workspace. Build outside TRUEOS, then run from the caller's original
# directory so relative asset/output arguments retain their usual meaning.
lily_pack_tool=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export CARGO_TARGET_DIR="${LILY_PACK_TARGET_DIR:-$lily_pack_tool/target}"
(
    cd /tmp
    cargo +nightly-2026-07-10 build --release \
        --manifest-path "$lily_pack_tool/Cargo.toml" \
        --target x86_64-unknown-linux-gnu
)
exec "$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release/lily-pack" "$@"
