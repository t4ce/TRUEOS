#!/usr/bin/env bash
set -euo pipefail

# Build reviewed-but-unpublished candidate Zebins on the pinned TRUEOS ADL-S
# bakery host. This intentionally publishes below bld/ and never mutates the
# maintained kernel artifact directory.

tool_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
trueos_root="$(cd "${tool_dir}/../.." && pwd)"
bakery_dir="${trueos_root}/tools/intel-gpu-bakery"
publish_dir="${INTEL_GPU_PRIMITIVES_PUBLISH_DIR:-${trueos_root}/bld/intel-gpu-primitives-adls}"
python_bin="${PYTHON:-python3}"

common_args=(
  --profile "${bakery_dir}/profiles/adls-4680-r0c-cpp.json"
  --variant cpp-native
  --publish-dir "${publish_dir}"
  --repro-check
  --toolchain-lock "${bakery_dir}/toolchains/adls-cpp-proof.lock.json"
)

"${python_bin}" -B "${bakery_dir}/bake.py" \
  --source "${tool_dir}/collective_probe_simd16.clcpp" \
  --artifact-name collective_probe_simd16 \
  "${common_args[@]}" \
  --expect-kernel collective_probe_simd16 \
  --rust-symbol collective_probe_simd16=COLLECTIVE_PROBE_SIMD16_ADLS_CPP_ABI_CONTRACT \
  "$@"

"${python_bin}" -B "${bakery_dir}/bake.py" \
  --source "${tool_dir}/parallel_u32.clcpp" \
  --artifact-name parallel_u32 \
  "${common_args[@]}" \
  --expect-kernel parallel_u32_normalize_flags \
  --expect-kernel parallel_u32_scan_tiles_exclusive \
  --expect-kernel parallel_u32_add_tile_offsets \
  --expect-kernel parallel_u32_reduce_sum_tiles \
  --expect-kernel parallel_u32_select_indices \
  --expect-kernel parallel_u32_write_selected_count \
  --rust-symbol parallel_u32_normalize_flags=PARALLEL_U32_NORMALIZE_FLAGS_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_scan_tiles_exclusive=PARALLEL_U32_SCAN_TILES_EXCLUSIVE_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_add_tile_offsets=PARALLEL_U32_ADD_TILE_OFFSETS_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_reduce_sum_tiles=PARALLEL_U32_REDUCE_SUM_TILES_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_select_indices=PARALLEL_U32_SELECT_INDICES_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_write_selected_count=PARALLEL_U32_WRITE_SELECTED_COUNT_ADLS_CPP_ABI_CONTRACT \
  "$@"

"${python_bin}" -B "${bakery_dir}/bake.py" \
  --source "${tool_dir}/radix_u32.clcpp" \
  --artifact-name radix_u32 \
  "${common_args[@]}" \
  --expect-kernel radix_u32_histogram_tiles_4bit \
  --expect-kernel radix_u32_scan_tile_histograms_4bit \
  --expect-kernel radix_u32_histogram_totals_4bit \
  --expect-kernel radix_u32_bin_bases_4bit \
  --expect-kernel radix_u32_scatter_4bit \
  --rust-symbol radix_u32_histogram_tiles_4bit=RADIX_U32_HISTOGRAM_TILES_4BIT_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol radix_u32_scan_tile_histograms_4bit=RADIX_U32_SCAN_TILE_HISTOGRAMS_4BIT_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol radix_u32_histogram_totals_4bit=RADIX_U32_HISTOGRAM_TOTALS_4BIT_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol radix_u32_bin_bases_4bit=RADIX_U32_BIN_BASES_4BIT_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol radix_u32_scatter_4bit=RADIX_U32_SCATTER_4BIT_ADLS_CPP_ABI_CONTRACT \
  "$@"

"${python_bin}" -B "${bakery_dir}/bake.py" \
  --source "${tool_dir}/group_u32.clcpp" \
  --artifact-name group_u32 \
  "${common_args[@]}" \
  --expect-kernel parallel_u32_histogram_tiles_16 \
  --expect-kernel parallel_u32_rle_mark_heads \
  --expect-kernel parallel_u32_rle_emit_runs \
  --expect-kernel parallel_u32_rle_emit_lengths \
  --expect-kernel parallel_u32_segmented_scan_tiles_exclusive \
  --expect-kernel parallel_u32_segmented_scan_tile_carries \
  --expect-kernel parallel_u32_segmented_add_tile_carries \
  --expect-kernel parallel_u32_segmented_emit_totals \
  --rust-symbol parallel_u32_histogram_tiles_16=PARALLEL_U32_HISTOGRAM_TILES_16_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_rle_mark_heads=PARALLEL_U32_RLE_MARK_HEADS_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_rle_emit_runs=PARALLEL_U32_RLE_EMIT_RUNS_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_rle_emit_lengths=PARALLEL_U32_RLE_EMIT_LENGTHS_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_segmented_scan_tiles_exclusive=PARALLEL_U32_SEGMENTED_SCAN_TILES_EXCLUSIVE_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_segmented_scan_tile_carries=PARALLEL_U32_SEGMENTED_SCAN_TILE_CARRIES_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_segmented_add_tile_carries=PARALLEL_U32_SEGMENTED_ADD_TILE_CARRIES_ADLS_CPP_ABI_CONTRACT \
  --rust-symbol parallel_u32_segmented_emit_totals=PARALLEL_U32_SEGMENTED_EMIT_TOTALS_ADLS_CPP_ABI_CONTRACT \
  "$@"

printf 'parallel-u32 candidates published to %s\n' "${publish_dir}"
