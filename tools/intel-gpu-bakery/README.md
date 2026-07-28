# TRUEOS Intel GPU artifact bakery

The bakery is an offline host tool for the repository's single Intel kernel
architecture: C++ for OpenCL. Runtime and ordinary kernel builds consume only
checked-in artifacts.

```text
.clcpp source
  -> Clang spir64 LLVM bitcode
  -> llvm-spirv OpenCL Kernel SPIR-V
  -> ocloc/IGC Intel Zebin
  -> ELF + .ze_info audit manifest
  -> generated no_std Rust ABI contract
```

Direct `clang --target=spirv64` publication is not accepted because the tested
toolchain loses OpenCL argument metadata. The pinned bitcode/translator route
preserves names, types, pointer access, and address modes.

## Complete publication

Set `CLANG`, `LLVM_SPIRV`, `OCLOC`, and `OCLOC_LD_LIBRARY_PATH` when the pinned
tools are not already discoverable, then run:

```sh
make intel-gpu-bake-cpp-artifacts
```

This refreshes every `.clcpp` source into
`crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/`. Migrated single-entry
kernels and Lab256 are handled by `bake_adls_cpp_migrated.sh`; focused scripts
retain the special multi-entry or application-specific policies for copy,
demo, audio, ParticleCraft, font instance, LFM2.5, and Spirit.

All publications use `variant=cpp-native` and the
`cpp-native-aot-v1` policy. There is no ABI-reference or fallback artifact.

## Publication gates

The bakery:

- checks the executable hashes, compiler libraries, Clang resource tree, and
  ocloc/IGC resources against `toolchains/adls-cpp-proof.lock.json`;
- removes ambient compiler override/include/library inputs;
- compiles in two distinct output roots and requires byte-identical bitcode,
  SPIR-V, and Zebin;
- captures the source and every dependency from Clang's depfile;
- validates the exact expected kernel set, SIMD16, and zero scratch/SLM;
- requires the sibling SPIR-V to match the section embedded in Zebin;
- parses ELF64, symbols, and `.ze_info`;
- publishes `.bin`, `.spv`, `.manifest.json`, and `.contract.rs` as one unit.

Toolchain changes are explicit review events. Generate and inspect a candidate
lock with `bake.py --write-toolchain-lock` before replacing the repository
lock.

## Compiler-free verification

CI and ordinary development machines run:

```sh
make intel-gpu-verify-cpp-artifacts
```

The verifier reparses every artifact, checks hashes and provenance, regenerates
the Rust contract in memory, and asserts a one-to-one mapping between `.clcpp`
sources and publications. It also asserts that the parent ADL-S artifact
directory contains no alternate `.bin`, `.spv`, manifest, or contract files.

The physical ADL-S copy-rectangle transcript remains a strict hardware gate:

```sh
make intel-gpu-verify-copy-cpp-hardware-log \
  INTEL_GPU_CPP_PROBE_LOG=/path/to/copy-rect-probe.log
```

## Contract boundary

The generated contract records section and entry ranges, SIMD/GRF counts,
scratch/SLM, cross-thread and per-thread payload sizes, dispatch implicit
records, stateful buffer offsets, local IDs, BTIs, explicit argument offsets,
pointer qualifiers, and source argument metadata.

The direct-RCS profile admits the three programmed dispatch records plus
compiler-emitted stateful buffer offsets and a private-memory base only when
their runtime values are guaranteed zero. Every payload writer clears the
complete indirect block before setting explicit fields.

Do not add `-O0` casually. With the pinned stack it removes stateful BTIs and
buffer-address records and therefore changes the runtime ABI.
