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
  local sha256="$3"
  local relative_path="$4"
  local archive="${package_root}/${package}_${version}_amd64.deb"

  curl --fail --location --silent --show-error --retry 3 \
    --connect-timeout 15 --max-time 300 \
    --output "${archive}" \
    "http://de.archive.ubuntu.com/ubuntu/${relative_path}"
  printf '%s  %s\n' "${sha256}" "${archive}" | sha256sum --check --status || {
    echo "toolchain: checksum mismatch for ${package} ${version}" >&2
    exit 1
  }

  local actual_package actual_version
  actual_package="$(dpkg-deb -f "${archive}" Package)"
  actual_version="$(dpkg-deb -f "${archive}" Version)"
  if [[ "${actual_package}" != "${package}" || "${actual_version}" != "${version}" ]]; then
    echo "toolchain: ${archive} is ${actual_package} ${actual_version}, expected ${package} ${version}" >&2
    exit 1
  fi
  dpkg-deb --extract "${archive}" "${install_root}"
}

# Ubuntu archive indexes can point different mirrors at different bytes under
# the same package version. Pin the package contents so artifact locks remain
# reproducible across hosted runner images.
download_and_extract llvm-spirv-21 21.1.5-1 752e1444431271a97ec165c85053f78265fff8ad0220cea0557bf960e9cb763a pool/universe/s/spirv-llvm-translator-21/llvm-spirv-21_21.1.5-1_amd64.deb
download_and_extract libllvmspirvlib21.1 21.1.5-1 6c72bb05e2edb94159f7d66c49626ecbf5a5ecebdc9fbb385371cad592b574af pool/universe/s/spirv-llvm-translator-21/libllvmspirvlib21.1_21.1.5-1_amd64.deb
download_and_extract intel-ocloc 26.05.37020.3-1 c4055dff5c2c7e76ce91f0f252441c22c59eabc07b5e4de73929c0e9e69edcef pool/universe/i/intel-compute-runtime/intel-ocloc_26.05.37020.3-1_amd64.deb
download_and_extract libigc2 2.28.4-4 b7626c939d27434b3e107ad5b7d883c6115efe8cc59d6435b09ed09ce737e7c2 pool/universe/i/intel-graphics-compiler2/libigc2_2.28.4-4_amd64.deb
download_and_extract libigc2-tools 2.28.4-4 98f15440fd0c6e0f0501469950b62a60f7fdc5c0aba652a1fd2c4f00d698f7bc pool/universe/i/intel-graphics-compiler2/libigc2-tools_2.28.4-4_amd64.deb
download_and_extract libigdfcl2 2.28.4-4 4f5188166280a82356def0c305274cfe0c4b3bbc30e21c8413ee679f4b77ef8d pool/universe/i/intel-graphics-compiler2/libigdfcl2_2.28.4-4_amd64.deb
download_and_extract libigdgmm12 22.9.0+ds1-1 352c951a48a795a59aa94d235ca163d70239dfb686066ea8eb077e454a37d6a5 pool/universe/i/intel-gmmlib/libigdgmm12_22.9.0+ds1-1_amd64.deb
download_and_extract intel-opencl-icd 26.05.37020.3-1 d9a86663da5e64e9850444038c89efbede1a8265e9774e41b1a3be659fe5ed89 pool/universe/i/intel-compute-runtime/intel-opencl-icd_26.05.37020.3-1_amd64.deb

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
