#!/usr/bin/env bash
set -euo pipefail

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
iso="${TRUEOS_ISO:-$here/trueos.iso}"
ovmf="${TRUEOS_OVMF:-$here/ovmf-code-x86_64.fd}"

if ! command -v qemu-system-x86_64 >/dev/null 2>&1; then
    echo "qemu-system-x86_64 not found. Install QEMU first, for example: brew install qemu"
    exit 1
fi

if [ ! -f "$iso" ]; then
    echo "ISO not found: $iso"
    exit 1
fi

if [ ! -f "$ovmf" ]; then
    echo "OVMF firmware not found: $ovmf"
    echo "Set TRUEOS_OVMF=/path/to/ovmf-code-x86_64.fd or keep the bundled file next to this script."
    exit 1
fi

exec qemu-system-x86_64 \
    -accel tcg,thread=multi \
    -machine q35 \
    -cpu qemu64 \
    -m 2G \
    -smp 3 \
    -drive if=pflash,unit=0,format=raw,file="$ovmf",readonly=on \
    -boot d \
    -cdrom "$iso" \
    -display cocoa,gl=on \
    -vga none \
    -device virtio-gpu-gl-pci,disable-modern=off,xres=1920,yres=1080 \
    -audiodev none,id=snd0 \
    -device ich9-intel-hda,id=hda0,bus=pcie.0,addr=0x7 \
    -device hda-duplex,audiodev=snd0,bus=hda0.0
