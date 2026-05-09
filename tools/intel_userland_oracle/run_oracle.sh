#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOL_DIR="$ROOT/tools/intel_userland_oracle"
OUT_DIR="${TRUEOS_ORACLE_LOG_DIR:-$ROOT/.codex_tmp/intel_userland_oracle/latest}"
BUILD_DIR="$OUT_DIR/build"
LOG="$OUT_DIR/log.txt"
HW_LOG="$OUT_DIR/hw_mmio_log.txt"

mkdir -p "$BUILD_DIR" "$OUT_DIR/dumps"
: > "$LOG"
: > "$HW_LOG"
find "$OUT_DIR/dumps" -type f -delete

{
  echo "oracle-run: started_at=$(date --iso-8601=seconds)"
  echo "oracle-run: root=$ROOT"
  echo "oracle-run: out_dir=$OUT_DIR"
  echo "oracle-run: render_nodes=$(ls -l /dev/dri/by-path 2>/dev/null | tr '\n' ';')"
  echo "oracle-run: lspci=$(lspci -nn -s 00:02.0 2>/dev/null || true)"
} >> "$LOG"

glslangValidator -V -S comp \
  -o "$BUILD_DIR/sentinel.comp.spv" \
  "$TOOL_DIR/sentinel.comp" >> "$LOG" 2>&1

cc -O2 -g -Wall -Wextra -fPIC -shared \
  "$TOOL_DIR/ioctl_trace.c" \
  -o "$BUILD_DIR/libtrueos_ioctl_trace.so" \
  -ldl -rdynamic >> "$LOG" 2>&1

cc -O2 -g -Wall -Wextra -rdynamic \
  "$TOOL_DIR/vk_compute_sentinel.c" \
  -o "$BUILD_DIR/vk_compute_sentinel" \
  -lvulkan >> "$LOG" 2>&1

cc -O2 -g -Wall -Wextra \
  "$TOOL_DIR/hw_mmio_sampler.c" \
  -o "$BUILD_DIR/hw_mmio_sampler" >> "$LOG" 2>&1

{
  echo "oracle-run: build_ok=1"
  echo "oracle-run: env MESA_VK_DEVICE_SELECT=8086:a780"
  echo "oracle-run: env VK_LOADER_DRIVERS_SELECT=${VK_LOADER_DRIVERS_SELECT:-*intel*}"
  echo "oracle-run: env TRUEOS_ORACLE_VK_DEVICE_ID=${TRUEOS_ORACLE_VK_DEVICE_ID:-0xA780}"
  echo "oracle-run: env TRUEOS_ORACLE_MAX_DUMP_BYTES=${TRUEOS_ORACLE_MAX_DUMP_BYTES:-0}"
  echo "oracle-run: env TRUEOS_ORACLE_TRACE_STACKS=${TRUEOS_ORACLE_TRACE_STACKS:-1}"
  echo "oracle-run: env TRUEOS_ORACLE_STACK_DEPTH=${TRUEOS_ORACLE_STACK_DEPTH:-64}"
  echo "oracle-run: env TRUEOS_ORACLE_TRACE_SNAPSHOTS=${TRUEOS_ORACLE_TRACE_SNAPSHOTS:-1}"
  echo "oracle-run: env TRUEOS_ORACLE_REQUIRE_HW=${TRUEOS_ORACLE_REQUIRE_HW:-1}"
  echo "oracle-run: hw_mmio=attempt-sudo-noninteractive"
} >> "$LOG"

HW_PID=""
if sudo -n true >> "$LOG" 2>&1; then
  sudo -n "$BUILD_DIR/hw_mmio_sampler" \
    --out "$HW_LOG" \
    --interval-us "${TRUEOS_ORACLE_HW_INTERVAL_US:-200}" &
  HW_PID="$!"
  echo "oracle-run: hw_mmio_sampler_pid=$HW_PID log=$HW_LOG" >> "$LOG"
  sleep 0.05
else
  echo "oracle-run: hw_mmio_sampler_unavailable reason=sudo-noninteractive-failed log=$HW_LOG" >> "$LOG"
  echo "hw t_ns=0 unavailable reason=sudo-noninteractive-failed need=\"run sudo -v first or run this script as root\"" >> "$HW_LOG"
  if [[ "${TRUEOS_ORACLE_REQUIRE_HW:-1}" != "0" ]]; then
    echo "oracle-run: aborted reason=hw-mmio-required-but-unavailable" >> "$LOG"
    echo "$LOG"
    exit 3
  fi
fi

set +e
TRUEOS_ORACLE_LOG_DIR="$OUT_DIR" \
TRUEOS_ORACLE_VK_DEVICE_ID="${TRUEOS_ORACLE_VK_DEVICE_ID:-0xA780}" \
TRUEOS_ORACLE_MAX_DUMP_BYTES="${TRUEOS_ORACLE_MAX_DUMP_BYTES:-0}" \
TRUEOS_ORACLE_TRACE_STACKS="${TRUEOS_ORACLE_TRACE_STACKS:-1}" \
TRUEOS_ORACLE_STACK_DEPTH="${TRUEOS_ORACLE_STACK_DEPTH:-64}" \
TRUEOS_ORACLE_TRACE_SNAPSHOTS="${TRUEOS_ORACLE_TRACE_SNAPSHOTS:-1}" \
MESA_VK_DEVICE_SELECT="${MESA_VK_DEVICE_SELECT:-8086:a780}" \
VK_LOADER_DRIVERS_SELECT="${VK_LOADER_DRIVERS_SELECT:-*intel*}" \
LD_PRELOAD="$BUILD_DIR/libtrueos_ioctl_trace.so${LD_PRELOAD:+:$LD_PRELOAD}" \
"$BUILD_DIR/vk_compute_sentinel" "$BUILD_DIR/sentinel.comp.spv" >> "$LOG" 2>&1
APP_STATUS="$?"
set -e

if [[ -n "$HW_PID" ]]; then
  kill "$HW_PID" 2>/dev/null || true
  wait "$HW_PID" 2>/dev/null || true
fi

{
  echo "oracle-run: finished_at=$(date --iso-8601=seconds)"
  echo "oracle-run: app_status=$APP_STATUS"
  echo "oracle-run: dump_count=$(find "$OUT_DIR/dumps" -type f | wc -l)"
  echo "oracle-run: log=$LOG"
  echo "oracle-run: hw_log=$HW_LOG"
} >> "$LOG"

echo "$LOG"
exit "$APP_STATUS"
