#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RTL_DIR="$PROJECT_DIR/src/compute"
TRACE="${TRUEGA_LFM25_DECODE_TRACE:-$PROJECT_DIR/artifacts/lfm25_token1_decode.golden.bin}"
GENERATOR="$SCRIPT_DIR/q30-q8-vectors/Cargo.toml"
HOST_TOOLCHAIN="${TRUEGA_HOST_TOOLCHAIN:-1.96}"
HOST_TARGET="${TRUEGA_Q30_Q8_TARGET:-/tmp/truega-q30-q8-host-target}"
IVERILOG="${TRUEGA_IVERILOG:-$(command -v iverilog || true)}"
VVP="${TRUEGA_VVP:-$(command -v vvp || true)}"
IVERILOG_BASE="${TRUEGA_IVERILOG_BASE:-}"
VVP_MODULE_DIR="${TRUEGA_VVP_MODULE_DIR:-}"
STAGE_DIR="$(mktemp -d)"

finish() {
  local rc=$?
  rm -rf -- "$STAGE_DIR"
  exit "$rc"
}
trap finish EXIT

if [[ -z "$IVERILOG" || -z "$VVP" ]]; then
  echo "Icarus Verilog is required; set TRUEGA_IVERILOG and TRUEGA_VVP if it is not on PATH" >&2
  exit 1
fi
if [[ ! -s "$TRACE" ]]; then
  echo "missing sealed token trace: $TRACE" >&2
  exit 1
fi

(
  cd /tmp
  CARGO_TARGET_DIR="$HOST_TARGET" cargo "+$HOST_TOOLCHAIN" run --quiet \
    --manifest-path "$GENERATOR" -- "$TRACE" "$STAGE_DIR/vectors.txt"
)

iverilog_args=(-g2012 -s truega_q30_to_q8_0_block_slot_tb -o "$STAGE_DIR/q30_to_q8.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q30_to_q8_0_block_slot.v" \
  "$RTL_DIR/truega_q30_to_q8_0_block_slot_tb.sv"

vvp_args=()
if [[ -n "$VVP_MODULE_DIR" ]]; then
  vvp_args=(-M "$VVP_MODULE_DIR")
fi
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/q30_to_q8.vvp" "+VECTORS=$STAGE_DIR/vectors.txt"
