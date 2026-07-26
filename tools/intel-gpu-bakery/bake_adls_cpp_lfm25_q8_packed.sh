#!/usr/bin/env bash
set -euo pipefail

# Canonical pinned bake for the fixed packed LFM2.5-350M Q8 projection.
# This is an experimental sibling of lfm25_q8_project, not a replacement.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

exec "${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${trueos_root}/crates/trueos-shader/gpgpu/kernels/lfm25_q8_project_packed.clcpp" \
  --artifact-name lfm25_q8_project_packed \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${trueos_root}/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp" \
  --expect-kernel lfm25_q8_project_packed \
  --rust-symbol lfm25_q8_project_packed=LFM25_Q8_PROJECT_PACKED_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"
