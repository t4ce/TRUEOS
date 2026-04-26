#!/usr/bin/env bash
set -euo pipefail

mode="${1:-iso}"
shift || true

iso_path="${ISO_PATH:-bld/trueos.iso}"
ovmf="${QEMU_UEFI_FIRMWARE:-${OVMF_BUNDLE_PATH:-}}"
qemu_bin="${QEMU_BIN:-qemu-system-x86_64}"
qemu_bridge="${QEMU_BRIDGE:-br0}"
qemu_bridge_helper="${QEMU_BRIDGE_HELPER:-}"
qemu_hda_audiodev="${QEMU_HDA_AUDIODEV:-none,id=snd0}"

if [[ -z "$ovmf" ]]; then
    echo "QEMU_UEFI_FIRMWARE is not set" >&2
    exit 1
fi

qemu_env=(
    env -i
    "HOME=${HOME:-}"
    "PATH=/usr/bin:/bin"
    "TERM=${TERM:-xterm}"
    "LANG=${LANG:-C.UTF-8}"
    "DISPLAY=${DISPLAY:-}"
    "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}"
    "XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-}"
    "XAUTHORITY=${XAUTHORITY:-}"
)

gfx_flags=(
    -display sdl,gl=on
    -vga none
    -device virtio-gpu-gl-pci,disable-modern=off,xres=1440,yres=900
)

netdev="bridge,id=net1,br=${qemu_bridge}"
if [[ -n "$qemu_bridge_helper" ]]; then
    netdev="${netdev},helper=${qemu_bridge_helper}"
fi
net_flags=(
    -netdev "$netdev"
    -device virtio-net-pci,netdev=net1,disable-modern=off,bus=pcie.0,addr=0x3
)

rng_flags=(
    -object rng-random,filename=/dev/urandom,id=rng0
    -device virtio-rng-pci,rng=rng0,disable-modern=off,bus=pcie.0,addr=0x4
)

hda_flags=(
    -audiodev "$qemu_hda_audiodev"
    -device ich9-intel-hda,id=hda0,bus=pcie.0,addr=0x7
    -device hda-duplex,audiodev=snd0,bus=hda0.0
)

usb_flags=(
    -drive file=nvme.img,if=none,id=nvme
    -device nvme,serial=deadbeef,drive=nvme
    -device qemu-xhci,id=xhci,p2=8,p3=8,bus=pcie.0,addr=0x5
    -device usb-mouse,bus=xhci.0,port=1,id=usbmouse
    -device usb-tablet,bus=xhci.0,port=2,id=usbtablet
    -device usb-kbd,bus=xhci.0,port=3,id=usbkbd
)

case "$mode" in
    iso | run | run-with-nvme)
        exec "${qemu_env[@]}" "$qemu_bin" -no-shutdown \
            "${gfx_flags[@]}" \
            -enable-kvm \
            -machine q35 \
            -bios "$ovmf" \
            -boot order=d \
            -cdrom "$iso_path" \
            -debugcon stdio \
            -D bld/qemu.log \
            -d int,guest_errors,cpu_reset,unimp \
            -m 2000M \
            -smp cores=8 \
            -cpu host,host-phys-bits=true \
            -serial tcp:127.0.0.1:5555,server,nowait \
            "${net_flags[@]}" \
            "${rng_flags[@]}" \
            "${hda_flags[@]}" \
            "${usb_flags[@]}" \
            "$@"
        ;;
    iso-debug | dbg-vscode)
        exec "${qemu_env[@]}" "$qemu_bin" -no-shutdown \
            "${gfx_flags[@]}" \
            -machine q35 \
            -bios "$ovmf" \
            -cdrom "$iso_path" \
            -debugcon stdio \
            -D bld/qemu.log \
            -d int,guest_errors,cpu_reset,unimp \
            -m 2000M \
            -smp cores=4 \
            -cpu qemu64,phys-bits=39 \
            -serial tcp:127.0.0.1:5555,server,nowait \
            "${net_flags[@]}" \
            "${rng_flags[@]}" \
            "${hda_flags[@]}" \
            "${usb_flags[@]}" \
            "$@"
        ;;
    installed | run-installed)
        exec "${qemu_env[@]}" "$qemu_bin" -no-shutdown \
            "${gfx_flags[@]}" \
            -bios "$ovmf" \
            -debugcon stdio \
            -m 2000M \
            -smp cores=6 \
            -cpu qemu64,phys-bits=39 \
            -serial tcp:127.0.0.1:5555,server,nowait \
            "${net_flags[@]}" \
            "${rng_flags[@]}" \
            "${hda_flags[@]}" \
            "$@"
        ;;
    *)
        echo "usage: $0 {iso|iso-debug|installed} [extra qemu args...]" >&2
        exit 2
        ;;
esac
