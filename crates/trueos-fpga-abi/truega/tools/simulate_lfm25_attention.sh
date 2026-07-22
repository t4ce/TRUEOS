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

"$IVERILOG" "${iverilog_base_args[@]}" -g2012 \
  -s truega_lfm25_attention_norm_rope_tb \
  -o "$STAGE_DIR/norm_rope.vvp" \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_lfm25_head_rms_inverse_slot.v" \
  "$RTL_DIR/truega_lfm25_qk_norm_rope_slot.v" \
  "$RTL_DIR/truega_lfm25_attention_norm_rope_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/norm_rope.vvp"

"$IVERILOG" "${iverilog_base_args[@]}" -g2012 \
  -s truega_lfm25_attention_cache_softmax_tb \
  -o "$STAGE_DIR/cache_softmax.vvp" \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_lfm25_kv_address_slot.v" \
  "$RTL_DIR/truega_lfm25_kv_cache_slot.v" \
  "$RTL_DIR/truega_lfm25_gqa_dot_slot.v" \
  "$RTL_DIR/truega_lfm25_online_softmax_value_slot.v" \
  "$RTL_DIR/truega_lfm25_attention_cache_softmax_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/cache_softmax.vvp"

"$IVERILOG" "${iverilog_base_args[@]}" -g2012 \
  -s truega_lfm25_attention_token_slot_tb \
  -o "$STAGE_DIR/token_slot.vvp" \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_lfm25_head_rms_inverse_slot.v" \
  "$RTL_DIR/truega_lfm25_qk_norm_rope_slot.v" \
  "$RTL_DIR/truega_lfm25_gqa_dot_slot.v" \
  "$RTL_DIR/truega_lfm25_online_softmax_value_slot.v" \
  "$RTL_DIR/truega_lfm25_attention_token_slot.v" \
  "$RTL_DIR/truega_lfm25_attention_token_slot_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/token_slot.vvp"

"$IVERILOG" "${iverilog_base_args[@]}" -g2012 \
  -s truega_lfm25_attention_first_token_slot_tb \
  -o "$STAGE_DIR/first_token_slot.vvp" \
  "$RTL_DIR/truega_float_to_q30.v" \
  "$RTL_DIR/truega_q30_mul_seq.v" \
  "$RTL_DIR/truega_lfm25_head_rms_inverse_slot.v" \
  "$RTL_DIR/truega_lfm25_qk_norm_rope_slot.v" \
  "$RTL_DIR/truega_lfm25_gqa_dot_slot.v" \
  "$RTL_DIR/truega_lfm25_online_softmax_value_slot.v" \
  "$RTL_DIR/truega_lfm25_attention_token_slot.v" \
  "$RTL_DIR/truega_lfm25_attention_first_token_slot.v" \
  "$RTL_DIR/truega_lfm25_attention_first_token_slot_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/first_token_slot.vvp"
