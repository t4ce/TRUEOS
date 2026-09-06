#!/usr/bin/env bash
set -euo pipefail

# Regenerate and reproducibly bake the reviewed, single-pass ShaderToy catalog.
# Blueprint-owned packages carry the payloads; the kernel keeps the trust records.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
python_bin="${PYTHON:-python3}"
adapter_dir="${trueos_root}/tools/shadertoy-cpp-offline"
blueprints_root="${TRUEOS_BLUEPRINTS_ROOT:-${trueos_root}/../TRUEOS-Blueprints}"
assets_dir="${blueprints_root}/apps/shadertoy/assets"
kernel_dir="${trueos_root}/crates/trueos-shader/gpgpu/kernels"
publish_dir="${trueos_root}/bld/shadertoy-blueprint-bake/published"

shaders=(mandelbrot cube_field nguyen palette_grid cosmic_strands protean_clouds)
for shader in "${shaders[@]}"; do
  profile="${tool_dir}/profiles/adls-4680-r0c-cpp.json"
  toolchain_lock="${tool_dir}/toolchains/adls-shadertoy-cpp-proof.lock.json"
  # Keep the fractal and sine-hashed geometry byte-stable. The four visual
  # effects below have separate fast-math image comparisons and admission.
  case "${shader}" in
    nguyen|palette_grid|cosmic_strands|protean_clouds)
      profile="${tool_dir}/profiles/adls-4680-r0c-shadertoy.json"
      toolchain_lock="${tool_dir}/toolchains/adls-shadertoy-relaxed.lock.json"
      ;;
  esac
  kernel_name="shadertoy_${shader}"
  shader_dir="${assets_dir}/${shader}"
  "${python_bin}" -B "${adapter_dir}/export_kernel.py" \
    "${shader_dir}/input.glsl" \
    "${shader_dir}/kernel.clcpp" \
    --kernel-name "${kernel_name}"

  # Compile an exact staging copy under TRUEOS so provenance paths are relative
  # and package hashes do not depend on the Blueprint checkout's absolute path.
  staged_source_dir="${trueos_root}/bld/shadertoy-blueprint-bake/sources/${shader}"
  mkdir -p "${staged_source_dir}"
  cp "${shader_dir}/kernel.clcpp" "${staged_source_dir}/kernel.clcpp"

  "${python_bin}" -B "${tool_dir}/bake.py" \
    --source "${staged_source_dir}/kernel.clcpp" \
    --artifact-name "${kernel_name}" \
    --profile "${profile}" \
    --variant cpp-native \
    --publish-dir "${publish_dir}" \
    --expect-kernel "${kernel_name}" \
    --rust-symbol "${kernel_name}=SHADERTOY_${shader^^}_ADLS_CPP_ABI_CONTRACT" \
    --repro-check \
    --toolchain-lock "${toolchain_lock}" \
    "$@"
  for extension in bin spv manifest.json contract.rs; do
    cp "${publish_dir}/${kernel_name}.${extension}" "${shader_dir}/kernel.${extension}"
  done
  # Only the ABI/hash contract remains a compiled kernel input.
  cp "${shader_dir}/kernel.contract.rs" "${kernel_dir}/artifacts/adls/cpp/${kernel_name}.contract.rs"
done

"${python_bin}" -B "${adapter_dir}/package_blueprint.py" \
  --blueprints-root "${blueprints_root}" --update-trust
