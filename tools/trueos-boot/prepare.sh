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
export TRUEOS_BUILDINS_SOURCE=${TRUEOS_BUILDINS_SOURCE:-"$root/../TRUEOS-Buildins"}
app_profile=release
if [ "$profile" = debug ]; then app_profile=dev; fi
export TRUEOS_BUILDINS_DIR="$root/tgt/buildins/$app_profile"
python3 "$TRUEOS_BUILDINS_SOURCE/tools/build.py" --kernel "$root" --profile "$app_profile" --output "$TRUEOS_BUILDINS_DIR"
cargo build "${args[@]}"
cargo boot
# Publish an immutable complete generation; PXE never observes partial copies.
mkdir -p "$root/tgt/rig-images"
destination=$(mktemp -d "$root/tgt/rig-images/$profile.XXXXXX")
bundle_link=rigreal.tar.lz4
if [ "$profile" = debug ]; then bundle_link=rigdev.tar.lz4; fi
trap 'rm -f -- "$root/tgt/.pxe-current.$$" "$root/tgt/.$bundle_link.$$" "$root/tgt/.TrueOS.tar.lz4.$$"' EXIT
mkdir -p "$destination/EFI/BOOT"
cp tgt/boot/x86_64-unknown-uefi/release/BOOTX64.efi "$destination/EFI/BOOT/BOOTX64.EFI"
# Firmware retains the runtime file below 4 GiB. Keep host debug information in
# Cargo's original ELF rather than consuming boot-time DMA memory with it.
sysroot=$(rustc --print sysroot)
LD_LIBRARY_PATH="$sysroot/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    "$sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-objcopy" \
    --strip-debug "tgt/x86_64-unknown-trueos/$profile/TRUEOS" "$destination/TRUEOS.elf"
cp firmware/i915/tgl_guc_70.bin "$destination/EFI/BOOT/tgl_guc_70.bin"
# Complete the single update archive before publishing any part of this generation.
python3 "$root/tools/trueos-boot/bundle.py" "$destination"
# Preserve the existing USB staging path as well as the shared PXE selection.
mkdir -p "$root/tgt/usb-$profile/EFI/BOOT"
cp "$destination/EFI/BOOT/BOOTX64.EFI" "$root/tgt/usb-$profile/EFI/BOOT/BOOTX64.EFI"
cp "$destination/TRUEOS.elf" "$root/tgt/usb-$profile/TRUEOS.elf"
cp "$destination/EFI/BOOT/tgl_guc_70.bin" "$root/tgt/usb-$profile/EFI/BOOT/tgl_guc_70.bin"
ln -s "rig-images/$(basename -- "$destination")/TrueOS.tar.lz4" "$root/tgt/.$bundle_link.$$"
mv -Tf "$root/tgt/.$bundle_link.$$" "$root/tgt/$bundle_link"
ln -s "pxe-current/TrueOS.tar.lz4" "$root/tgt/.TrueOS.tar.lz4.$$"
mv -Tf "$root/tgt/.TrueOS.tar.lz4.$$" "$root/tgt/TrueOS.tar.lz4"
ln -s "rig-images/$(basename -- "$destination")" "$root/tgt/.pxe-current.$$"
mv -Tf "$root/tgt/.pxe-current.$$" "$root/tgt/pxe-current"
printf 'USB files ready in tgt/usb-%s; PXE files published via tgt/pxe-current\n' "$profile"
printf 'Live-update archive published via tgt/%s (latest: tgt/TrueOS.tar.lz4)\n' "$bundle_link"
