#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
LOG_DIR="${TRUEGA_LOG_DIR:-/tmp/truega-logs}"
mkdir -p "$LOG_DIR"
LOG="${TRUEGA_FLASH_LOG:-$LOG_DIR/flash_sram-$(date +%Y%m%d-%H%M%S).log}"
exec > >(tee -a "$LOG") 2>&1
echo "truega flash_sram log=$LOG"
echo "started_at=$(date --iso-8601=seconds)"
trap 'rc=$?; echo "finished_at=$(date --iso-8601=seconds) status=$rc"; exit $rc' EXIT

find_exe() {
  local name="$1"
  shift
  if command -v "$name" >/dev/null 2>&1; then
    command -v "$name"
    return 0
  fi
  local candidate
  for candidate in "$@"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

PROGRAMMER="$(find_exe programmer_cli "$HOME/Programmme/Gowin/Programmer/bin/programmer_cli" "$HOME/Programmes/Gowin/Programmer/bin/programmer_cli" "$HOME/Programs/Gowin/Programmer/bin/programmer_cli")" || {
  echo "missing programmer_cli; put Gowin Programmer bin on PATH or install under ~/Programmme/Gowin" >&2
  exit 1
}
FS_FILE="$PROJECT_DIR/impl/pnr/min_pci_led.fs"
DEVICE="GW5AST-138B"
CABLE_INDEX="${TRUEGA_CABLE_INDEX:-4}"
CABLE_CHANNEL="${TRUEGA_CABLE_CHANNEL:-0}"
FREQ="2.5MHz"

if [[ ! -f "$FS_FILE" ]]; then
  echo "missing bitstream: $FS_FILE" >&2
  echo "build it first: $PROJECT_DIR/tools/build_fs.sh" >&2
  exit 1
fi

FS_FILE="$(readlink -f "$FS_FILE")"

echo "bitstream: $FS_FILE"

echo "scan cables:"
"$PROGRAMMER" --scan-cables L || true
"$PROGRAMMER" --scan-cables F || true

echo "program SRAM:"
"$PROGRAMMER" \
  --device "$DEVICE" \
  --run 2 \
  --fsFile "$FS_FILE" \
  --cable-index "$CABLE_INDEX" \
  --channel "$CABLE_CHANNEL" \
  --frequency "$FREQ"
