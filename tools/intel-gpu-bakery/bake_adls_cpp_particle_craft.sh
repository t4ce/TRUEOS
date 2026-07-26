#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned bake for the stateful three-pass ParticleCraft engine.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

exec "${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${trueos_root}/crates/trueos-shader/gpgpu/kernels/particle_craft.clcpp" \
  --artifact-name particle_craft \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${trueos_root}/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp" \
  --expect-kernel particle_craft_step \
  --expect-kernel particle_craft_bin_tiles \
  --expect-kernel particle_craft_render_rgba8 \
  --rust-symbol particle_craft_step=PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol particle_craft_bin_tiles=PARTICLE_CRAFT_BIN_TILES_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol particle_craft_render_rgba8=PARTICLE_CRAFT_RENDER_RGBA8_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"
