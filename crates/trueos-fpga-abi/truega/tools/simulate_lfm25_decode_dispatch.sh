#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
IVERILOG=${TRUEGA_IVERILOG:-iverilog}
VVP=${TRUEGA_VVP:-vvp}
IVERILOG_BASE=${TRUEGA_IVERILOG_BASE:-}
VVP_MODULE_DIR=${TRUEGA_VVP_MODULE_DIR:-}
OUT=${TMPDIR:-/tmp}/truega_lfm25_decode_dispatch.vvp

if [ -n "$IVERILOG_BASE" ]; then
    set -- -B "$IVERILOG_BASE"
else
    set --
fi
"$IVERILOG" "$@" -g2012 -s truega_lfm25_decode_dispatch_tb -o "$OUT" \
    "$ROOT/src/compute/truega_lfm25_decode_dispatch.v" \
    "$ROOT/src/compute/truega_lfm25_decode_dispatch_tb.sv"
if [ -n "$VVP_MODULE_DIR" ]; then
    "$VVP" -M "$VVP_MODULE_DIR" "$OUT"
else
    "$VVP" "$OUT"
fi
