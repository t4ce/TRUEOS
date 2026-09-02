#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
usage:
  stage-tree.sh <EFI/BOOT-directory> [FirmwareScout.efi]
  stage-tree.sh --restore <EFI/BOOT-directory>

The stage operation preserves the current BOOTX64.EFI as LIMINE.EFI and installs
FirmwareScout as BOOTX64.EFI. The restore operation copies LIMINE.EFI back.
USAGE
    exit 2
}

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
default_scout="$repo_root/bld/firmware-scout-target/x86_64-unknown-uefi/release/trueos-firmware-scout.efi"

if [[ ${1:-} == --restore ]]; then
    [[ $# -eq 2 ]] || usage
    boot_dir=$2
    [[ -f "$boot_dir/LIMINE.EFI" ]] || {
        echo "stage-tree: missing $boot_dir/LIMINE.EFI" >&2
        exit 1
    }
    cp -f -- "$boot_dir/LIMINE.EFI" "$boot_dir/BOOTX64.EFI"
    echo "stage-tree: restored $boot_dir/BOOTX64.EFI"
    exit 0
fi

[[ $# -ge 1 && $# -le 2 ]] || usage
boot_dir=$1
scout=${2:-$default_scout}
mkdir -p -- "$boot_dir"
[[ -f "$scout" ]] || {
    echo "stage-tree: FirmwareScout artifact not found: $scout" >&2
    echo "stage-tree: run tools/firmware-scout/build.sh first" >&2
    exit 1
}

if [[ ! -f "$boot_dir/LIMINE.EFI" ]]; then
    [[ -f "$boot_dir/BOOTX64.EFI" ]] || {
        echo "stage-tree: neither BOOTX64.EFI nor LIMINE.EFI exists in $boot_dir" >&2
        exit 1
    }
    cp -- "$boot_dir/BOOTX64.EFI" "$boot_dir/LIMINE.EFI"
fi

cp -f -- "$scout" "$boot_dir/BOOTX64.EFI"
echo "stage-tree: installed FirmwareScout as $boot_dir/BOOTX64.EFI"
echo "stage-tree: preserved loader as $boot_dir/LIMINE.EFI"
