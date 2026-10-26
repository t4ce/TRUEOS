#!/bin/bash
set -eu
root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"
work=$(mktemp -d /tmp/trueos-boot-test.XXXXXXXX)
trap 'rm -rf -- "$work"' EXIT
# Compile the locked protocol definitions for the host without a C driver or
# changing the firmware target's build configuration.
r_efi_src=$(cargo metadata --manifest-path tools/trueos-boot/Cargo.toml \
    --format-version 1 --locked --offline | python3 -c '
import json, sys
from pathlib import Path
package = next(p for p in json.load(sys.stdin)["packages"] if p["name"] == "r-efi" and p["version"] == "5.3.0")
print(Path(package["manifest_path"]).parent / "src" / "lib.rs")')
rustc --edition 2018 --crate-name r_efi --crate-type rlib "$r_efi_src" \
    -o "$work/libr_efi.rlib"
rustc --edition 2024 --test tools/trueos-boot/tests/host.rs \
    --extern "r_efi=$work/libr_efi.rlib" \
    -C "linker=$root/tools/rust-host-ld" -Awarnings -o "$work/host"
"$work/host"
rustc --edition 2024 --test tools/trueos-boot/src/smp.rs \
    -C "linker=$root/tools/rust-host-ld" -Awarnings -o "$work/smp"
"$work/smp"
rustc --edition 2024 --test tools/trueos-boot/src/network.rs \
    --extern "r_efi=$work/libr_efi.rlib" \
    -C "linker=$root/tools/rust-host-ld" -o "$work/network"
"$work/network"
python3 tools/testpy/test_kernel_service_fallback.py
python3 tools/testpy/test_native_boot_display_metrics.py
