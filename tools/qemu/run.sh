#!/usr/bin/env bash
set -euo pipefail

QEMU_BIN="${QEMU_BIN:-qemu-system-x86_64}"
ISO_PATH="${ISO_PATH:-bld/trueos.iso}"
QEMU_NVME_IMG="${QEMU_NVME_IMG:-tools/nvme.img}"
QEMU_MEMORY="${QEMU_MEMORY:-4000M}"
QEMU_SMP="${QEMU_SMP:-14}"
QEMU_NIC_DEVICE="${QEMU_NIC_DEVICE:-virtio-net-pci,disable-modern=off}"
# Hold the guest until the COM1 logger has connected, so early output is
# captured by tools/emulator-log-capture.sh rather than racing the boot.
QEMU_SERIAL="${QEMU_SERIAL:-tcp:127.0.0.1:5555,server,wait}"
QEMU_WIFI_ONLY="${QEMU_WIFI_ONLY:-1}"
# Optional host PCI address to expose through VFIO, for example 06:00.0.
# The host device must already be safely detached from its native driver and
# bound to vfio-pci before QEMU is started.
QEMU_VFIO_HOST="${QEMU_VFIO_HOST:-}"
QEMU_VFIO_WIFI_HOST="${QEMU_VFIO_WIFI_HOST:-00:14.3}"

# VFIO must DMA-map guest RAM, which is limited by RLIMIT_MEMLOCK.  Ubuntu's
# default interactive limit is often only a few MiB, so transparently retry
# this opt-in passthrough launch with an unlimited root memlock allowance.
# The normal non-VFIO launcher never enters this path.
if [[ -n "${QEMU_VFIO_HOST}" || -n "${QEMU_VFIO_WIFI_HOST}" ]] \
   && [[ "${QEMU_VFIO_MEMLOCK_REEXEC:-0}" != "1" ]]; then
    qemu_memlock_kib="$(ulimit -l)"
    if [[ "${qemu_memlock_kib}" != "unlimited" && "${qemu_memlock_kib}" =~ ^[0-9]+$ && "${qemu_memlock_kib}" -lt 16777216 ]]; then
        exec sudo env \
            "HOME=${HOME:-}" \
            "PATH=${PATH:-/usr/bin:/bin}" \
            "TERM=${TERM:-xterm}" \
            "LANG=${LANG:-C.UTF-8}" \
            "DISPLAY=${DISPLAY:-}" \
            "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-}" \
            "XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-}" \
            "XAUTHORITY=${XAUTHORITY:-}" \
            "ISO_PATH=${ISO_PATH}" \
            "QEMU_BIN=${QEMU_BIN}" \
            "QEMU_NVME_IMG=${QEMU_NVME_IMG}" \
            "QEMU_MEMORY=${QEMU_MEMORY}" \
            "QEMU_SMP=${QEMU_SMP}" \
            "QEMU_NIC_DEVICE=${QEMU_NIC_DEVICE}" \
            "QEMU_SERIAL=${QEMU_SERIAL}" \
            "QEMU_WIFI_ONLY=${QEMU_WIFI_ONLY}" \
            "QEMU_VFIO_HOST=${QEMU_VFIO_HOST}" \
            "QEMU_VFIO_WIFI_HOST=${QEMU_VFIO_WIFI_HOST}" \
            "QEMU_UEFI_FIRMWARE=${QEMU_UEFI_FIRMWARE:?QEMU_UEFI_FIRMWARE is not set}" \
            "QEMU_VFIO_MEMLOCK_REEXEC=1" \
            bash -c 'ulimit -l unlimited && exec "$@"' -- "${BASH_SOURCE[0]}" "$@"
    fi
fi

QEMU_MODE="${1:-iso}"
if [[ "${QEMU_MODE}" == "iso" || "${QEMU_MODE}" == "iso-debug" ]]; then
    shift || true
fi

QEMU_DEBUG_ARGS=()
if [[ "${QEMU_MODE}" == "iso-debug" ]]; then
    QEMU_DEBUG_ARGS+=("-S" "-s" "-no-reboot")
fi

QEMU_VFIO_ARGS=()
if [[ -n "${QEMU_VFIO_HOST}" ]]; then
    QEMU_VFIO_ARGS+=("-device" "vfio-pci,host=${QEMU_VFIO_HOST}")
fi
if [[ -n "${QEMU_VFIO_WIFI_HOST}" ]]; then
    QEMU_VFIO_ARGS+=("-device" "vfio-pci,host=${QEMU_VFIO_WIFI_HOST},bus=pcie.0")
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

QEMU_NETWORK_ARGS=()
if [[ "${QEMU_WIFI_ONLY}" == "1" ]]; then
    if [[ -z "${QEMU_VFIO_WIFI_HOST}" ]]; then
        echo "QEMU_WIFI_ONLY=1 requires QEMU_VFIO_WIFI_HOST (for example 00:14.3)" >&2
        exit 2
    fi
else
    QEMU_NETWORK_ARGS+=(
        "-netdev" "${QEMU_NETDEV_USER}"
        "-device" "${QEMU_NIC_DEVICE},netdev=net1,bus=pcie.0,addr=0x3"
    )
fi

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
    "${QEMU_VFIO_ARGS[@]}" \
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
    "${QEMU_NETWORK_ARGS[@]}" \
    -object rng-random,filename=/dev/urandom,id=rng0 \
    -device virtio-rng-pci,rng=rng0,disable-modern=off,bus=pcie.0,addr=0x4 \
    -audiodev none,id=snd0 \
    -device ich9-intel-hda,id=hda0,bus=pcie.0,addr=0x7 \
    -device hda-duplex,audiodev=snd0,bus=hda0.0 \
    -drive file="${QEMU_NVME_IMG}",format=raw,if=none,id=nvme \
    -device nvme,serial=deadbeef,drive=nvme 
#last state had usb stack here, just revert 1
