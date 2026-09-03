#!/usr/bin/env bash
set -euo pipefail

# Regenerate and reproducibly bake the reviewed, single-pass ShaderToy catalog.
# Runtime Blueprint code can select these exact-target artifacts; it never
# submits source text or arbitrary GPU addresses to the kernel.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"
adapter_dir="${trueos_root}/tools/shadertoy-cpp-offline"
source_dir="${trueos_root}/crates/trueos-shader/gpgpu/kernels/shadertoy"
kernel_dir="${trueos_root}/crates/trueos-shader/gpgpu/kernels"
publish_dir="${kernel_dir}/artifacts/adls/cpp"
profile="${tool_dir}/profiles/adls-4680-r0c-cpp.json"
toolchain_lock="${tool_dir}/toolchains/adls-shadertoy-cpp-proof.lock.json"

shaders=(mandelbrot cube_field nguyen palette_grid cosmic_strands)
for shader in "${shaders[@]}"; do
  kernel_name="shadertoy_${shader}"
  "${python_bin}" -B "${adapter_dir}/export_kernel.py" \
    "${source_dir}/${shader}.glsl" \
    "${kernel_dir}/${kernel_name}.clcpp" \
    --kernel-name "${kernel_name}"

  "${python_bin}" -B "${tool_dir}/bake.py" \
    --source "${kernel_dir}/${kernel_name}.clcpp" \
    --artifact-name "${kernel_name}" \
    --profile "${profile}" \
    --variant cpp-native \
    --publish-dir "${publish_dir}" \
    --expect-kernel "${kernel_name}" \
    --rust-symbol "${kernel_name}=SHADERTOY_${shader^^}_ADLS_CPP_ABI_CONTRACT" \
    --repro-check \
    --toolchain-lock "${toolchain_lock}" \
    "$@"
done
