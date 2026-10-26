#!/usr/bin/env bash
# Build, stage, restart PXE, and reset the physical rig exactly once.
set -euo pipefail

if [[ ${1:-} == --help || ${2:-} == --help ]]; then
    echo "cargo rigdev: dev kernel + native loader + PXE restart + one physical reset"
    echo "cargo rigreal: release kernel + native loader + PXE restart + one physical reset"
    exit 0
fi
case "${1:-}" in
    rigdev) shift; set -- --dev "$@" ;;
    rigreal) shift; set -- --release "$@" ;;
esac
if [[ $# != 1 || ( $1 != --dev && $1 != --release ) ]]; then
    echo "usage: cargo rigdev | cargo rigreal" >&2
    exit 2
fi
root=$(cd -- "$(dirname -- "$(readlink -f -- "$0")")/.." && pwd)
cd "$root"
profile=release
[[ $1 != --dev ]] || profile=debug
for tool in node dnsmasq ip sudo systemd-run systemctl python3 flock; do
    command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
mkdir -p tgt
exec 9>tgt/rig-build.lock
flock -n 9 || { echo "Another rig build is already running." >&2; exit 1; }

bash tools/trueos-boot/prepare.sh "$1"

# Replace only this repository's PXE server. Keep it running after Cargo exits.
sudo -v
unit="trueos-rig-pxe-$(printf '%s' "$root" | sha256sum | cut -c1-12)"
if sudo -n systemctl is-active --quiet "$unit.service"; then
    sudo -n systemctl stop "$unit.service"
fi
sudo -n node tools/netboot/pxe3.js --stop
ready_dir=$(mktemp -d "$root/tgt/rig-pxe-ready.XXXXXX")
trap 'rm -rf -- "$ready_dir"' EXIT
sudo -n systemd-run --quiet --collect --unit="$unit" --service-type=exec \
    "$(command -v node)" "$root/tools/netboot/pxe3.js" \
    --ready-file "$ready_dir/ready" "${TRUEOS_PXE_INTERFACE:-}" "$root/tgt/usb-$profile"
for ((attempt = 0; attempt < 100; attempt++)); do
    if [[ -f $ready_dir/ready ]]; then
        echo "PXE ready: tgt/usb-$profile (logs: journalctl -u $unit.service)"
        bash tools/rig-reset.sh
        exit 0
    fi
    if ! sudo -n systemctl is-active --quiet "$unit.service"; then
        echo "PXE failed; rig was not reset. Check: journalctl -u $unit.service" >&2
        exit 1
    fi
    sleep 0.1
done
echo "PXE readiness timed out; rig was not reset. Check: journalctl -u $unit.service" >&2
exit 1
