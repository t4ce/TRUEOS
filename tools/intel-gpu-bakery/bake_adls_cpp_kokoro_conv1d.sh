#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned bake for the i5-14500T dominant Kokoro ConvInteger lane.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

exec "${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${trueos_root}/crates/trueos-shader/gpgpu/kernels/kokoro_conv1d_u8_u8.clcpp" \
  --artifact-name kokoro_conv1d_u8_u8 \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${trueos_root}/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp" \
  --expect-kernel kokoro_conv1d_u8_u8 \
  --rust-symbol kokoro_conv1d_u8_u8=KOKORO_CONV1D_U8_U8_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"
