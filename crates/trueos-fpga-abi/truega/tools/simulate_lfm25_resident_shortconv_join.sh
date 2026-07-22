#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RTL_DIR="$PROJECT_DIR/src/compute"
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

iverilog_args=(-g2012 -s truega_lfm25_resident_shortconv_join_tb
  -o "$STAGE_DIR/resident_shortconv_join.vvp")
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
"$IVERILOG" "${iverilog_args[@]}" \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30.v" \
  "$RTL_DIR/truega_q8_0_gemv.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_dequant_block_slot.v" \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_float_to_q30.v" \
  "$RTL_DIR/truega_q30_to_q8_0_block_slot.v" \
  "$RTL_DIR/truega_lfm25_shortconv_triplet_row_slot.v" \
  "$RTL_DIR/truega_lfm25_shortconv_channel_slot.v" \
  "$RTL_DIR/truega_lfm25_shortconv_token_slot.v" \
  "$RTL_DIR/truega_lfm25_q8_projection_row_engine.v" \
  "$RTL_DIR/truega_lfm25_resident_tensor_store.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_reduce_slot.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_residual_slot.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_vector_slot.v" \
  "$RTL_DIR/truega_lfm25_residual_vector_slot.v" \
  "$RTL_DIR/truega_lfm25_resident_vector_engine.v" \
  "$RTL_DIR/truega_lfm25_resident_shortconv_join.v" \
  "$RTL_DIR/truega_lfm25_resident_shortconv_join_tb.sv"

vvp_args=()
if [[ -n "$VVP_MODULE_DIR" ]]; then
  vvp_args=(-M "$VVP_MODULE_DIR")
fi
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/resident_shortconv_join.vvp"
