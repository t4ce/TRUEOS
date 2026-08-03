#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
local_root="${trueos_root}/bld/shadertoy-cpp-toolchain/root"
program="${trueos_root}/bld/tools/shadertoy_cpp_offline"

if [[ ! -x "${program}" ]]; then
  echo "preview: ${program} is missing; run make -C tools/shadertoy-cpp-offline" >&2
  exit 1
fi

if [[ -f "${local_root}/etc/OpenCL/vendors/intel.icd" ]]; then
  export OCL_ICD_VENDORS="${local_root}/etc/OpenCL/vendors"
  local_libraries="${local_root}/usr/lib/x86_64-linux-gnu:${local_root}/usr/lib/x86_64-linux-gnu/intel-opencl"
  export LD_LIBRARY_PATH="${local_libraries}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
fi

cd "${trueos_root}"
exec "${program}" "$@"

