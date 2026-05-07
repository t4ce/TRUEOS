#!/usr/bin/env bash
set -euo pipefail

exec env -i \
    "HOME=${HOME:-}" \
    "PATH=/usr/bin:/bin" \
    "TERM=${TERM:-xterm}" \
    "LANG=${LANG:-C.UTF-8}" \
    "DISPLAY=${DISPLAY:-}" \
    "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}" \
    "XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-}" \
    "XAUTHORITY=${XAUTHORITY:-}" \
    qemu-system-x86_64 -no-shutdown \
    -display sdl,gl=on \
    -vga none \
    -device virtio-gpu-gl-pci,xres=1600,yres=900 \
    -machine q35,accel=kvm:tcg \
    -bios "${QEMU_UEFI_FIRMWARE:?QEMU_UEFI_FIRMWARE is not set}" \
    -boot order=d \
    -cdrom bld/trueos.iso \
    -debugcon stdio \
    -D bld/qemu.log \
    -d int,guest_errors,cpu_reset,unimp \
    -m 8000M \
    -smp cores=14 \
    -cpu host,host-phys-bits=true \
    -serial tcp:127.0.0.1:5555,server,nowait \
    -netdev user,id=net1 \
    -device virtio-net-pci,netdev=net1,disable-modern=off,bus=pcie.0,addr=0x3 \
    -object rng-random,filename=/dev/urandom,id=rng0 \
    -device virtio-rng-pci,rng=rng0,disable-modern=off,bus=pcie.0,addr=0x4 \
    -audiodev none,id=snd0 \
    -device ich9-intel-hda,id=hda0,bus=pcie.0,addr=0x7 \
    -device hda-duplex,audiodev=snd0,bus=hda0.0 \
    -drive file=nvme.img,if=none,id=nvme \
    -device nvme,serial=deadbeef,drive=nvme \
    -device qemu-xhci,id=xhci,p2=8,p3=8,bus=pcie.0,addr=0x5 \
    -device usb-mouse,bus=xhci.0,port=1,id=usbmouse \
    -device usb-tablet,bus=xhci.0,port=2,id=usbtablet \
    -device usb-kbd,bus=xhci.0,port=3,id=usbkbd
