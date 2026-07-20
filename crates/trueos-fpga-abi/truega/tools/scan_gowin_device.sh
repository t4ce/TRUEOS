#!/usr/bin/env bash
set -euo pipefail

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
DEVICE="GW5AST-138B"
CABLE_INDEX="5"

echo "scan cables:"
"$PROGRAMMER" --scan-cables L || true
"$PROGRAMMER" --scan-cables F || true

echo "scan device:"
"$PROGRAMMER" \
  --device "$DEVICE"
  --cable-index "$CABLE_INDEX"
  --scan
