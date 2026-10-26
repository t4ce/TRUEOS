#!/bin/bash
set -eu
root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"
work=$(mktemp -d /tmp/trueos-boot-test.XXXXXXXX)
trap 'rm -rf -- "$work"' EXIT
rustc --edition 2024 --test tools/trueos-boot/tests/host.rs \
    -C "linker=$root/tools/rust-host-ld" -Awarnings -o "$work/host"
"$work/host"
rustc --edition 2024 --test tools/trueos-boot/src/smp.rs \
    -C "linker=$root/tools/rust-host-ld" -Awarnings -o "$work/smp"
"$work/smp"
python3 tools/testpy/test_kernel_service_fallback.py
