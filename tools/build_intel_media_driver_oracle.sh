#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src_dir="${INTEL_MEDIA_DRIVER_SRC:-/home/t4ce/REPOS/reference/intel-media-driver}"
build_dir="${INTEL_MEDIA_DRIVER_BUILD_DIR:-$repo_root/bld/intel-media-driver-oracle/build}"
install_dir="${INTEL_MEDIA_DRIVER_INSTALL_DIR:-$repo_root/bld/intel-media-driver-oracle/install}"
trace_dir="${INTEL_MEDIA_DRIVER_TRACE_DIR:-$repo_root/bld/intel-media-driver-oracle}"
expected_commit="${INTEL_MEDIA_DRIVER_COMMIT:-a203cfc}"
jobs="${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf '1')}"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

need_pkg() {
    pkg-config --exists "$1" || missing_pkgs+=("$1")
}

need_cmd git
need_cmd cmake
need_cmd pkg-config
need_cmd rustc

[[ -d "$src_dir/.git" ]] || die "INTEL_MEDIA_DRIVER_SRC is not a git checkout: $src_dir"

actual_commit="$(git -C "$src_dir" rev-parse --short HEAD)"
if [[ "$actual_commit" != "$expected_commit" ]]; then
    die "intel/media-driver checkout is $actual_commit, expected $expected_commit"
fi

if [[ -n "$(git -C "$src_dir" status --porcelain)" ]]; then
    die "intel/media-driver checkout has local changes; set INTEL_MEDIA_DRIVER_SRC to a clean oracle checkout"
fi

missing_pkgs=()
need_pkg libva
need_pkg libdrm
need_pkg igdgmm
if ((${#missing_pkgs[@]})); then
    cat >&2 <<EOF
error: missing pkg-config dependencies: ${missing_pkgs[*]}

Install/provide LibVA, libdrm, and GmmLib before compiling the oracle.
On Debian/Ubuntu-like hosts the packages are typically:

    sudo apt install build-essential cmake pkg-config libva-dev libdrm-dev libigdgmm-dev

If the deps live in a local prefix, export PKG_CONFIG_PATH before rerunning.
EOF
    exit 1
fi

mkdir -p "$build_dir" "$install_dir" "$trace_dir"

trace_file="$trace_dir/trueos_avc_recipe_trace.txt"
trace_bin="$build_dir/trueos_avc_recipe_trace"
rustc --edition=2024 "$repo_root/tools/avc_recipe_trace.rs" -o "$trace_bin"
"$trace_bin" >"$trace_file"

cmake -S "$src_dir" -B "$build_dir" \
    -DCMAKE_BUILD_TYPE=RelWithDebInfo \
    -DCMAKE_INSTALL_PREFIX="$install_dir" \
    -DLIBVA_DRIVERS_PATH="$install_dir/lib/dri" \
    -DBUILD_CMRTLIB=OFF \
    -DENABLE_KERNELS=OFF \
    -DENABLE_NONFREE_KERNELS=OFF \
    -DBUILD_KERNELS=OFF \
    -DINSTALL_DRIVER_SYSCONF=OFF

cmake --build "$build_dir" --target iHD_drv_video --parallel "$jobs"

driver_so="$build_dir/media_driver/iHD_drv_video.so"
[[ -f "$driver_so" ]] || die "build finished but artifact is missing: $driver_so"

manifest="$trace_dir/manifest.txt"
{
    printf 'intel_media_driver_src=%s\n' "$src_dir"
    printf 'intel_media_driver_commit=%s\n' "$actual_commit"
    printf 'build_dir=%s\n' "$build_dir"
    printf 'driver_so=%s\n' "$driver_so"
    printf 'trueos_trace=%s\n' "$trace_file"
} >"$manifest"

printf 'intel media oracle built:\n'
printf '  driver: %s\n' "$driver_so"
printf '  trace:  %s\n' "$trace_file"
printf '  manifest: %s\n' "$manifest"
