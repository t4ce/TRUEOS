#!/usr/bin/env bash
set -euo pipefail

QEMU_BIN="${QEMU_BIN:-qemu-system-x86_64}"
ISO_PATH="${ISO_PATH:-bld/trueos.iso}"
QEMU_NVME_IMG="${QEMU_NVME_IMG:-tools/nvme.img}"
QEMU_MEMORY="${QEMU_MEMORY:-12000M}"
QEMU_SMP="${QEMU_SMP:-14}"
QEMU_NIC_DEVICE="${QEMU_NIC_DEVICE:-virtio-net-pci,disable-modern=off}"
QEMU_SERIAL="${QEMU_SERIAL:-tcp:127.0.0.1:5555,server,nowait}"

QEMU_MODE="${1:-iso}"
if [[ "${QEMU_MODE}" == "iso" || "${QEMU_MODE}" == "iso-debug" ]]; then
    shift || true
fi

QEMU_DEBUG_ARGS=()
if [[ "${QEMU_MODE}" == "iso-debug" ]]; then
    QEMU_DEBUG_ARGS+=("-S" "-s" "-no-reboot")
fi

QEMU_HOST_TCP_PORT_8081="${QEMU_HOST_TCP_PORT_8081:-18081}"
QEMU_HOST_TCP_PORT_3="${QEMU_HOST_TCP_PORT_3:-10003}"
QEMU_HOST_TCP_PORT_4="${QEMU_HOST_TCP_PORT_4:-10004}"
QEMU_HOST_TCP_PORT_100="${QEMU_HOST_TCP_PORT_100:-10100}"
QEMU_HOST_TCP_PORT_80="${QEMU_HOST_TCP_PORT_80:-8080}"
QEMU_HOST_TCP_PORT_54321="${QEMU_HOST_TCP_PORT_54321:-15432}"
QEMU_HOST_TCP_PORT_32123="${QEMU_HOST_TCP_PORT_32123:-32123}"
QEMU_HOST_TCP_PORT_NET_SHELL="${QEMU_HOST_TCP_PORT_NET_SHELL:-14245}"
QEMU_HOST_UDP_PORT_32343="${QEMU_HOST_UDP_PORT_32343:-32343}"
QEMU_NETDEV_USER="user,id=net1"
QEMU_NETDEV_USER+=",hostfwd=tcp:127.0.0.1:${QEMU_HOST_TCP_PORT_8081}-:8081"
QEMU_NETDEV_USER+=",hostfwd=tcp:127.0.0.1:${QEMU_HOST_TCP_PORT_3}-:3"
QEMU_NETDEV_USER+=",hostfwd=tcp:127.0.0.1:${QEMU_HOST_TCP_PORT_4}-:4"
QEMU_NETDEV_USER+=",hostfwd=tcp:127.0.0.1:${QEMU_HOST_TCP_PORT_100}-:100"
QEMU_NETDEV_USER+=",hostfwd=tcp:127.0.0.1:${QEMU_HOST_TCP_PORT_80}-:80"
QEMU_NETDEV_USER+=",hostfwd=tcp:0.0.0.0:${QEMU_HOST_TCP_PORT_54321}-:54321"
QEMU_NETDEV_USER+=",hostfwd=tcp:0.0.0.0:${QEMU_HOST_TCP_PORT_32123}-:32123"
QEMU_NETDEV_USER+=",hostfwd=tcp:127.0.0.1:${QEMU_HOST_TCP_PORT_NET_SHELL}-:4245"
QEMU_NETDEV_USER+=",hostfwd=udp:0.0.0.0:${QEMU_HOST_UDP_PORT_32343}-:32343"

exec env -i \
    "HOME=${HOME:-}" \
    "PATH=/usr/bin:/bin" \
    "TERM=${TERM:-xterm}" \
    "LANG=${LANG:-C.UTF-8}" \
    "DISPLAY=${DISPLAY:-}" \
    "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}" \
    "XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-}" \
    "XAUTHORITY=${XAUTHORITY:-}" \
    "${QEMU_BIN}" -no-shutdown \
    "${QEMU_DEBUG_ARGS[@]}" \
    "$@" \
    -display sdl,gl=on \
    -vga none \
    -device virtio-gpu-gl-pci,xres=2560,yres=1440 \
    -machine q35,accel=kvm:tcg \
    -bios "${QEMU_UEFI_FIRMWARE:?QEMU_UEFI_FIRMWARE is not set}" \
    -boot order=d \
    -cdrom "${ISO_PATH}" \
    -debugcon stdio \
    -D bld/qemu.log \
    -d int,guest_errors,cpu_reset,unimp \
    -m "${QEMU_MEMORY}" \
    -smp "cores=${QEMU_SMP}" \
    -cpu host,host-phys-bits=true \
    -serial "${QEMU_SERIAL}" \
    -netdev "${QEMU_NETDEV_USER}" \
    -device "${QEMU_NIC_DEVICE},netdev=net1,bus=pcie.0,addr=0x3" \
    -object rng-random,filename=/dev/urandom,id=rng0 \
    -device virtio-rng-pci,rng=rng0,disable-modern=off,bus=pcie.0,addr=0x4 \
    -audiodev none,id=snd0 \
    -device ich9-intel-hda,id=hda0,bus=pcie.0,addr=0x7 \
    -device hda-duplex,audiodev=snd0,bus=hda0.0 \
    -drive file="${QEMU_NVME_IMG}",format=raw,if=none,id=nvme \
    -device nvme,serial=deadbeef,drive=nvme 
#last state had usb stack here, just revert 1
