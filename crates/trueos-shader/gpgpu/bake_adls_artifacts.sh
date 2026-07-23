#!/usr/bin/env bash
set -euo pipefail

# Compatibility entry point for the opt-in host bakery.  This script is never
# called by Cargo/build.rs: ordinary TRUEOS builds only consume checked-in
# artifacts.

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
shader_root="$(cd "${script_dir}/.." && pwd)"

if [[ -n "${TRUEOS_ROOT:-}" ]]; then
  trueos_root="${TRUEOS_ROOT}"
elif [[ -f "${shader_root}/../../Cargo.toml" ]]; then
  trueos_root="$(cd "${shader_root}/../.." && pwd)"
else
  trueos_root="${shader_root}"
fi

bakery="${trueos_root}/tools/intel-gpu-bakery/bake.py"
profile="${BAKE_PROFILE:-${trueos_root}/tools/intel-gpu-bakery/profiles/adls.json}"
python_bin="${PYTHON:-python3}"

if [[ ! -f "${bakery}" ]]; then
  echo "missing TRUEOS Intel GPU bakery: ${bakery}" >&2
  exit 1
fi

# Advanced/single-source mode exposes the full bakery CLI, including the C++
# side-by-side path:
#
#   bake_adls_artifacts.sh --source kernels/copy_rect_rgba8.clcpp \
#     --artifact-name copy_rect_rgba8 --variant cpp \
#     --publish-dir kernels/artifacts/adls/cpp --repro-check
if [[ "$#" -gt 0 && "${1}" == --* ]]; then
  exec "${python_bin}" "${bakery}" --profile "${profile}" "$@"
fi

device="${DEVICE:-0x4680}"
target="${TARGET:-adls}"
if [[ "${device}" != "0x4680" || "${target}" != "adls" ]]; then
  echo "DEVICE/TARGET overrides require a matching BAKE_PROFILE JSON" >&2
  echo "the bundled profile is pinned to target=adls device=0x4680" >&2
  exit 1
fi

kernel_dir="${script_dir}/kernels"
artifact_dir="${kernel_dir}/artifacts/${target}"
build_root="${BUILD_ROOT:-${shader_root}/bld/intel-tools/bake/${target}}"
local_tool_root="${IGC_ROOT:-${trueos_root}/bld/intel-tools/root}"
local_ocloc="${local_tool_root}/usr/bin/ocloc-26.05.1"
local_libdir="${local_tool_root}/usr/lib/x86_64-linux-gnu"

if [[ -z "${OCLOC:-}" && -x "${local_ocloc}" ]]; then
  export OCLOC="${local_ocloc}"
fi
if [[ -z "${OCLOC_LD_LIBRARY_PATH:-}" && -d "${local_libdir}" ]]; then
  export OCLOC_LD_LIBRARY_PATH="${local_libdir}"
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

repro_args=()
if [[ "${REPRO_CHECK:-0}" == "1" ]]; then
  repro_args+=(--repro-check)
fi

for kernel in "${kernels[@]}"; do
  src="${kernel_dir}/${kernel}.cl"
  if [[ ! -f "${src}" ]]; then
    echo "missing source: ${src}" >&2
    exit 1
  fi
  echo "bake ${target}/${kernel} device=${device}"
  "${python_bin}" "${bakery}" \
    --source "${src}" \
    --artifact-name "${kernel}" \
    --frontend ocloc-cl \
    --variant legacy \
    --profile "${profile}" \
    --publish-dir "${artifact_dir}" \
    --build-root "${build_root}" \
    "${repro_args[@]}"
done
