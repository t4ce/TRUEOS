#!/usr/bin/env bash
set -euo pipefail

# Bake the small RCS0 subset-sum probe.  Publication is intentionally explicit:
# the kernel cannot be admitted until the generated Zebin, SPIR-V, manifest,
# and ABI contract are all reviewed together.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
kernel_dir="${trueos_root}/crates/trueos-shader/gpgpu/kernels"
publish_dir="${kernel_dir}/artifacts/adls/cpp"
python_bin="${PYTHON:-python3}"

"${python_bin}" -B "${tool_dir}/bake.py" \
  --source "${kernel_dir}/subset_sum_collapse5_merge10.clcpp" \
  --artifact-name subset_sum_collapse5_merge10 \
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json" \
  --variant cpp-native \
  --publish-dir "${publish_dir}" \
  --repro-check \
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json" \
  --expect-kernel subset_sum_collapse5_merge10 \
  --rust-symbol subset_sum_collapse5_merge10=SUBSET_SUM_COLLAPSE5_MERGE10_ADLS_CPP_ABI_CONTRACT \
  "$@"
