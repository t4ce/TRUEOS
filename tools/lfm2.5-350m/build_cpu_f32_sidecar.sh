#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

GGUF="${LFM25_GGUF:-$SCRIPT_DIR/LFM2.5-350M-Q8_0.gguf}"
OUTPUT="${LFM25_F32_SIDECAR:-$SCRIPT_DIR/LFM2.5-350M-Q8_0.cpu-f32.bin}"
HOST_TOOLCHAIN="${LFM25_HOST_TOOLCHAIN:-1.96}"
HOST_TARGET="${LFM25_HOST_TARGET:-x86_64-unknown-linux-gnu}"
TARGET_DIR="${LFM25_F32_TARGET_DIR:-/tmp/trueos-lfm25-f32-target}"
MANIFEST="$REPO_ROOT/crates/trueos-lfm25-cpu/Cargo.toml"

if [[ ! -f "$GGUF" ]]; then
  echo "missing pinned model: $GGUF" >&2
  exit 1
fi

CARGO_TARGET_DIR="$TARGET_DIR" cargo "+$HOST_TOOLCHAIN" run --quiet \
  --manifest-path "$MANIFEST" \
  --target "$HOST_TARGET" \
  --config 'unstable.build-std=[]' \
  --bin lfm25-f32-seal \
  -- "$GGUF" "$OUTPUT"

sha256sum "$OUTPUT"
