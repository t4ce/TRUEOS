#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned bake for the i5-14500T Kokoro U8xI8 projection kernel.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

exec "${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${trueos_root}/crates/trueos-shader/gpgpu/kernels/kokoro_qgemm_u8_i8.clcpp" \
  --artifact-name kokoro_qgemm_u8_i8 \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${trueos_root}/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp" \
  --expect-kernel kokoro_qgemm_u8_i8 \
  --rust-symbol kokoro_qgemm_u8_i8=KOKORO_QGEMM_U8_I8_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"
