#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned standalone bake for Spirit's maintained C++ visual suite.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"
kernel_dir="${trueos_root}/crates/trueos-shader/gpgpu/kernels"
publish_dir="${kernel_dir}/artifacts/adls/cpp"
common_args=(
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json"
  --variant cpp-native
  --publish-dir "${publish_dir}"
  --repro-check
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json"
)

"${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${kernel_dir}/spirit_vfx_background_rgba8.clcpp" \
  --artifact-name spirit_vfx_background_rgba8 \
  --expect-kernel spirit_vfx_background_rgba8 \
  --rust-symbol spirit_vfx_background_rgba8=SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_CPP_ABI_CONTRACT \
  "${common_args[@]}" \
  "$@"

"${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${kernel_dir}/spirit_vfx_sprite_rgba8.clcpp" \
  --artifact-name spirit_vfx_sprite_rgba8 \
  --expect-kernel spirit_vfx_sprite_rgba8 \
  --rust-symbol spirit_vfx_sprite_rgba8=SPIRIT_VFX_SPRITE_RGBA8_ADLS_CPP_ABI_CONTRACT \
  "${common_args[@]}" \
  "$@"
