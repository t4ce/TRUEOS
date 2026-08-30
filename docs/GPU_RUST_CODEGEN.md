# GPU-resident Rust-to-x86 compiler

Status: architecture spike; no runtime or security policy is changed by this
document.

## Decision

TRUEOS should pursue this as a sealed GPU compiler work graph with a measured,
incremental path to the full experiment.

The practical first architecture is:

```text
CPU / rustc                       Intel iGPU                         CPU

parse + expand + analyse          CompilerIR validation             validate ET_REL
monomorphise                      per-function lowering             linker
MIR -> flat CompilerIR   --->     register allocation      --->     Blueprint pack/launch
copy rust_code_buffer             x86 encode + relocations
submit one sealed graph           ELF64 ET_REL assembly
```

The moonshot architecture moves the left column onto the GPU too. That is a
new Rust compiler with rustc as its correctness oracle, not a build of the
existing rustc for Intel EUs. The existing rustc object graph is pointer-rich,
dynamically allocated, recursive, and depends on host Rust code and proc-macro
execution. It cannot be copied into GPU memory and traversed by a shader.

An unrestricted kernel or raw-Zebin backdoor is not part of the design. It is
not needed for a fast loop, and VMX isolation does not contain a shared iGPU
hang, GT reset, invalid DMA mapping, or display-engine disruption. Development
admission remains digest-, ABI-, resource-, principal-, and device-bound.

## Keep the three targets separate

There are three independent meanings of "target":

| Target | Initial value | Meaning |
| --- | --- | --- |
| Rust output target | `x86_64-unknown-trueos` | ABI and x86 code emitted for the Blueprint |
| BADC output target | `linux-x64` | BADC's SysV/ELF output; useful for a pilot, unrelated to the GPU profile |
| Compiler GPU target | exact PCI device + revision + artifact ABI | Device on which the compiler kernels execute |

A Tiger Lake `-g tgl` assembly/precompile check is useful for source and
integer-algorithm development, but it is not runtime proof for another GPU.
The current production C++ artifact contract is exact ADL-S `8086:4680`,
revision `0x0c`, SIMD16, with zero scratch and SLM. Tiger Lake is absent from
the current runtime target policy. The Rust target used by `rustc-min` is also
generic x86-64, not Tiger Lake-tuned x86.

Admission therefore requires the exact execution device/revision or a new,
reviewed target contract. Cross-generation arithmetic tests remain part of
the test suite, but never replace physical retirement and coherency evidence.

## What is already available

The packed toolchain is a strong oracle and integration host:

- `rustc-min` embeds the matching `rustc_driver_impl` and Cranelift for
  nightly `2026-07-10` / rustc commit `af3d95584`.
- Its normal request already uses `--emit=obj`, `panic=abort`, one codegen
  unit, and no parallel backend. The result is one x86-64 ELF relocatable
  object before the external linker.
- The driver exposes the normal `CodegenBackend::codegen_crate`,
  `join_codegen`, and `link` boundary.
- Cranelift's AOT driver collects deterministic monomorphized functions,
  lowers each MIR body to a Cranelift `Function`, and only then calls
  `module.define_function`, where target lowering, register allocation, and
  machine encoding occur. That call boundary is the smallest credible rustc
  GPU seam.

TRUEOS already supplies the other half:

- the offline C++ for OpenCL -> SPIR-V -> IGC Zebin bakery;
- generated, audited artifact ABI contracts and exact hash admission;
- SIMD16 scan, selection, reduction, radix-sort, histogram, RLE, and segmented
  operations in the GPU primitive incubator;
- `VVideoMem`, which is CPU-mapped in a VMX tenant and GPU-mapped through the
  owning isolated PPGTT while keeping GPU addresses opaque;
- compute queues, timelines, per-principal quotas, execution-lane quarantine,
  and known-artifact reload plumbing.

The missing pieces are a flat compiler wire format, a sealed compiler work
graph in vGPU, promoted primitive artifacts, x86 compiler kernels, and a safe
ephemeral development admission path.

## `rust_code_buffer`

`rust_code_buffer` is a `VVideoMem` allocation, not a pointer to rustc memory.
It is a versioned, offset-only arena shared by the producer and compiler work
graph. No record contains a host pointer, GPU virtual address, Rust enum
layout, or compiler-internal `usize`.

The initial header is conceptually:

```text
CompilerBufferHeader
  magic                 "TRGC"
  format_version        1
  state                 EMPTY | READY | RUNNING | COMPLETE | ERROR
  request_digest        sha256(canonical request bytes)
  compiler_set_digest   authenticated work-graph identity
  output_target         x86_64-unknown-trueos + feature bitmap
  gpu_target            PCI device + revision + ABI revision
  arena_bytes           total mapped extent
  function_table        offset, count, capacity
  block_table           offset, count, capacity
  value_table           offset, count, capacity
  instruction_columns   offsets, count, capacity
  constant_pool         offset, bytes, capacity
  symbol_table          offset, count, capacity
  text_output           offset, bytes, capacity
  relocation_output     offset, count, capacity
  diagnostic_output     offset, count, capacity
  temporary_arenas      declared offset/range per stage
```

Every range is checked for alignment, overflow, overlap, and containment
before dispatch. Counts never authorize memory beyond capacities. Kernels
write only to declared output and temporary ranges. A terminal error contains
the stage, function ID, record ID, and stable error code; source text is
diagnosed later by the CPU using retained span tables.

`CompilerIR-v1` should initially be a deliberately small, flat SSA/CLIF-like
language:

- fixed-width opcodes and operand columns;
- numeric function, block, value, constant, and symbol IDs;
- already-computed type sizes, alignments, calling conventions, and layouts;
- explicit CFG successor ranges and phi/block parameters;
- explicit linkage, visibility, and relocation targets;
- no raw MIR allocations, `TyCtxt`, host references, or recursive structures.

The present 32 MiB vGPU guest quota is enough for the first proof, not an
entire rustc session. The protocol therefore supports deterministic function
batches. Raising the quota is a measured later decision, not a prerequisite.

## The resident compiler army

The "army of shader files" is built once and kept resident. Generating or
compiling shaders for every Rust input would move compiler latency into IGC
and defeat the experiment. The runtime intentionally has no source compiler.

The proposed sealed `CompilerWorkGraph` contains bounded stages such as:

1. request/range validation and record classification;
2. liveness seeds and dense bitset propagation for the admitted IR subset;
3. instruction selection and operand-form classification;
4. per-function register allocation;
5. instruction length calculation;
6. exclusive scan of function and instruction output sizes;
7. x86 encode/scatter;
8. relocation select/compact and stable radix sort;
9. symbol/section sizing and deterministic ELF layout;
10. ELF64 ET_REL emission and final structural checksum.

The unit of parallelism is a function or a large flat record batch, not one
shader per source file. With 32 EUs, the useful shape is thousands of
independent small functions. A single large branch-heavy function will not
magically become SIMD-friendly; lane 0 may perform its serial portions while
other workgroups process other functions.

For x86 v1, prefer deterministic and simple choices:

- a restricted instruction-selection table;
- linear-scan allocation before graph colouring;
- conservative long branch forms before optional relaxation;
- explicit external/data relocation tuples;
- `panic=abort`, no debug info, no unwinding, no inline assembly;
- no unsupported instruction silently falling back inside a GPU result.

Unsupported functions return a stable status and are compiled by the CPU
oracle in hybrid mode. The moonshot mode instead rejects the crate.

## Two compiler tracks

### Track A: useful hybrid compiler

Rustc remains responsible for the semantics that it is already exceptionally
good at: parsing, macro expansion and hygiene, name resolution, type and trait
solving, borrow checking, layout, ABI classification, monomorphisation, and
MIR-to-flat-IR translation.

The first hook belongs after a per-function Cranelift `Function` has been
built and before `module.define_function`. The AOT driver already batches the
functions before this loop. A canonical serializer turns those functions and
their symbol/configuration tables into `CompilerIR-v1`; the GPU returns code
fragments and relocations. CPU Cranelift remains a per-function oracle and
fallback until the supported surface is complete.

This reaches the expensive target-specific work without inventing a second
Rust frontend. It can become a production feature if it wins benchmarks and
passes the full differential gates.

### Track B: literal GPU-only Rust compiler

To honour "the CPU only loaded the buffer and submitted it", start with a
named Rust subset rather than claiming full Rust:

```text
source bytes
  -> parallel UTF-8/byte classification
  -> token-boundary flags + scan + compaction
  -> delimiter structure
  -> table-driven restricted parser
  -> fixed-arena symbols and monomorphic type constraints
  -> SSA
  -> the same x86/ELF work graph used by Track A
```

The first subset should contain free functions, `i32`, `u32`, `i64`, `u64`,
`bool`, locals, arithmetic, comparisons, `if`, loops, direct calls, and fixed
arrays. Initially exclude macros, proc macros, generics, traits, closures,
lifetimes/borrow checking, heap allocation, unwinding, floats, inline assembly,
build scripts, and FFI beyond a small declared import table.

Full Rust requires GPU-native implementations for macro hygiene, arbitrary
proc-macro execution, query invalidation, trait solving, inference, borrow
checking, layout, monomorphisation, diagnostics, and many fixed-point graph
passes. That is a long-lived compiler project. The packed rustc is the golden
semantic oracle and corpus generator throughout it.

## BADC should be the stage-zero x86 laboratory

BADC is a smaller and cleaner place to prove the GPU machine-code half before
coupling it to rustc-private data. Its `--target linux-x64` path already has a
flat-ish SSA pipeline, liveness and register-allocation passes, a table-driven
x86-64 encoder, ELF relocation support, an SSA interpreter, and extensive
cross-target tests.

The exact first seam is after `produce_ssa_funcs` and
`ssa::reg_alloc::allocate`, immediately before the per-function
`x86_64::emit_function` call made by `x86_64::encode::lower`. The packed input
is the admitted subset of `FunctionSsa` plus its `Allocation`/`Place` records.
One workgroup writes each function's bytes, internal block fixups, PC map, and
normalized relocation records. CPU code retains global fixups and
`object::elf_reloc::write_relocatable`.

This order intentionally leaves register allocation on the CPU first. It
separates encoder bugs from allocation bugs; deterministic linear scan moves
only after byte emission is proven. BADC's CPU implementation and SSA
interpreter provide two independent oracles. Once the buffer ABI and x86
emitter survive this corpus, the rustc adapter can feed the same machine-op
layer.

`linux-x64` here selects BADC's emitted ABI and ELF format. It says nothing
about whether the compiler shader is valid for Tiger Lake, ADL-S, or RPL-S.
Linux executables and Linux ET_REL files are not automatically TRUEOS
Blueprints. The pilot proves SSA -> x86 bytes and relocations. A later TRUEOS
adapter must deliberately produce the admitted ET_REL/import/entry contract,
then pass the existing Blueprint validator and packer; no generated `.o` is
directly kernel-loadable merely because it is ELF.

## Object and linker boundary

The requested boundary already exists: rustc-min requests `--emit=obj`, and
the driver returns after codegen when no executable or metadata output is
requested. Its result is checked as ELF64, little-endian, x86-64, relocatable,
then wrapped into a Blueprint payload.

Development can use two object stages:

1. GPU returns `.text`, symbols, and relocation tuples; the existing CPU
   object writer constructs ET_REL. This isolates x86 compiler correctness.
2. GPU additionally performs deterministic section sizing, prefix layout,
   string/symbol ordering, and ET_REL emission. CPU only performs strict
   structural and policy validation before the linker.

Stage 2 is the stated end goal. Stage 1 is the fastest way to determine
whether GPU x86 generation is correct and faster before debugging two new
subsystems at once.

The validator is deliberately retained even in the end state. It checks ELF
bounds, architecture, object kind, sections, symbol IDs, relocation kinds and
addends, executable ranges, imports, and output digest. Validation is a small
trust-boundary operation, not a second code generator.

## Reboot-free development loop

Blueprint application iteration already has a no-reboot path:

```text
host: cargo bp <app>
TRUEOS: status
TRUEOS: stop <vmid>        # only when replacing a running instance
TRUEOS: §§<app>             # verified online fetch + direct VMX launch
```

`cargo bp` performs the CABI guard and publishes a hash-qualified package.
`§§<app>` is the existing verified fetch-and-launch operator. `dl <app>` then
`start <app>` is the install-then-launch variant. None of these require a
kernel reboot.

TRUEOSFS HTTP upload is storage only. Uploading a root `.bp` does not insert it
into `app.db` and does not make `start` discover it. That separation must not
be silently removed: the HTTP service is reachable on usable NICs and its
upload route is not an authenticated code-execution authority.

There are two distinct future development conveniences:

### Blueprint inbox importer

A non-default development build may expose an explicit import action that:

- reads only one normalized `.bp` beneath a fixed local dev inbox;
- caps it at the existing 64 MiB application-fetch limit;
- requires an operator-supplied SHA-256;
- fully parses, decompresses, checks version/trailing bytes, validates x86-64
  ET_REL, imports, relocations, and prebind readiness;
- atomically replaces the volatile `app.db` row only after every check;
- records root, path, digest, and source in the audit log;
- leaves launch as a separate ordinary `start` action.

This is proposed, not an existing Shell2 command. The release build must not
contain the importer.

### GPU compiler artifact registration

Known-artifact reload currently reloads only catalogued, exact allowed bytes.
Fast compiler-kernel iteration needs a boot-scoped registration capability,
not a global bypass. A one-shot allowance is bound to:

```text
boot nonce
operator/principal
artifact sha256
kernel-set/entry names
exact PCI device and revision
generated ABI manifest
declared input/output/temp ranges
maximum scratch, SLM, walkers, duration, and use count
expiry
```

Registration never grants MMIO, GGTT, physical addresses, page-table writes,
kernel mappings, or arbitrary queue submission. The artifact executes only in
the owning isolated PPGTT, can access only the declared compiler buffers, and
runs on a quarantinable execution lane with a deadline. A fault or timeout
invalidates the allowance and quarantines/resets that context. Production
builds omit the registration capability entirely.

The uploaded file itself grants no authority. An HTTP marker file is not an
authorization mechanism because the same unauthenticated route could create
it.

## Staged implementation

### Stage 0: baseline and oracle

- Measure a warm rustc-min compile by parse, analysis, CLIF construction,
  target codegen, object write, Blueprint pack, and launch phases.
- Exclude one-time Blueprint boot and one-time offline shader baking from warm
  compiler latency, but report them separately.
- Preserve every CPU object and normalized result as a differential oracle.

### Stage 1: BADC emitter proof

- Define and fuzz the offset-only `CompilerIR-v1` parser on CPU.
- Batch BADC `linux-x64` functions into `rust_code_buffer`.
- Implement classify -> size -> scan -> encode/scatter -> relocation
  compact/sort for a small integer instruction surface.
- Keep CPU register allocation first; move deterministic linear scan only
  after byte emission is proven.

### Stage 2: sealed TRUEOS work graph

- Promote only the required scan/select/radix primitives through their normal
  artifact and physical-hardware gates.
- Add a named `CompilerWorkGraph` vGPU profile with fixed buffer roles and a
  full authenticated kernel-set digest.
- Add timing, byte-count, submission-count, fault, timeout, and quarantine
  telemetry.

### Stage 3: rustc hybrid

- Add a canonical Cranelift-IR-to-`CompilerIR-v1` adapter at the
  pre-`module.define_function` seam.
- Start with arithmetic-only leaf functions and CPU fallback per unsupported
  function.
- Expand calls, loads/stores, branches, ABI forms, statics, relocations, and
  intrinsics only behind differential tests.
- Move deterministic ELF ET_REL construction to the GPU after the fragment
  interface is stable.

### Stage 4: GPU-only subset

- Add source tokenization and the restricted parser/type/SSA stages.
- Compile an increasing, versioned Rust subset with no CPU semantic fallback.
- Use rustc-min to compile the same source corpus and compare observable
  results.

### Stage 5: expand only with evidence

GPU implementations of macro expansion, type/trait solving, borrow checking,
and more complex optimisation follow only when the previous stage wins its
correctness and performance gates. A stage may remain hybrid permanently if
the CPU algorithm is faster or substantially simpler.

## Acceptance gates

Correctness gates:

- canonical request validation fails closed for every malformed range/count;
- deterministic output digest across repeated runs and scheduling variation;
- differential compilation against BADC and Cranelift CPU oracles;
- random arithmetic/branch/memory/ABI corpus plus real Blueprint functions;
- valid ELF64 x86-64 ET_REL with only admitted relocations and imports;
- execution result matches the CPU oracle in VMX;
- no unsupported opcode, feature, or relocation is approximated;
- exact-device physical retirement with zero unexpected GPU faults.

Performance report:

- warm median and p95 wall time;
- CPU-active and GPU-active nanoseconds;
- source/IR/result bytes and cache-maintenance bytes;
- command submissions and timeline waits;
- function/op count at which GPU becomes faster than CPU;
- energy where counters are trustworthy;
- small-input regression and fallback overhead.

The first measurable kernel should accept 1k, 10k, and 100k fixed-size
machine-op records across independent functions and return byte-exact x86
text, function offsets, symbols, and RELA records. It must fit the current
32 MiB quota, SIMD16, zero scratch/SLM/atomics profile. "The GPU ran" is not a
success criterion; correctness plus a reported break-even point is.

## Decisions still requiring discussion

1. Is the first user-visible goal a fast rustc hybrid, or the deliberately
   pure GPU subset compiler? The buffer and emitter can serve both, but the
   scheduling differs.
2. Which exact physical GPU is the development/production contract: current
   ADL-S `0x4680 r0c`, a new RPL-S/UHD770 profile, or a separately supported
   Tiger Lake machine?
3. Should v0 stop at machine-code fragments, or include GPU-written ET_REL?
   Fragments produce a faster correctness signal; ET_REL is the end-state.
4. Is BADC `linux-x64` accepted as the stage-zero oracle before rustc
   integration?
5. Which minimal x86/ABI surface forms the v1 executable corpus?
6. Is a non-release Blueprint inbox importer worth adding, or is the existing
   `cargo bp` + `§§app` cycle already fast enough?
7. Do compiler batches stay within 32 MiB, or is a larger per-principal vGPU
   quota justified after measurements?

## Grounding in the current tree

- `tools/intel-gpu-bakery/README.md`
- `tools/intel-gpu-primitives/README.md`
- `src/intel/gpgpu/artifacts/runtime.rs`
- `src/intel/gpgpu/artifacts/uploads.rs`
- `src/intel/gpgpu/runtime_state.rs`
- `src/gpu/vgpu.rs`
- `crates/trueos-v/src/vgpu.rs`
- `src/r/fs/http_trueosfs.rs`
- `src/shell2/shell2_dl.rs`
- sibling toolchain `TRUEOS_PORT.md`
- sibling toolchain `blueprints/rustc-min/src/main.rs`
- sibling toolchain rustc sources in `rustc_codegen_cranelift` and
  `rustc_codegen_ssa`
- sibling Blueprints app `apps/badc/src/c5/codegen/`
- sibling Blueprints app `apps/badc/src/c5/object/elf.rs`
