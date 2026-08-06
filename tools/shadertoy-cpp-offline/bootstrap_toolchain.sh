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
  local sha512="$3"
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
  printf '%s  %s\n' "${sha512}" "${archives[0]}" | sha512sum --check --status
  dpkg-deb --extract "${archives[0]}" "${install_root}"
}

download_and_extract llvm-spirv-21 21.1.5-1 0c9ce43b1d2bb490e7baa399ab5a9b11afbd3c1a5bf119aaf729e73ca8dfb0e8fdc881b30dc68634996df0754528690e5a6feed63d0fcbc015c3b1c849f4145b
download_and_extract libllvmspirvlib21.1 21.1.5-1 298c8007e430691fcd6bfb5108dd8bff5dc992ca27e56bf42c06ba1b63a1cf63b3b10b0fcf610e86dd612779ca41e382cf1ab21cf843e134a9c0f77f5a451091
download_and_extract intel-ocloc 26.05.37020.3-1 fff40fefcb50ddd32a2f47d1c5d597fefa0080b0c56469f1332865a7120d7163e237637d4a9be23fa2096474d5e969ba82f6f187fd527445d4915c4f32692953
download_and_extract libigc2 2.28.4-4 5deedbb792fa4758f3804d8795a4959b13bbe9472beeb686674ecd139c0e4c7ccf14cde621cc73bb0f62faf76e9cbce75211bc0b81f6a485c9b1e028e62424ef
download_and_extract libigdfcl2 2.28.4-4 412fb2156cccb5c4ed56e7b75bd3e9baac9131a8894f3d07188ab40cf0b466ee92acc9d746f0ee16c09cb0f14f276995b292b44b3f7a4f2eb992d7769328653b
download_and_extract libigdgmm12 22.9.0+ds1-1 4c13a00b79d9cda12e410126b9f8d5d33f13db5ce1419946f2f9991606793bb596a8b90b7f5eb5c63833c667dca0deea992fbb202b12076ecec106f882df3d45
download_and_extract intel-opencl-icd 26.05.37020.3-1 4fa641a24d39049373fe4eeee16ad713a208e453c27619c0ca05be9cff1152bbbc7d8ca609fb7c3343fa4d51026310dc8dd72597a9c6e433158bd2e67278cf82

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
