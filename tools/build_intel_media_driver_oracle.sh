#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
oracle_root="${INTEL_MEDIA_DRIVER_ORACLE_ROOT:-$repo_root/bld/intel-media-driver-oracle}"
src_dir="${INTEL_MEDIA_DRIVER_SRC:-/home/t4ce/REPOS/reference/intel-media-driver}"
build_dir="${INTEL_MEDIA_DRIVER_BUILD_DIR:-$oracle_root/build}"
install_dir="${INTEL_MEDIA_DRIVER_INSTALL_DIR:-$oracle_root/install}"
trace_dir="${INTEL_MEDIA_DRIVER_TRACE_DIR:-$oracle_root}"
deps_dir="${INTEL_MEDIA_DRIVER_DEPS_DIR:-$oracle_root/deps}"
deps_src_dir="${INTEL_MEDIA_DRIVER_DEPS_SRC_DIR:-$oracle_root/src}"
deps_prefix="${INTEL_MEDIA_DRIVER_DEPS_PREFIX:-$deps_dir/prefix}"
venv_dir="${INTEL_MEDIA_DRIVER_VENV_DIR:-$oracle_root/venv}"
expected_commit="${INTEL_MEDIA_DRIVER_COMMIT:-a203cfc}"
jobs="${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf '1')}"
bootstrap_deps="${TRUEOS_INTEL_MEDIA_BOOTSTRAP_DEPS:-auto}"

libdrm_url="${INTEL_LIBDRM_URL:-https://gitlab.freedesktop.org/mesa/drm.git}"
libva_url="${INTEL_LIBVA_URL:-https://github.com/intel/libva.git}"
gmmlib_url="${INTEL_GMMLIB_URL:-https://github.com/intel/gmmlib.git}"
libdrm_ref="${INTEL_LIBDRM_REF:-}"
libva_ref="${INTEL_LIBVA_REF:-}"
gmmlib_ref="${INTEL_GMMLIB_REF:-}"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

pkg_exists() {
    PKG_CONFIG_PATH="$deps_prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
        pkg-config --exists "$1"
}

clone_dep() {
    local name="$1"
    local url="$2"
    local ref="$3"
    local dir="$deps_src_dir/$name"

    if [[ -d "$dir/.git" ]]; then
        return
    fi

    mkdir -p "$deps_src_dir"
    if [[ -n "$ref" ]]; then
        git clone --depth 1 --branch "$ref" "$url" "$dir"
    else
        git clone --depth 1 "$url" "$dir"
    fi
}

ensure_meson_env() {
    need_cmd python3
    if [[ ! -x "$venv_dir/bin/meson" || ! -x "$venv_dir/bin/ninja" ]]; then
        python3 -m venv "$venv_dir"
        "$venv_dir/bin/pip" install meson ninja
    fi
}

build_libdrm() {
    clone_dep libdrm "$libdrm_url" "$libdrm_ref"
    PATH="$venv_dir/bin:$PATH" meson setup --wipe "$deps_dir/libdrm-build" "$deps_src_dir/libdrm" \
        --prefix="$deps_prefix" \
        --libdir=lib \
        --buildtype=release \
        --wrap-mode=nofallback \
        -Dintel=disabled \
        -Dradeon=disabled \
        -Damdgpu=disabled \
        -Dnouveau=disabled \
        -Dvmwgfx=disabled \
        -Dudev=false \
        -Dcairo-tests=disabled \
        -Dman-pages=disabled \
        -Dvalgrind=disabled
    PATH="$venv_dir/bin:$PATH" meson compile -C "$deps_dir/libdrm-build"
    PATH="$venv_dir/bin:$PATH" meson install -C "$deps_dir/libdrm-build"
}

build_libva() {
    clone_dep libva "$libva_url" "$libva_ref"
    PATH="$venv_dir/bin:$PATH" \
        PKG_CONFIG_PATH="$deps_prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
        meson setup --wipe "$deps_dir/libva-build" "$deps_src_dir/libva" \
        --prefix="$deps_prefix" \
        --libdir=lib \
        --buildtype=release \
        --wrap-mode=nofallback \
        -Dwith_x11=no \
        -Dwith_wayland=no \
        -Dwith_glx=no \
        -Denable_docs=false
    PATH="$venv_dir/bin:$PATH" \
        PKG_CONFIG_PATH="$deps_prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
        meson compile -C "$deps_dir/libva-build"
    PATH="$venv_dir/bin:$PATH" meson install -C "$deps_dir/libva-build"
}

build_gmmlib() {
    clone_dep gmmlib "$gmmlib_url" "$gmmlib_ref"
    cmake -S "$deps_src_dir/gmmlib" -B "$deps_dir/gmmlib-build" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$deps_prefix" \
        -DCMAKE_INSTALL_LIBDIR=lib
    cmake --build "$deps_dir/gmmlib-build" --parallel "$jobs"
    cmake --install "$deps_dir/gmmlib-build"
}

ensure_oracle_deps() {
    local missing=()
    pkg_exists libdrm || missing+=(libdrm)
    pkg_exists libva || missing+=(libva)
    pkg_exists igdgmm || missing+=(igdgmm)

    if ((${#missing[@]} == 0)); then
        return
    fi

    if [[ "$bootstrap_deps" == "0" || "$bootstrap_deps" == "false" ]]; then
        die "missing pkg-config dependencies: ${missing[*]}"
    fi

    ensure_meson_env
    pkg_exists libdrm || build_libdrm
    pkg_exists libva || build_libva
    pkg_exists igdgmm || build_gmmlib
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

mkdir -p "$build_dir" "$install_dir" "$trace_dir" "$deps_dir"
ensure_oracle_deps

export PKG_CONFIG_PATH="$deps_prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$deps_prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

trace_file="$trace_dir/trueos_avc_recipe_trace.txt"
trace_bin="$build_dir/trueos_avc_recipe_trace"
rustc --edition=2024 "$repo_root/tools/avc_recipe_trace.rs" -o "$trace_bin"
"$trace_bin" >"$trace_file"

cmake -S "$src_dir" -B "$build_dir" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_CXX_FLAGS="-Wno-error=array-bounds ${CMAKE_CXX_FLAGS:-}" \
    -DCMAKE_INSTALL_PREFIX="$install_dir" \
    -DCMAKE_INSTALL_LIBDIR=lib \
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
    printf 'deps_prefix=%s\n' "$deps_prefix"
    printf 'pkg_config_path=%s\n' "$PKG_CONFIG_PATH"
} >"$manifest"

printf 'intel media oracle built:\n'
printf '  driver: %s\n' "$driver_so"
printf '  trace:  %s\n' "$trace_file"
printf '  deps:   %s\n' "$deps_prefix"
printf '  manifest: %s\n' "$manifest"
