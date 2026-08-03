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
  (
    cd "${package_root}"
    rm -f "${package}"_*.deb
    apt-get download "${package}"
  )
  local archive
  archive="$(find "${package_root}" -maxdepth 1 -type f -name "${package}_*.deb" -print -quit)"
  if [[ -z "${archive}" ]]; then
    echo "toolchain: apt-get did not download ${package}" >&2
    exit 1
  fi
  dpkg-deb --extract "${archive}" "${install_root}"
}

download_and_extract llvm-spirv-21
download_and_extract libllvmspirvlib21.1
download_and_extract intel-ocloc
download_and_extract libigc2
download_and_extract libigdfcl2
download_and_extract libigdgmm12
download_and_extract intel-opencl-icd

# Ubuntu's package records a system-absolute ICD path. This installation is
# intentionally local, so give the ICD loader a local vendor file and resolve
# the driver plus IGC dependencies through run.sh's library path.
printf '%s\n' 'libigdrcl.so' > "${install_root}/etc/OpenCL/vendors/intel.icd"

test -x "${install_root}/usr/lib/llvm-21/bin/clang"
test -x "${install_root}/usr/bin/llvm-spirv-21"
find "${install_root}/usr/bin" -maxdepth 1 -type f -name 'ocloc*' -perm -111 \
  -print -quit | grep -q .
test -f "${install_root}/usr/lib/x86_64-linux-gnu/intel-opencl/libigdrcl.so"

echo "Local shader toolchain ready under ${install_root}"
echo "The Intel compiler and OpenCL runtime are both local; use make run."
