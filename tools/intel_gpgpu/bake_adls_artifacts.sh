#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"

device="${DEVICE:-0x4680}"
target="${TARGET:-adls}"
kernel_dir="${repo_root}/src/intel/kernels"
artifact_dir="${kernel_dir}/artifacts/${target}"
build_root="${BUILD_ROOT:-${repo_root}/bld/intel-tools/bake/${target}}"
local_tool_root="${IGC_ROOT:-${repo_root}/bld/intel-tools/root}"
local_ocloc="${local_tool_root}/usr/bin/ocloc-26.05.1"
local_libdir="${local_tool_root}/usr/lib/x86_64-linux-gnu"

uses_local_toolchain=0
if [[ -n "${OCLOC:-}" ]]; then
  ocloc="${OCLOC}"
elif [[ -x "${local_ocloc}" ]]; then
  ocloc="${local_ocloc}"
  uses_local_toolchain=1
else
  ocloc="$(command -v ocloc || true)"
fi

if [[ -z "${ocloc}" || ! -x "${ocloc}" ]]; then
  echo "missing ocloc; set OCLOC=/path/to/ocloc or extract intel-ocloc into ${local_tool_root}" >&2
  exit 1
fi

ld_library_path="${OCLOC_LD_LIBRARY_PATH:-}"
if [[ -z "${ld_library_path}" && "${uses_local_toolchain}" -eq 1 && -d "${local_libdir}" ]]; then
  ld_library_path="${local_libdir}"
fi
if [[ -n "${LD_LIBRARY_PATH:-}" ]]; then
  ld_library_path="${ld_library_path:+${ld_library_path}:}${LD_LIBRARY_PATH}"
fi

if [[ "$#" -gt 0 ]]; then
  kernels=("$@")
else
  kernels=()
  while IFS= read -r src; do
    kernel="$(basename "${src}" .cl)"
    if [[ -f "${artifact_dir}/${kernel}.bin" ]]; then
      kernels+=("${kernel}")
    fi
  done < <(find "${kernel_dir}" -maxdepth 1 -type f -name '*.cl' | sort)
fi

mkdir -p "${build_root}" "${artifact_dir}"

for kernel in "${kernels[@]}"; do
  src="${kernel_dir}/${kernel}.cl"
  out_dir="${build_root}/${kernel}"

  if [[ ! -f "${src}" ]]; then
    echo "missing source: ${src}" >&2
    exit 1
  fi

  rm -rf "${out_dir}"
  mkdir -p "${out_dir}"
  echo "bake ${target}/${kernel} device=${device}"
  env LD_LIBRARY_PATH="${ld_library_path}" "${ocloc}" compile \
    -file "${src}" \
    -device "${device}" \
    -64 \
    -output "${kernel}" \
    -out_dir "${out_dir}" \
    -output_no_suffix \
    -gen_file

  env LD_LIBRARY_PATH="${ld_library_path}" "${ocloc}" validate \
    -file "${out_dir}/${kernel}.bin"

  cp "${out_dir}/${kernel}.bin" "${artifact_dir}/${kernel}.bin"
  cp "${out_dir}/${kernel}.spv" "${artifact_dir}/${kernel}.spv"
  sha256sum "${artifact_dir}/${kernel}.bin" "${artifact_dir}/${kernel}.spv"
done
