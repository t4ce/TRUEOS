#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned bake for the Shell2/UI4 C++ generative demo suite.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

blueprints_root="${TRUEOS_BLUEPRINTS_DIR:-${trueos_root}/../TRUEOS-Blueprints}"
output_dir="${trueos_root}/bld/shadertoy-native/cpp_gallery"

"${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${blueprints_root}/apps/shadertoy/assets/cpp_gallery/kernel.clcpp" \
  --artifact-name cpp_demo_rgba8 \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${output_dir}" \
  --expect-kernel cpp_demo_rgba8 \
  --rust-symbol cpp_demo_rgba8=CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"

"${python_bin}" "${trueos_root}/tools/shadertoy-cpp-offline/import_native.py" \
  --blueprints-root "${blueprints_root}" --artifact-dir "${output_dir}" --program cpp_gallery
"${python_bin}" "${trueos_root}/tools/shadertoy-cpp-offline/package_blueprint.py" \
  --blueprints-root "${blueprints_root}" --update-trust
