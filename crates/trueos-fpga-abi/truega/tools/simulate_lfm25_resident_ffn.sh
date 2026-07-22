#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RTL_DIR="$PROJECT_DIR/src/compute"
LOCAL_TOOL_ROOT="$(cd "$PROJECT_DIR/../../.." && pwd)/bld/tools/iverilog/usr"
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

if [[ -z "$IVERILOG" && -x "$LOCAL_TOOL_ROOT/bin/iverilog" ]]; then
  IVERILOG="$LOCAL_TOOL_ROOT/bin/iverilog"
  IVERILOG_BASE="$LOCAL_TOOL_ROOT/lib/x86_64-linux-gnu/ivl"
fi
if [[ -z "$VVP" && -x "$LOCAL_TOOL_ROOT/bin/vvp" ]]; then
  VVP="$LOCAL_TOOL_ROOT/bin/vvp"
  VVP_MODULE_DIR="$LOCAL_TOOL_ROOT/lib/x86_64-linux-gnu/ivl"
fi
if [[ -z "$IVERILOG" || -z "$VVP" ]]; then
  echo "Icarus Verilog is required; set TRUEGA_IVERILOG and TRUEGA_VVP" >&2
  exit 1
fi

iverilog_args=(-g2012 -s truega_lfm25_resident_ffn_row_engine_tb
  -o "$STAGE_DIR/resident_ffn.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi

"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30.v" \
  "$RTL_DIR/truega_q8_0_gemv.v" \
  "$RTL_DIR/truega_lfm25_gate_row_slot.v" \
  "$RTL_DIR/truega_lfm25_silu_q30_slot.v" \
  "$RTL_DIR/truega_q30_to_q8_0_block_slot.v" \
  "$RTL_DIR/truega_lfm25_resident_ffn_row_engine.v" \
  "$RTL_DIR/truega_lfm25_resident_ffn_row_engine_tb.sv"

vvp_args=()
if [[ -n "$VVP_MODULE_DIR" ]]; then
  vvp_args=(-M "$VVP_MODULE_DIR")
fi
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/resident_ffn.vvp"
