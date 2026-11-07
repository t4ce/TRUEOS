#!/usr/bin/env bash
# Build, publish native boot files and a live-update archive, then reset the rig once.
set -euo pipefail

if [[ ${1:-} == --help || ${2:-} == --help ]]; then
    echo "cargo rig: dev kernel + native loader + tar/LZ4 update + publish PXE files + one physical reset"
    echo "cargo rigreal: release kernel + native loader + tar/LZ4 update + publish PXE files + one physical reset"
    echo "Add --native-only for the audited core/alloc builtins in a fresh isolated build."
    exit 0
fi
case "${1:-}" in
    rig) shift; set -- --dev "$@" ;;
    rigreal) shift; set -- --release "$@" ;;
esac
if [[ $# -lt 1 || $# -gt 2 || ( ${1:-} != --dev && ${1:-} != --release ) || ( $# == 2 && ${2:-} != --native-only ) ]]; then
    echo "usage: cargo rig [--native-only] | cargo rigreal [--native-only]" >&2
    exit 2
fi
root=$(cd -- "$(dirname -- "$(readlink -f -- "$0")")/.." && pwd)
cd "$root"
for tool in python3 flock; do
    command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
mkdir -p tgt
exec 9>tgt/rig-build.lock
flock -n 9 || { echo "Another rig build is already running." >&2; exit 1; }

bash tools/trueos-boot/prepare.sh "$@"

# PXE runs separately and automatically notices the published files.
bash tools/rig-reset.sh
