#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$PROJECT_DIR/../../.." && pwd)"
MODEL_DIR="$REPO_ROOT/tools/lfm2.5-350m"
LLAMA_SOURCE="${TRUEGA_LLAMA_SOURCE:-$MODEL_DIR/runtime/llama-b10075-src}"
LLAMA_COMMIT="76f46ad29d61fd8c1401e8221842934bf62a6064"
TRACE_TOOL="$SCRIPT_DIR/lfm25-golden"
TRACE_PATCH="$TRACE_TOOL/llama-b10075-ffn-trace.patch"
TRACE_BUILD="${TRUEGA_LFM25_TRACE_BUILD:-/tmp/truega-lfm25-trace-build}"
RUST_TARGET="${TRUEGA_LFM25_GOLDEN_TARGET:-/tmp/truega-lfm25-golden-host-target}"
HOST_TOOLCHAIN="${TRUEGA_HOST_TOOLCHAIN:-1.96}"
GGUF="${TRUEGA_LFM25_GGUF:-$MODEL_DIR/LFM2.5-350M-Q8_0.gguf}"
NATIVE_IMAGE="${TRUEGA_LFM25_IMAGE:-$MODEL_DIR/LFM2.5-350M-Q8_0.truega.bin}"
GOLDEN="${TRUEGA_LFM25_GOLDEN:-$PROJECT_DIR/artifacts/lfm25_layer0_ffn.golden.bin}"
BLOCK_GOLDEN="${TRUEGA_LFM25_BLOCK_GOLDEN:-$PROJECT_DIR/artifacts/lfm25_q8_block.golden.bin}"
STAGE_DIR="$(mktemp -d)"

finish() {
  local rc=$?
  rm -rf -- "$STAGE_DIR"
  exit "$rc"
}
trap finish EXIT

if [[ ! -d "$LLAMA_SOURCE/.git" ]]; then
  mkdir -p "$(dirname "$LLAMA_SOURCE")"
  git clone --filter=blob:none --no-checkout https://github.com/ggml-org/llama.cpp.git "$LLAMA_SOURCE"
  git -C "$LLAMA_SOURCE" checkout --detach "$LLAMA_COMMIT"
fi

actual_commit="$(git -C "$LLAMA_SOURCE" rev-parse HEAD)"
if [[ "$actual_commit" != "$LLAMA_COMMIT" ]]; then
  echo "llama.cpp checkout is $actual_commit, expected $LLAMA_COMMIT" >&2
  exit 1
fi

if git -C "$LLAMA_SOURCE" apply --reverse --check "$TRACE_PATCH" >/dev/null 2>&1; then
  : # Exact trace-only patch is already present.
elif git -C "$LLAMA_SOURCE" diff --quiet && git -C "$LLAMA_SOURCE" apply --check "$TRACE_PATCH"; then
  git -C "$LLAMA_SOURCE" apply "$TRACE_PATCH"
else
  echo "llama.cpp checkout has changes other than the exact tracked trace patch" >&2
  git -C "$LLAMA_SOURCE" status --short >&2
  exit 1
fi

cmake -S "$TRACE_TOOL" -B "$TRACE_BUILD" \
  -DLLAMA_SOURCE_DIR="$LLAMA_SOURCE" \
  -DCMAKE_BUILD_TYPE=Release
cmake --build "$TRACE_BUILD" --target truega-lfm25-trace -j"$(nproc)"

RAW_TRACE="$STAGE_DIR/layer0.raw"
"$TRACE_BUILD/truega-lfm25-trace" "$GGUF" "$RAW_TRACE"

(
  cd /tmp
  export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Aunsafe-op-in-unsafe-fn"
  CARGO_TARGET_DIR="$RUST_TARGET" cargo "+$HOST_TOOLCHAIN" test --quiet \
    --manifest-path "$TRACE_TOOL/Cargo.toml"
  CARGO_TARGET_DIR="$RUST_TARGET" cargo "+$HOST_TOOLCHAIN" run --quiet --release \
    --manifest-path "$TRACE_TOOL/Cargo.toml" -- \
    seal "$RAW_TRACE" "$GGUF" "$NATIVE_IMAGE" "$GOLDEN"
  CARGO_TARGET_DIR="$RUST_TARGET" cargo "+$HOST_TOOLCHAIN" run --quiet --release \
    --manifest-path "$TRACE_TOOL/Cargo.toml" -- verify "$GOLDEN"
  CARGO_TARGET_DIR="$RUST_TARGET" cargo "+$HOST_TOOLCHAIN" run --quiet --release \
    --manifest-path "$TRACE_TOOL/Cargo.toml" -- \
    block "$GOLDEN" "$GOLDEN.vectors" "$NATIVE_IMAGE" "$BLOCK_GOLDEN"
  CARGO_TARGET_DIR="$RUST_TARGET" cargo "+$HOST_TOOLCHAIN" run --quiet --release \
    --manifest-path "$TRACE_TOOL/Cargo.toml" -- \
    verify-block "$BLOCK_GOLDEN" "$GOLDEN" "$GOLDEN.vectors"
)

(
  cd "$(dirname "$GOLDEN")"
  sha256sum "$(basename "$GOLDEN")" > "$(basename "$GOLDEN").sha256"
)
echo "capture complete token=1 layer=0 llama_commit=$LLAMA_COMMIT"
echo "block_golden=$BLOCK_GOLDEN"
echo "heartbeat project and bitstream were not build inputs"
