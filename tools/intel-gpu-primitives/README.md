# TRUEOS SIMD16 parallel-u32 incubator

This directory fires the first four reusable C++/GPU primitive families without
changing the maintained runtime artifact catalog yet:

1. an exact SIMD16 collective probe;
2. modular `u32` exclusive scan, stable index selection, and sum reduction;
3. stable four-bit LSD radix-sort passes;
4. sixteen-bin histogram, run-length encoding, and segmented scan/reduction.

The sources are ordinary freestanding C++ for OpenCL. Every kernel requires one
16-lane subgroup, uses caller-owned global buffers, and deliberately contains no
SLM, barriers, or atomics. A workgroup owns one subgroup. The base tile is 256
items: sixteen rows of sixteen lanes.

## Cloud builds

The GitHub workflow has two independent jobs.

### Portable source proof

The fast job performs the work that needs no Intel backend installation:

- compiles all four sources to SPIR64 LLVM bitcode in two independent output
  roots and requires byte-identical results;
- emits textual LLVM IR and checks the exact 20 `spir_kernel` entries;
- requires the `intel_reqd_sub_group_size(16)` metadata on every entry;
- rejects SLM, barriers, atomics, or allocation in the v1 sources;
- runs bit-exact host semantics at subgroup, tile, recursive, overflow, stable
  ordering, RLE, and cross-tile segmented boundaries;
- writes a SHA-256 manifest and uploads the complete compiler proof.

Run the same lane locally:

```sh
make -C tools/intel-gpu-primitives cloud-verify
```

### Locked ADL-S candidate bake

The second job enters an Ubuntu 26.04 container, installs the repository's
SHA-512-pinned Clang 21 stack plus the exact `llvm-spirv`, `ocloc`, IGC, and
OpenCL packages named by the existing proof lock, then invokes the normal Intel
GPU bakery. It uploads candidate SPIR-V, Zebin, provenance manifests, generated
Rust ABI contracts, and aggregate SHA-256 values.

This is a real offline target bake, but still not runtime admission. Candidate
artifacts remain below `bld/intel-gpu-primitives-adls`; they are never copied
into `crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/` automatically.
Promotion still requires ISA/ABI review, runtime wiring, and a physical
`8086:4680` revision `0x0c` transcript.

The same candidate bake can be run on the pinned compiler host:

```sh
make -C tools/intel-gpu-primitives bake-adls-candidates
```

## Dispatch recipes

Every launch below uses local size `16 x 1 x 1`.

### Scan, reduction, and selection

For `N` elements:

- `parallel_u32_normalize_flags`: `ceil(N / 16)` groups;
- `parallel_u32_scan_tiles_exclusive`: `ceil(N / 256)` groups;
- `parallel_u32_add_tile_offsets`: the same `ceil(N / 256)` tile groups;
- `parallel_u32_reduce_sum_tiles`: `ceil(N / 256)` groups;
- `parallel_u32_select_indices`: `ceil(N / 16)` groups;
- `parallel_u32_write_selected_count`: exactly one group.

Recursively scan the tile sums emitted by the tile-scan entry, then apply
scanned offsets from the top level downward. The same recursive shape reduces
sums. Stable selection normalizes arbitrary nonzero predicates, scans the
normalized predicate, scatters surviving input indices, and writes the final
cardinality.

### Radix sort

One stable four-bit pass uses:

- `radix_u32_histogram_tiles_4bit`: `ceil(N / 256)` groups;
- `radix_u32_scan_tile_histograms_4bit`: exactly 16 groups, one per bin;
- `radix_u32_histogram_totals_4bit`: exactly one group;
- `radix_u32_bin_bases_4bit`: exactly one group;
- `radix_u32_scatter_4bit`: `ceil(N / 256)` groups.

Repeat for shifts `0, 4, ..., 28`, swapping caller-owned key/value buffers after
each pass. Duplicate keys retain input order.

### RLE and segmented operations

For RLE:

- `parallel_u32_rle_mark_heads`: `ceil(N / 16)` groups;
- scan the binary head flags with the generic scan flow;
- `parallel_u32_rle_emit_runs`: `ceil(N / 16)` groups;
- write `run_count` with the one-group selected-count entry;
- `parallel_u32_rle_emit_lengths`: `ceil(run_count / 16)` groups.

For segmented scan and reduction:

- `parallel_u32_segmented_scan_tiles_exclusive`: `ceil(N / 256)` groups;
- `parallel_u32_segmented_scan_tile_carries`: exactly one group;
- `parallel_u32_segmented_add_tile_carries`: `ceil(N / 256)` groups;
- scan binary head flags with the generic scan flow;
- `parallel_u32_segmented_emit_totals`: `ceil(N / 16)` groups.

The tile scan first writes local output and four-word tile metadata. The compact
metadata pass derives incoming carries, and the final tile pass adds each carry
only before that tile's first head. Head flags are exactly `0` or `1`; non-empty
input requires `head_flags[0] == 1`.

## Contract boundary

`semantic-contract-v1.json` fixes the mathematical behavior, launch width,
modular overflow, ordering, temporary-storage ownership, and current aliasing
rule. It intentionally separates semantic identity from the generated
machine-level Rust contracts produced by the Intel bakery.
