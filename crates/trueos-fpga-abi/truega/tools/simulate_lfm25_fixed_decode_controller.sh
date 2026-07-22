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
  echo "Icarus Verilog is required; set TRUEGA_IVERILOG and TRUEGA_VVP" >&2
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
  -s truega_lfm25_fixed_decode_controller_tb \
  -o "$STAGE_DIR/fixed_decode_controller.vvp" \
  "$RTL_DIR/truega_lfm25_fixed_decode_controller.v" \
  "$RTL_DIR/truega_lfm25_fixed_decode_controller_tb.sv"
"$VVP" "${vvp_args[@]}" "$STAGE_DIR/fixed_decode_controller.vvp"

# A short FAST=0 smoke enters through the controller's frontend dword-read
# boundary, elaborates every fixed join, and numerically checks the real shared
# resident engine. The complete schedule above is accelerated only to keep CI
# bounded.
compute_sources=("$RTL_DIR"/*.v)
"$IVERILOG" "${iverilog_base_args[@]}" -g2012 \
  -s truega_lfm25_fixed_decode_controller_production_tb \
  -o "$STAGE_DIR/fixed_decode_controller_production.vvp" \
  "${compute_sources[@]}" \
  "$RTL_DIR/truega_lfm25_fixed_decode_controller_production_tb.sv"
"$VVP" "${vvp_args[@]}" \
  "$STAGE_DIR/fixed_decode_controller_production.vvp"
