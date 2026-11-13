#!/bin/bash
set -eu

profile=release
args=(--release)
buildins_option=
selection_selected=0
profile_selected=0
native_only=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --release|--dev)
            if [ "$profile_selected" = 1 ]; then
                echo "Choose only one of --release or --dev." >&2; exit 2
            fi
            profile_selected=1
            if [ "$1" = --dev ]; then profile=debug; args=(); fi
            ;;
        --no-buildins|--buildins)
            if [ "$selection_selected" = 1 ]; then
                echo "Choose only one of --buildins or --no-buildins." >&2; exit 2
            fi
            selection_selected=1
            if [ "$1" = --no-buildins ]; then
                buildins_option=none
            else
                if [ "$#" -lt 2 ]; then
                    echo "--buildins needs all, none, or a comma-separated app list." >&2; exit 2
                fi
                shift
                buildins_option=$1
            fi
            ;;
        --native-only)
            if [ "$native_only" = 1 ]; then
                echo "Choose --native-only only once." >&2; exit 2
            fi
            native_only=1
            ;;
        *) echo "usage: $0 [--release|--dev] [--no-buildins|--buildins all|none|APP,APP] [--native-only]" >&2; exit 2 ;;
    esac
    shift
done
root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"
app_profile=release
if [ "$profile" = debug ]; then app_profile=dev; fi
# Ordinary maintainer builds retain Cargo's defaults. Cloud jobs must provide
# a normalized manifest for this exact source commit and profile. The planner
# emits only allowlisted arguments, never a command or caller-supplied flags.
kernel_feature_args=()
buildins_list=
plan_args=()
if [ -n "${TRUEOS_CLOUDBUILD_SPEC:-}" ]; then
    plan_args=(--spec "$TRUEOS_CLOUDBUILD_SPEC")
elif [ "$selection_selected" = 1 ]; then
    plan_args=(--buildins "$buildins_option")
fi
if [ "${#plan_args[@]}" -gt 0 ]; then
    feature_args=$(python3 "$root/tools/cloudbuild/build_plan.py" \
        "${plan_args[@]}" --profile "$app_profile" --cargo-args)
    buildins_list=$(python3 "$root/tools/cloudbuild/build_plan.py" \
        "${plan_args[@]}" --profile "$app_profile" --buildins-list)
    mapfile -t kernel_feature_args <<< "$feature_args"
else
    buildins_list=$(python3 "$root/tools/cloudbuild/build_plan.py" \
        --buildins all --profile "$app_profile" --buildins-list)
fi
if [ -n "${TRUEOS_CLOUDBUILD_SPEC:-}" ] && [ "$selection_selected" = 1 ]; then
    requested_buildins=$(python3 "$root/tools/cloudbuild/build_plan.py" \
        --buildins "$buildins_option" --profile "$app_profile" --buildins-list)
    if [ "$requested_buildins" != "$buildins_list" ]; then
        echo "The app selection conflicts with the supplied build manifest; choose the exact buildins list in that manifest." >&2
        exit 2
    fi
fi
# Always replace a caller's stale selection. The exact list is validated above
# and is rechecked when build.rs embeds the prepared AppDB.
export TRUEOS_BUILDINS_SELECTION="$buildins_list"
export TRUEOS_BUILDINS_DIR="$root/tgt/buildins/$app_profile"
export TRUEOS_BUILDINS_NATIVE_ONLY="$native_only"
buildins_args=()
kernel_target=tgt
if [ "$native_only" = 1 ]; then
    mkdir -p "$root/tgt"
    experiment=$(mktemp -d "$root/tgt/native-only.XXXXXX")
    export CARGO_TARGET_DIR="$experiment/kernel"
    export TRUEOS_BUILDINS_DIR="$experiment/buildins/$app_profile"
    kernel_target="$CARGO_TARGET_DIR"
    buildins_args=(--native-only)
    printf 'Native-only clean build: %s\n' "$experiment"
    printf 'Default builtin manifest and build caches remain available.\n'
elif [ -n "${CARGO_TARGET_DIR:-}" ]; then
    kernel_target="$CARGO_TARGET_DIR"
fi
# The IDE API reference fingerprints Cargo.lock before invoking Cargo.
# Resolve both missing and stale locks before that fingerprint is taken.
# metadata preserves existing locked versions where possible, unlike rebuilding
# the lockfile with generate-lockfile. Do not use --no-deps: it skips resolution.
cargo metadata --format-version 1 > /dev/null
if [ -n "$buildins_list" ]; then
    export TRUEOS_BUILDINS_SOURCE=${TRUEOS_BUILDINS_SOURCE:-"$root/../TRUEOS-Buildins"}
    # The adapter stages Edit's generated API reference in its private output
    # tree. Source checkouts must remain clean before and after every job.
    export TRUEOS_API_SOURCE_ROOT="$root"
    python3 "$root/tools/cloudbuild/buildins.py" --source "$TRUEOS_BUILDINS_SOURCE" \
        --kernel "$root" --profile "$app_profile" --output "$TRUEOS_BUILDINS_DIR" \
        --selection "$buildins_list" "${buildins_args[@]}"
else
    printf 'Kernel-only build: skipping buildin compilation, API reference generation, and app payload embedding.\n'
fi
if [ "$TRUEOS_BUILDINS_NATIVE_ONLY" = 1 ]; then
    # The map records live symbols even though the release ELF strips them.
    cargo rustc --bin TRUEOS "${args[@]}" "${kernel_feature_args[@]}" -- -C "link-arg=-Map=$experiment/kernel.map"
    test -s "$experiment/kernel.map"
else
    cargo build "${args[@]}" "${kernel_feature_args[@]}"
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
    if [ -n "$buildins_list" ]; then
        cp "$TRUEOS_BUILDINS_DIR/buildins.json" "$destination/buildins.json"
        cp "$TRUEOS_BUILDINS_DIR/ready.json" "$destination/buildins-ready.json"
    else
        # There is no prepared AppDB or readiness record when the app stage is
        # omitted. Keep the exact empty selection beside the isolated kernel.
        printf '{"buildins": []}\n' > "$destination/buildins.json"
    fi
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
