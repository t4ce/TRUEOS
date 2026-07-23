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

iverilog_args=(-g2012)
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_args=(-B "$IVERILOG_BASE" "${iverilog_args[@]}")
fi
vvp_args=()
if [[ -n "$VVP_MODULE_DIR" ]]; then
  vvp_args=(-M "$VVP_MODULE_DIR")
fi

run_tb() {
  local top=$1
  shift
  "$IVERILOG" "${iverilog_args[@]}" -s "$top" -o "$STAGE_DIR/$top.vvp" "$@"
  "$VVP" "${vvp_args[@]}" "$STAGE_DIR/$top.vvp"
}

run_tb truega_lfm25_rmsnorm_reduce_slot_tb \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_float_to_q30.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_residual_slot.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_reduce_slot.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_reduce_slot_tb.sv"

run_tb truega_lfm25_shortconv_triplet_row_slot_tb \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_gemv.v" \
  "$RTL_DIR/truega_lfm25_shortconv_triplet_row_slot.v" \
  "$RTL_DIR/truega_lfm25_shortconv_triplet_row_slot_tb.sv"

run_tb truega_lfm25_shortconv_channel_slot_tb \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_float_to_q30.v" \
  "$RTL_DIR/truega_lfm25_shortconv_channel_slot.v" \
  "$RTL_DIR/truega_lfm25_shortconv_channel_slot_tb.sv"

# The strict quantizer test generates adversarial and sealed-trace vectors with
# its Rust reference before invoking Icarus.
"$SCRIPT_DIR/simulate_q30_to_q8_0.sh"

run_tb truega_lfm25_rmsnorm_vector_slot_tb \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_float_to_q30.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_reduce_slot.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_residual_slot.v" \
  "$RTL_DIR/truega_q30_to_q8_0_block_slot.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_vector_slot.v" \
  "$RTL_DIR/truega_lfm25_rmsnorm_vector_slot_tb.sv"

run_tb truega_lfm25_residual_vector_slot_tb \
  "$RTL_DIR/truega_lfm25_residual_vector_slot.v" \
  "$RTL_DIR/truega_lfm25_residual_vector_slot_tb.sv"

run_tb truega_lfm25_shortconv_token_slot_tb \
  "$RTL_DIR/truega_q8_0_dot32.v" \
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v" \
  "$RTL_DIR/truega_q8_0_gemv.v" \
  "$RTL_DIR/truega_lfm25_shortconv_triplet_row_slot.v" \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_float_to_q30.v" \
  "$RTL_DIR/truega_lfm25_shortconv_channel_slot.v" \
  "$RTL_DIR/truega_q30_to_q8_0_block_slot.v" \
  "$RTL_DIR/truega_lfm25_shortconv_token_slot.v" \
  "$RTL_DIR/truega_lfm25_shortconv_token_slot_tb.sv"
