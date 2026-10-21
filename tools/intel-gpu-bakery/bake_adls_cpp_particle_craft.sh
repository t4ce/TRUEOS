#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned bake for the stateful three-pass ParticleCraft engine.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

blueprints_root="${TRUEOS_BLUEPRINTS_DIR:-${trueos_root}/../TRUEOS-Blueprints}"
output_dir="${trueos_root}/bld/shadertoy-native/particle_craft"

"${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${blueprints_root}/apps/shadertoy/assets/particle_craft/kernel.clcpp" \
  --artifact-name particle_craft \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${output_dir}" \
  --expect-kernel particle_craft_step \
  --expect-kernel particle_craft_bin_tiles \
  --expect-kernel particle_craft_render_rgba8 \
  --rust-symbol particle_craft_step=PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol particle_craft_bin_tiles=PARTICLE_CRAFT_BIN_TILES_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol particle_craft_render_rgba8=PARTICLE_CRAFT_RENDER_RGBA8_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"

"${python_bin}" "${trueos_root}/tools/shadertoy-cpp-offline/import_native.py" \
  --blueprints-root "${blueprints_root}" --artifact-dir "${output_dir}" --program particle_craft
"${python_bin}" "${trueos_root}/tools/shadertoy-cpp-offline/package_blueprint.py" \
  --blueprints-root "${blueprints_root}" --update-trust
