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
# Publish an immutable complete generation; PXE never observes partial copies.
mkdir -p "$root/tgt/rig-images"
destination=$(mktemp -d "$root/tgt/rig-images/$profile.XXXXXX")
trap 'rm -f -- "$root/tgt/.pxe-current.$$"' EXIT
mkdir -p "$destination/EFI/BOOT"
cp tgt/boot/x86_64-unknown-uefi/release/BOOTX64.efi "$destination/EFI/BOOT/BOOTX64.EFI"
cp "tgt/x86_64-unknown-trueos/$profile/TRUEOS" "$destination/TRUEOS.elf"
cp firmware/i915/tgl_guc_70.bin "$destination/EFI/BOOT/tgl_guc_70.bin"
# Preserve the existing USB staging path as well as the shared PXE selection.
mkdir -p "$root/tgt/usb-$profile/EFI/BOOT"
cp "$destination/EFI/BOOT/BOOTX64.EFI" "$root/tgt/usb-$profile/EFI/BOOT/BOOTX64.EFI"
cp "$destination/TRUEOS.elf" "$root/tgt/usb-$profile/TRUEOS.elf"
cp "$destination/EFI/BOOT/tgl_guc_70.bin" "$root/tgt/usb-$profile/EFI/BOOT/tgl_guc_70.bin"
ln -s "rig-images/$(basename -- "$destination")" "$root/tgt/.pxe-current.$$"
mv -Tf "$root/tgt/.pxe-current.$$" "$root/tgt/pxe-current"
printf 'USB files ready in tgt/usb-%s; PXE files published via tgt/pxe-current\n' "$profile"
