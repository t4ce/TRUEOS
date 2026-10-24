#!/usr/bin/env bash
set -euo pipefail

# Canonical bakes for the production kernels migrated from OpenCL C. The
# remaining native C++ artifacts have focused scripts for their special ABI
# policies; Make composes both sets into the complete artifact publication.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
kernel_dir="${trueos_root}/crates/trueos-shader/gpgpu/kernels"
publish_dir="${kernel_dir}/artifacts/adls/cpp"
python_bin="${PYTHON:-python3}"

common_args=(
  --profile "${tool_dir}/profiles/adls-4680-r0c-cpp.json"
  --variant cpp-native
  --publish-dir "${publish_dir}"
  --repro-check
  --toolchain-lock "${tool_dir}/toolchains/adls-cpp-proof.lock.json"
)

single_entry_kernels=(
  alpha_blend_worklist_rgba8
  sprite_quad_worklist_rgba8
  ui4_compose_layers_rgba8
  ui4_nv12_tile64_to_rgba8_frame
)

for kernel in "${single_entry_kernels[@]}"; do
  rust_symbol="${kernel^^}_ADLS_CPP_ABI_CONTRACT"
  "${python_bin}" -B "${tool_dir}/bake.py" \
    --source "${kernel_dir}/${kernel}.clcpp" \
    --artifact-name "${kernel}" \
    "${common_args[@]}" \
    --expect-kernel "${kernel}" \
    --rust-symbol "${kernel}=${rust_symbol}" \
    "$@"
done

