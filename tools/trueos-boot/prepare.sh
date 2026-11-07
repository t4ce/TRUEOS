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
if [ "$#" -gt 2 ] || { [ "$#" -eq 2 ] && [ "$2" != --native-only ]; }; then
    echo "usage: $0 [--release|--dev] [--native-only]" >&2; exit 2
fi
root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"
export TRUEOS_BUILDINS_SOURCE=${TRUEOS_BUILDINS_SOURCE:-"$root/../TRUEOS-Buildins"}
app_profile=release
if [ "$profile" = debug ]; then app_profile=dev; fi
export TRUEOS_BUILDINS_DIR="$root/tgt/buildins/$app_profile"
export TRUEOS_BUILDINS_NATIVE_ONLY=0
buildins_args=()
kernel_target=tgt
if [ "${2:-}" = --native-only ]; then
    mkdir -p "$root/tgt"
    experiment=$(mktemp -d "$root/tgt/native-only.XXXXXX")
    export CARGO_TARGET_DIR="$experiment/kernel"
    export TRUEOS_BUILDINS_DIR="$experiment/buildins/$app_profile"
    export TRUEOS_BUILDINS_NATIVE_ONLY=1
    kernel_target="$CARGO_TARGET_DIR"
    buildins_args=(--native-only)
    printf 'Native-only clean build: %s\n' "$experiment"
    printf 'Default builtin manifest and build caches remain available.\n'
elif [ -n "${CARGO_TARGET_DIR:-}" ]; then
    kernel_target="$CARGO_TARGET_DIR"
fi
# The IDE API reference fingerprints Cargo.lock before invoking Cargo.
# Fresh checkouts have no lock.
if [ ! -f Cargo.lock ]; then
    cargo generate-lockfile
fi
# edit's build script consumes this reference from the sibling Blueprint crate.
# Generate it before buildins fingerprints its inputs, outside app Cargo's env.
export TRUEOS_API_SOURCE_ROOT="$root"
python3 "$root/tools/build_api_prm.py" --output "$root/../TRUEOS-Blueprints/crates/trueos-ide/api-prm"
python3 "$TRUEOS_BUILDINS_SOURCE/tools/build.py" --kernel "$root" --profile "$app_profile" --output "$TRUEOS_BUILDINS_DIR" "${buildins_args[@]}"
if [ "$TRUEOS_BUILDINS_NATIVE_ONLY" = 1 ]; then
    # The map records live symbols even though the release ELF strips them.
    cargo rustc --bin TRUEOS "${args[@]}" -- -C "link-arg=-Map=$experiment/kernel.map"
    test -s "$experiment/kernel.map"
else
    cargo build "${args[@]}"
fi
cargo boot
# Publish an immutable complete generation; PXE never observes partial copies.
mkdir -p "$root/tgt/rig-images"
destination=$(mktemp -d "$root/tgt/rig-images/$profile.XXXXXX")
bundle_link=rigreal.tar.lz4
if [ "$profile" = debug ]; then bundle_link=rigdev.tar.lz4; fi
trap 'rm -f -- "$root/tgt/.pxe-current.$$" "$root/tgt/.$bundle_link.$$" "$root/tgt/.TrueOS.tar.lz4.$$"' EXIT
mkdir -p "$destination/EFI/BOOT"
cp tgt/boot/x86_64-unknown-uefi/release/BOOTX64.efi "$destination/EFI/BOOT/BOOTX64.EFI"
# Firmware retains the runtime file below 4 GiB. Normal kernel profiles omit
# debug data; also remove any introduced by local Cargo profile overrides.
sysroot=$(rustc --print sysroot)
LD_LIBRARY_PATH="$sysroot/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    "$sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-objcopy" \
    --strip-debug "$kernel_target/x86_64-unknown-trueos/$profile/TRUEOS" "$destination/TRUEOS.elf"
cp firmware/i915/tgl_guc_70.bin "$destination/EFI/BOOT/tgl_guc_70.bin"
# Complete the single update archive before publishing any part of this generation.
python3 "$root/tools/trueos-boot/bundle.py" "$destination"
if [ "$TRUEOS_BUILDINS_NATIVE_ONLY" = 1 ]; then
    cp "$TRUEOS_BUILDINS_DIR/buildins.json" "$destination/buildins.json"
    cp "$TRUEOS_BUILDINS_DIR/ready.json" "$destination/buildins-ready.json"
    printf '%s\n' "$experiment" > "$destination/native-only-build-root"
fi
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
