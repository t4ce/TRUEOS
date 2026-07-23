#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$PROJECT_DIR/../../.." && pwd)"

MODEL="${TRUEGA_LFM25_GGUF:-$REPO_ROOT/tools/lfm2.5-350m/LFM2.5-350M-Q8_0.gguf}"
IMAGE="${TRUEGA_LFM25_IMAGE:-$REPO_ROOT/tools/lfm2.5-350m/LFM2.5-350M-Q8_0.truega.bin}"
TOKENIZER="${TRUEGA_LFM25_TOKENIZER:-$REPO_ROOT/tools/lfm2.5-350m/LFM2.5-350M-Q8_0.tokenizer.bin}"
HOST_TOOLCHAIN="${TRUEGA_HOST_TOOLCHAIN:-1.96}"
HOST_TARGET="${TRUEGA_HOST_TARGET:-x86_64-unknown-linux-gnu}"
GENERATOR_TARGET_DIR="${TRUEGA_LFM25_TARGET_DIR:-/tmp/truega-lfm25-seal-target}"
GENERATOR_MANIFEST="$SCRIPT_DIR/lfm25-seal/Cargo.toml"
TOKENIZER_MANIFEST="$REPO_ROOT/crates/trueos-lfm25-cpu/Cargo.toml"
CONTRACT="$PROJECT_DIR/artifacts/lfm25_model.contract.bin"
RUST_METADATA="$PROJECT_DIR/../src/lfm25_generated.rs"
RTL_METADATA="$PROJECT_DIR/src/generated/truega_lfm25_model.v"

if [[ ! -f "$MODEL" ]]; then
  echo "missing pinned model: $MODEL" >&2
  echo "restore it with tools/lfm2.5-350m/download.sh" >&2
  exit 1
fi

CARGO_TARGET_DIR="$GENERATOR_TARGET_DIR" cargo "+$HOST_TOOLCHAIN" test --quiet \
  --manifest-path "$GENERATOR_MANIFEST" \
  --target "$HOST_TARGET" \
  --config 'unstable.build-std=[]'

CARGO_TARGET_DIR="$GENERATOR_TARGET_DIR" cargo "+$HOST_TOOLCHAIN" run --quiet \
  --manifest-path "$GENERATOR_MANIFEST" \
  --target "$HOST_TARGET" \
  --config 'unstable.build-std=[]' \
  -- pack \
  --gguf "$MODEL" \
  --image-out "$IMAGE" \
  --contract-out "$CONTRACT" \
  --rust-out "$RUST_METADATA" \
  --rtl-out "$RTL_METADATA"

CARGO_TARGET_DIR="$GENERATOR_TARGET_DIR" cargo "+$HOST_TOOLCHAIN" run --quiet \
  --manifest-path "$TOKENIZER_MANIFEST" \
  --target "$HOST_TARGET" \
  --config 'unstable.build-std=[]' \
  --bin lfm25-tokenizer-seal \
  -- "$MODEL" "$TOKENIZER"

echo "native_image=$IMAGE"
echo "tokenizer=$TOKENIZER"
echo "model_contract=$CONTRACT"
echo "rust_metadata=$RUST_METADATA"
echo "rtl_metadata=$RTL_METADATA"
