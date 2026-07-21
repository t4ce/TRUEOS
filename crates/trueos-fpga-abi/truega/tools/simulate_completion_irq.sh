#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/build/sim"
mkdir -p "${OUT_DIR}"

iverilog -g2012 \
  -o "${OUT_DIR}/truega_completion_irq_tb" \
  "${ROOT_DIR}/src/truega_completion_irq.v" \
  "${ROOT_DIR}/sim/truega_completion_irq_tb.v"
vvp "${OUT_DIR}/truega_completion_irq_tb"
