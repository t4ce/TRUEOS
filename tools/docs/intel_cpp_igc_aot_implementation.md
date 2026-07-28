# Intel C++/IGC AOT implementation ledger

TRUEOS uses one Intel GPGPU architecture:

```text
C++ for OpenCL source
  -> Clang SPIR64 LLVM bitcode
  -> llvm-spirv OpenCL Kernel SPIR-V
  -> Intel ocloc/IGC Zebin
  -> JSON provenance + no_std Rust ABI contract
  -> include_bytes! in the TRUEOS kernel
  -> Rust direct-RCS/GuC submission
```

Clang, `llvm-spirv`, `ocloc`, and IGC are artifact-bakery dependencies only.
No C++ runtime, standard library, exceptions, RTTI, allocator, NEO, or OpenCL
loader enters the TRUEOS runtime.

## Architectural commitments

- Every maintained source uses `.clcpp`.
- Every publication lives below
  `crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/`.
- Runtime selection is unconditional; there is no Cargo feature, Make switch,
  alternate source frontend, or artifact fallback.
- Every source has one `.bin`, `.spv`, `.manifest.json`, and `.contract.rs`
  publication.
- The exact target is ADL-S `8086:4680`, revision `0x0c`.
- The current direct-RCS capability boundary is SIMD16 with zero scratch and
  zero SLM.

## ABI authority

The generated contract, not hand-maintained offsets, is the authority for:

- Zebin and SPIR-V hashes;
- text-section and entry offsets/sizes;
- SIMD and GRF counts;
- cross-thread and per-thread sizes;
- bindings and explicit argument payloads;
- implicit dispatch records, stateful buffer offsets, and private base;
- local-ID payload shape;
- pointer access/address modes and source argument metadata.

C++ IGC may emit a four-byte buffer offset for a stateful pointer and an
eight-byte private-memory base. TRUEOS admits these only where the runtime
value is guaranteed zero. Payload writers clear the entire indirect block
before writing explicit fields.

## Reproducible publication

`make intel-gpu-bake-cpp-artifacts` runs the pinned C++ profile for every
source. The bakery removes ambient compiler inputs, hashes the complete
toolchain/resource set, captures transitive headers, compiles in two isolated
roots, validates the exact entry set, and requires byte-identical results.

`make intel-gpu-verify-cpp-artifacts` is compiler-free. It reparses all
publications, regenerates contracts in memory, verifies manifest provenance,
and enforces the one-source/one-publication invariant.

The physical copy-rectangle proof remains the base hardware gate:

```sh
make intel-gpu-verify-copy-cpp-hardware-log \
  INTEL_GPU_CPP_PROBE_LOG=/path/to/copy-rect-probe.log
```

It checks the exact ADL-S identity, artifact hash, pitches, origins, even/odd
widths, guard pixels, completion markers, and timeout.

## Removed architecture

The former OpenCL-C source set, root `artifacts/adls/*.{bin,spv}` publications,
feature-selected copy artifact, and diagnostic font-outline mesh probe were
deleted after the C++ publications were baked and their runtime contracts were
wired. Historical commits remain the recovery path; production code contains
no fallback.
