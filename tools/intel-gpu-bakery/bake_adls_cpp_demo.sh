#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned bake for the Shell2/UI4 C++ generative demo suite.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

exec "${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${trueos_root}/crates/trueos-shader/gpgpu/kernels/cpp_demo_rgba8.clcpp" \
  --artifact-name cpp_demo_rgba8 \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${trueos_root}/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp" \
  --expect-kernel cpp_demo_rgba8 \
  --rust-symbol cpp_demo_rgba8=CPP_DEMO_RGBA8_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"
