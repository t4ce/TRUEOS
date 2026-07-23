#!/usr/bin/env bash
set -euo pipefail

# Canonical, pinned proof bake. Tool binaries remain host dependencies; their
# versions, executable hashes, and compiler-library hashes must match the
# reviewed lock before any artifact is published.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"

exec "${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${trueos_root}/crates/trueos-shader/gpgpu/kernels/copy_rect_rgba8.clcpp" \
  --artifact-name copy_rect_rgba8 \
  --variant cpp \
  --publish-dir "${trueos_root}/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp" \
  --abi-reference-bin "${trueos_root}/crates/trueos-shader/gpgpu/kernels/artifacts/adls/copy_rect_rgba8.bin" \
  --expect-kernel copy_rect_rgba8 \
  --rust-symbol copy_rect_rgba8=COPY_RECT_RGBA8_ADLS_CPP_ABI_CONTRACT \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  "$@"
