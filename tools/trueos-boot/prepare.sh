#!/bin/bash
set -eu

profile=release
args=(--release)
case "${1:-}" in
    '') ;;
    --release) ;;
    --dev) profile=debug; args=() ;;
    *) echo "usage: $0 [--release|--dev]" >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then echo "usage: $0 [--release|--dev]" >&2; exit 2; fi
root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"
cargo build "${args[@]}"
cargo boot
destination="$root/tgt/usb-$profile"
mkdir -p "$destination/EFI/BOOT"
cp tgt/boot/x86_64-unknown-uefi/release/BOOTX64.efi "$destination/EFI/BOOT/BOOTX64.EFI"
cp "tgt/x86_64-unknown-trueos/$profile/TRUEOS" "$destination/TRUEOS.elf"
cp firmware/i915/tgl_guc_70.bin "$destination/EFI/BOOT/tgl_guc_70.bin"
printf 'USB files ready in %s\n' "$destination"
