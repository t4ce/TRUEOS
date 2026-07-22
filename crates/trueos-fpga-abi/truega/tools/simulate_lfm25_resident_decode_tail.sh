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

iverilog_base_args=()
if [[ -n "$IVERILOG_BASE" ]]; then
  iverilog_base_args=(-B "$IVERILOG_BASE")
fi
vvp_args=()
if [[ -n "$VVP_MODULE_DIR" ]]; then
  vvp_args=(-M "$VVP_MODULE_DIR")
fi
define_args=()
if [[ "${TRUEGA_TAIL_QUICK:-0}" == "1" ]]; then
  define_args=(-DTRUEGA_TAIL_QUICK)
fi

sources=(
  "$RTL_DIR/truega_q30_mul_seq.v"
  "$RTL_DIR/truega_float_to_q30.v"
  "$RTL_DIR/truega_q8_0_dot32.v"
  "$RTL_DIR/truega_q8_0_scale_q30.v"
  "$RTL_DIR/truega_q8_0_scale_q30_seq.v"
  "$RTL_DIR/truega_q8_0_gemv.v"
  "$RTL_DIR/truega_q8_0_dequant_block_slot.v"
  "$RTL_DIR/truega_q30_to_q8_0_block_slot.v"
  "$RTL_DIR/truega_lfm25_rmsnorm_reduce_slot.v"
  "$RTL_DIR/truega_lfm25_rmsnorm_residual_slot.v"
  "$RTL_DIR/truega_lfm25_rmsnorm_vector_slot.v"
  "$RTL_DIR/truega_lfm25_residual_vector_slot.v"
  "$RTL_DIR/truega_lfm25_resident_tensor_store.v"
  "$RTL_DIR/truega_lfm25_resident_vector_engine.v"
  "$RTL_DIR/truega_lfm25_tied_lm_head_argmax_slot.v"
  "$RTL_DIR/truega_lfm25_resident_decode_tail.v"
  "$RTL_DIR/truega_lfm25_resident_decode_tail_tb.sv"
)

"$IVERILOG" "${iverilog_base_args[@]}" "${define_args[@]}" -g2012 \
  -s truega_lfm25_resident_decode_tail_tb \
  -o "$STAGE_DIR/resident_decode_tail.vvp" "${sources[@]}"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/resident_decode_tail.vvp"
