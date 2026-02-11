#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  tools/symbolize.sh <addr> [addr...]

Notes:
  - Uses TRUEOS_ELF if set; otherwise picks newest bld/artifacts/*/TRUEOS.full.elf.
  - Address format can be 0x... or plain hex.
EOF
}

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

if ! command -v addr2line >/dev/null 2>&1; then
  echo "error: addr2line not found in PATH" >&2
  exit 1
fi

ELF="${TRUEOS_ELF:-}"
if [[ -z "${ELF}" ]]; then
  ELF="$(ls -1t bld/artifacts/*/TRUEOS.full.elf 2>/dev/null | head -n1 || true)"
fi

if [[ -z "${ELF}" || ! -f "${ELF}" ]]; then
  echo "error: TRUEOS.full.elf not found (set TRUEOS_ELF or run 'make artifacts')" >&2
  exit 1
fi

echo "symbolize: elf=${ELF}"
for raw in "$@"; do
  addr="${raw}"
  if [[ "${addr}" != 0x* ]]; then
    addr="0x${addr}"
  fi
  echo "----- ${addr} -----"
  addr2line -e "${ELF}" -f -C -p "${addr}" || true
done
