#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RTL_DIR="$PROJECT_DIR/src/compute"
VECTORS="${TRUEGA_Q8_VECTORS:-$PROJECT_DIR/artifacts/lfm25_layer0_ffn.golden.bin.vectors}"
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
if [[ ! -s "$VECTORS" ]]; then
  echo "missing vectors: $VECTORS (run capture_lfm25_ffn_golden.sh)" >&2
  exit 1
fi

iverilog_args=(-g2012 -s truega_q8_0_gemv_tb -o "$STAGE_DIR/q8_0_gemv.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_gemv.v" \
  "$RTL_DIR/truega_q8_0_gemv_tb.sv"

vvp_args=()
if [[ -n "$VVP_MODULE_DIR" ]]; then
  vvp_args=(-M "$VVP_MODULE_DIR")
fi
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/q8_0_gemv.vvp" "+VECTORS=$VECTORS"

iverilog_args=(-g2012 -s truega_lfm25_gate_row_slot_tb -o "$STAGE_DIR/lfm25_gate_row_slot.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_gemv.v" \
  "$RTL_DIR/truega_lfm25_gate_row_slot.v" \
  "$RTL_DIR/truega_lfm25_gate_row_slot_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/lfm25_gate_row_slot.vvp" "+VECTORS=$VECTORS"

iverilog_args=(-g2012 -s truega_q8_0_row_block_slot_tb -o "$STAGE_DIR/q8_0_row_block_slot.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_block_slot.v" \
  "$RTL_DIR/truega_q8_0_row_block_slot.v" \
  "$RTL_DIR/truega_q8_0_row_block_slot_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/q8_0_row_block_slot.vvp" "+VECTORS=$VECTORS"

iverilog_args=(-g2012 -s truega_q8_0_cached_pair_slot_tb -o "$STAGE_DIR/q8_0_cached_pair_slot.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_block_slot.v" \
  "$RTL_DIR/truega_q8_0_row_block_slot.v" \
  "$RTL_DIR/truega_q8_0_cached_pair_slot.v" \
  "$RTL_DIR/truega_q8_0_cached_pair_slot_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/q8_0_cached_pair_slot.vvp" "+VECTORS=$VECTORS"

iverilog_args=(-g2012 -s truega_q8_0_scale_q30_seq_tb -o "$STAGE_DIR/q8_0_scale_q30_seq.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/q8_0_scale_q30_seq.vvp"

iverilog_args=(-g2012 -s truega_q8_0_block_slot_tb -o "$STAGE_DIR/q8_0_block_slot.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_block_slot.v" \
  "$RTL_DIR/truega_q8_0_block_slot_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/q8_0_block_slot.vvp" "+VECTORS=$VECTORS"

iverilog_args=(-g2012 -s truega_lfm25_silu_q30_slot_tb -o "$STAGE_DIR/lfm25_silu_q30_slot.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_lfm25_silu_q30_slot.v" \
  "$RTL_DIR/truega_lfm25_silu_q30_slot_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/lfm25_silu_q30_slot.vvp"

iverilog_args=(-g2012 -s truega_functions_tb -o "$STAGE_DIR/functions.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$PROJECT_DIR/src/generated/truega_functions.v" \
  "$RTL_DIR/truega_functions_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/functions.vvp"
