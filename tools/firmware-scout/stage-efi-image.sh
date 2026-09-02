#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'USAGE'
usage:
  stage-efi-image.sh <efi.img> [FirmwareScout.efi]
  stage-efi-image.sh --restore <efi.img>

Requires mcopy from mtools. The FAT image must already contain
EFI/BOOT/BOOTX64.EFI or EFI/BOOT/LIMINE.EFI.
USAGE
    exit 2
}

command -v mcopy >/dev/null 2>&1 || {
    echo "stage-efi-image: mcopy is required" >&2
    exit 1
}

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
default_scout="$repo_root/bld/firmware-scout-target/x86_64-unknown-uefi/release/trueos-firmware-scout.efi"
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT

if [[ ${1:-} == --restore ]]; then
    [[ $# -eq 2 ]] || usage
    image=$2
    mcopy -n -i "$image" ::/EFI/BOOT/LIMINE.EFI "$tmp/LIMINE.EFI"
    mcopy -o -i "$image" "$tmp/LIMINE.EFI" ::/EFI/BOOT/BOOTX64.EFI
    echo "stage-efi-image: restored BOOTX64.EFI from LIMINE.EFI"
    exit 0
fi

[[ $# -ge 1 && $# -le 2 ]] || usage
image=$1
scout=${2:-$default_scout}
[[ -f "$image" ]] || {
    echo "stage-efi-image: image not found: $image" >&2
    exit 1
}
[[ -f "$scout" ]] || {
    echo "stage-efi-image: FirmwareScout artifact not found: $scout" >&2
    exit 1
}

if ! mcopy -n -i "$image" ::/EFI/BOOT/LIMINE.EFI "$tmp/LIMINE.EFI" 2>/dev/null; then
    mcopy -n -i "$image" ::/EFI/BOOT/BOOTX64.EFI "$tmp/LIMINE.EFI"
    mcopy -o -i "$image" "$tmp/LIMINE.EFI" ::/EFI/BOOT/LIMINE.EFI
fi
mcopy -o -i "$image" "$scout" ::/EFI/BOOT/BOOTX64.EFI

echo "stage-efi-image: installed FirmwareScout as EFI/BOOT/BOOTX64.EFI"
echo "stage-efi-image: preserved loader as EFI/BOOT/LIMINE.EFI"
