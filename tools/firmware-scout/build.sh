#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
toolchain=${TRUEOS_RUST_TOOLCHAIN:-nightly-2026-07-10}

rustup target add --toolchain "$toolchain" x86_64-unknown-uefi
(
    cd "$script_dir"
    cargo "+$toolchain" build --locked --release
)

artifact="$repo_root/bld/firmware-scout-target/x86_64-unknown-uefi/release/trueos-firmware-scout.efi"
printf '%s\n' "$artifact"
