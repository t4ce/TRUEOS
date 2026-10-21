#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned bake for the single-kernel live PCM audiovisual instrument.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

blueprints_root="${TRUEOS_BLUEPRINTS_DIR:-${trueos_root}/../TRUEOS-Blueprints}"
output_dir="${trueos_root}/bld/shadertoy-native/audio_visualizer"

"${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${blueprints_root}/apps/shadertoy/assets/audio_visualizer/kernel.clcpp" \
  --artifact-name cpp_audio_visualizer_rgba8 \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${output_dir}" \
  --expect-kernel cpp_audio_visualizer_rgba8 \
  --rust-symbol cpp_audio_visualizer_rgba8=CPP_AUDIO_VISUALIZER_RGBA8_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"

"${python_bin}" "${trueos_root}/tools/shadertoy-cpp-offline/import_native.py" \
  --blueprints-root "${blueprints_root}" --artifact-dir "${output_dir}" --program audio_visualizer
"${python_bin}" "${trueos_root}/tools/shadertoy-cpp-offline/package_blueprint.py" \
  --blueprints-root "${blueprints_root}" --update-trust
