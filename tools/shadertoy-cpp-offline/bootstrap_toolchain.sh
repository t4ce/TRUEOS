#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
install_root="${trueos_root}/bld/shadertoy-cpp-toolchain/root"
package_root="${trueos_root}/bld/shadertoy-cpp-toolchain/packages"
mkdir -p "${install_root}" "${package_root}"

# Clang is downloaded through the repository's SHA-512-pinned Ubuntu 26.04
# installer. The translator and ocloc packages are kept under bld as well;
# they do not mutate the host package database.
"${trueos_root}/tools/intel-gpu-bakery/install_locked_clang21.sh" "${install_root}"

download_and_extract() {
  local package="$1"
  local version="$2"
  (
    cd "${package_root}"
    rm -f "${package}"_*.deb
    apt-get download "${package}=${version}"
  )
  local archives
  mapfile -t archives < <(
    find "${package_root}" -maxdepth 1 -type f \
      -name "${package}_${version}_*.deb" -print | sort
  )
  if [[ "${#archives[@]}" -ne 1 ]]; then
    echo "toolchain: expected one ${package} ${version} archive, found ${#archives[@]}" >&2
    exit 1
  fi
  local actual_package actual_version
  actual_package="$(dpkg-deb -f "${archives[0]}" Package)"
  actual_version="$(dpkg-deb -f "${archives[0]}" Version)"
  if [[ "${actual_package}" != "${package}" || "${actual_version}" != "${version}" ]]; then
    echo "toolchain: ${archives[0]} is ${actual_package} ${actual_version}, expected ${package} ${version}" >&2
    exit 1
  fi
  dpkg-deb --extract "${archives[0]}" "${install_root}"
}

download_and_extract llvm-spirv-21 21.1.5-1
download_and_extract libllvmspirvlib21.1 21.1.5-1
download_and_extract intel-ocloc 26.05.37020.3-1
download_and_extract libigc2 2.28.4-4
download_and_extract libigc2-tools 2.28.4-4
download_and_extract libigdfcl2 2.28.4-4
download_and_extract libigdgmm12 22.9.0+ds1-1
download_and_extract intel-opencl-icd 26.05.37020.3-1

# Ubuntu's package records a system-absolute ICD path. This installation is
# intentionally local, so give the ICD loader a local vendor file and resolve
# the driver plus IGC dependencies through run.sh's library path.
printf '%s\n' 'libigdrcl.so' > "${install_root}/etc/OpenCL/vendors/intel.icd"

test -x "${install_root}/usr/lib/llvm-21/bin/clang"
test -x "${install_root}/usr/bin/llvm-spirv-21"
test -x "${install_root}/usr/bin/iga64"
find "${install_root}/usr/bin" -maxdepth 1 -type f -name 'ocloc*' -perm -111 \
  -print -quit | grep -q .
test -f "${install_root}/usr/lib/x86_64-linux-gnu/intel-opencl/libigdrcl.so"

echo "Local shader toolchain ready under ${install_root}"
echo "The Intel compiler and OpenCL runtime are both local; use make run."
