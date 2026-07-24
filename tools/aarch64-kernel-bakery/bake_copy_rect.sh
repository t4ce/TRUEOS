#!/usr/bin/env bash
set -euo pipefail

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"
publish_dir="${ARM_KERNEL_PUBLISH_DIR:-${trueos_root}/bld/aarch64-kernel-artifacts}"

exec "${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${trueos_root}/crates/trueos-shader/cpu/kernels/copy_rect_rgba8.cpp" \
  --artifact-name copy_rect_rgba8 \
  --profile "${tool_dir}/profiles/aarch64-none-elf.json" \
  --expect-entry trueos_arm_copy_rect_rgba8 \
  --publish-dir "${publish_dir}" \
  --repro-check \
  "$@"
