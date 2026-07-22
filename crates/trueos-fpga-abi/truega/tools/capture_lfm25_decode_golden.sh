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
GGUF="${TRUEGA_LFM25_GGUF:-$MODEL_DIR/LFM2.5-350M-Q8_0.gguf}"
OUTPUT="${TRUEGA_LFM25_DECODE_GOLDEN:-$PROJECT_DIR/artifacts/lfm25_token1_decode.golden.bin}"

actual_commit="$(git -C "$LLAMA_SOURCE" rev-parse HEAD)"
if [[ "$actual_commit" != "$LLAMA_COMMIT" ]]; then
  echo "llama.cpp checkout is $actual_commit, expected $LLAMA_COMMIT" >&2
  exit 1
fi

if ! git -C "$LLAMA_SOURCE" apply --reverse --check "$TRACE_PATCH" >/dev/null 2>&1; then
  echo "pinned checkout does not contain the exact tracked trace-only patch" >&2
  git -C "$LLAMA_SOURCE" status --short >&2
  exit 1
fi

cmake -S "$TRACE_TOOL" -B "$TRACE_BUILD" \
  -DLLAMA_SOURCE_DIR="$LLAMA_SOURCE" \
  -DCMAKE_BUILD_TYPE=Release
cmake --build "$TRACE_BUILD" --target truega-lfm25-decode-trace -j"$(nproc)"
"$TRACE_BUILD/truega-lfm25-decode-trace" "$GGUF" "$OUTPUT"

(
  cd "$(dirname "$OUTPUT")"
  sha256sum "$(basename "$OUTPUT")" > "$(basename "$OUTPUT").sha256"
  sha256sum --check "$(basename "$OUTPUT").sha256"
)

echo "decode capture complete token=1 llama_commit=$LLAMA_COMMIT artifact=$OUTPUT"
echo "host-only reference generation; FPGA and flash were not build inputs"
