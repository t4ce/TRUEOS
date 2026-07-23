# Intel C++/IGC AOT implementation ledger

This document records the design decisions, compiler behavior, and promotion
gates for TRUEOS's C++ for OpenCL frontend. It is an implementation ledger, not
an assertion that TRUEOS contains an OpenCL runtime.

The boundary remains:

```text
C++ for OpenCL source
  -> Clang SPIR64 LLVM bitcode
  -> llvm-spirv OpenCL Kernel SPIR-V
  -> Intel ocloc/IGC Zebin
  -> generated JSON provenance + no_std Rust ABI contract
  -> include_bytes! in the TRUEOS kernel
  -> Rust direct-RCS/GuC submission
```

Clang, `llvm-spirv`, `ocloc`, and IGC are release-artifact bakery
dependencies. Ordinary kernel builds consume checked-in artifacts and do not
load or link any of them. C++ templates and types disappear during the offline
bake; no C++ runtime, standard library, exception runtime, RTTI, allocator,
NEO, or OpenCL loader is part of the TRUEOS runtime.

## Three milestones

### 1. Reproducible bakery and generated contract

The host bakery owns:

- a target profile with the exact PCI device allowlist and revision range;
- C++ for OpenCL compilation to transient LLVM bitcode;
- metadata-preserving translation to OpenCL Kernel SPIR-V;
- Intel device compilation and `ocloc validate`;
- ELF, symbol-table, and `.ze_info` inspection;
- direct-RCS capability checks;
- byte-for-byte reproducibility checks;
- a full JSON provenance manifest;
- a compact generated Rust contract for runtime admission.

The generated contract distinguishes the executable ELF section from the
kernel entry symbol:

- text section name, file offset, and complete section size;
- entry file offset and function-symbol size;
- SIMD width and GRF count;
- scratch and SLM requirements;
- rounded cross-thread and exact per-thread payload sizes;
- binding-table indices;
- argument payload offsets, sizes, access, and address mode;
- Zebin and SPIR-V SHA-256 values;
- exact target PCI IDs and revision range.

The JSON additionally retains source argument names, type spellings,
qualifiers, per-thread payload records, tool versions, normalized commands,
and hashes for the source plus transitive quoted headers.

### 2. Runtime admission and target enforcement

Every artifact, whether embedded or read from TRUEOSFS, must pass the same
admission sequence before DMA allocation:

1. validate the target policy;
2. match the claimed Intel PCI device and revision;
3. validate the Intel ELF identity and structure;
4. require the allowlisted Zebin SHA-256 for every kernel;
5. when a generated contract is attached, validate its schema and the current
   direct-RCS capability envelope;
6. bind the contract to the artifact and SPIR-V hashes;
7. verify the generated ELF section and entry ranges.

An on-disk override is therefore not a mechanism for injecting a newly built
binary. Until signed external manifests exist, it can only reproduce an
already allowlisted artifact.

The initial artifact target is deliberately exact: ADL-S device `0x4680`,
revisions `0x00..0xff`. ADL-N and Raptor Lake require their own bake profile
and checked artifact instead of inheriting ADL-S by family resemblance.

### 3. First opt-in and hardware promotion

`copy_rect_rgba8.clcpp` is the first source-side twin. It retains the existing
entry symbol and argument order and uses a tiny freestanding header to prove
that `constexpr`, namespaces, and templates remain compile-time facilities.

The legacy OpenCL C artifact remains the default. The C++ pair lives under
`kernels/artifacts/adls/cpp/` and is selected only by the
`intel_gpu_cpp_aot` Cargo feature. Both frontends feed the same Rust payload,
surface-state, interface-descriptor, walker, and GuC code.

Host-side ABI equivalence is necessary but not sufficient for promotion. The
bare-metal copy probe must cover:

- even and odd widths;
- non-zero source and destination origins;
- different source and destination pitches;
- untouched row padding and guard pixels;
- retirement markers and CPU readback;
- the selected artifact hash, frontend, PCI ID, and revision.

Deleting the C fallback requires a second build using the legacy artifact and
equal destination results for the same cases.

## Findings that changed the implementation

### Direct Clang SPIR-V was executable but metadata-incomplete

With the tested LLVM 21 stack, `clang --target=spirv64 -x clcpp` produced
executable SPIR-V and a valid Zebin, but OpenCL argument metadata was degraded:
`uint` appeared as diagnostic `int`, the `const` source pointer became
`readwrite`, and names needed explicit preservation.

The publishable path is therefore:

```text
clang --target=spir64 -x clcpp -cl-std=CLC++
      -cl-kernel-arg-info -fno-discard-value-names -emit-llvm

llvm-spirv --preserve-ocl-kernel-arg-type-metadata-through-string

ocloc compile -spirv_input -device 0x4680 -64
```

Clang and `llvm-spirv` must use compatible LLVM majors because LLVM bitcode is
not a stable interchange format. Bitcode remains a disposable intermediate;
SPIR-V is the durable frontend/backend boundary.

An explicit `-O0` is not equivalent to leaving the tested Clang optimization
spelling unset in this pipeline. With `-O0`, the translated module caused IGC
to emit a 14,464-byte Zebin with no binding-table entries or
`buffer_address` payload records. The contract generator rejected it because
the pointer metadata and address payloads no longer paired. The published
profile therefore deliberately pins the absence of an `-O*` switch; adding or
changing an optimization flag is an ABI-affecting toolchain change that must
pass the legacy comparison again.

### Reproducibility includes path spelling

Two identical clean invocations reproduced LLVM bitcode, SPIR-V, and Zebin
byte-for-byte. Passing the source once by absolute path and once by a relative
path changed all hashes because Clang embeds the source spelling. The bakery
therefore invokes Clang from the source directory with a normalized basename
and performs its reproducibility build under a second temporary root.

### `ocloc` writes outside `-out_dir`

The tested package can create `IGC_REVISION` and `NEO_REVISION` in its process
working directory even when `-out_dir` is provided. Every invocation must run
with its current directory set to a disposable build directory.

### ELF section size is not function size

For the checked copy kernel, the entry starts at file offset `0x40`. The
function symbol is 712 bytes while `.text.copy_rect_rgba8` is 896 bytes.
Collapsing those into one `text_size` causes either incorrect runtime
validation or an ambiguous dispatch contract, so the generated schema stores
both ranges.

### `.ze_info` minor versions vary independently of the ABI

The checked-in C artifact carries `.ze_info` `1.70`; the locally tested C and
C++ pair carried `1.64`. Their relevant ABI facts matched. The parser accepts
reviewed major version 1 and records the minor version as provenance rather
than hard-coding one compiler release.

### SIMD selection changes the payload contract

Without `intel_reqd_sub_group_size(16)`, IGC selected SIMD32 and a 192-byte
per-thread payload in an early probe. The TRUEOS direct-RCS encoder currently
programs SIMD16 local IDs with 96 per-thread bytes, so the shared header makes
SIMD16 explicit and the generated contract rejects other widths.

### Hash enforcement exposed stale metadata

The Tile64 NV12 artifact was changed together with its source in commit
`0dfbca43`, but its catalog and README digest still described the preceding
artifact. The current committed binary is:

```text
f33f0f2f531aa4df74b932fd519d5c096f9576b94c09cf1e20b742151092e0b5
```

All-artifact admission turned that historical omission into a visible,
traceable correction instead of silently continuing with an unverified blob.

### Binary inspection must never mutate the input

One exploratory `llvm-objcopy --dump-section` invocation omitted an explicit
output object and rewrote its input ELF. The tracked legacy artifact was
immediately restored and hash-checked. Bakery and inspection tools operate on
temporary copies and treat published artifacts as immutable inputs.

## Toolchain used for the first proof

- Ubuntu Clang `21.1.8`
- `llvm-spirv` based on LLVM `21.0.0git`
- Intel IGC `libigc.so.2.30.0+0`
- the matching `ocloc` package
- target PCI device `0x4680`

The paths from the developer-machine proof are intentionally not encoded in
the repository. The bakery accepts explicit tool paths/environment variables
and records resolved versions and binary hashes in the output manifest.

## Tailoring points for later review

The first implementation intentionally leaves these decisions explicit:

- whether source/SPIR-V should remain embedded in production kernel images or
  move to a detached provenance bundle;
- how to sign and admit external artifact updates;
- whether revisions should remain a range or become exact stepping entries;
- when scratch and SLM programming become supported runtime capabilities;
- whether an RPL-S profile should be a separate artifact or a proven,
  documented compatibility set;
- how much generated argument metadata should replace the existing OpenCL
  registry declarations;
- when the C++ artifact becomes the default and the C fallback can be removed.
