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

## Milestones

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
- exact implicit global-ID-offset, local-size, and enqueued-local-size records;
- exact per-thread local-ID record;
- binding-table indices;
- argument payload offsets, sizes, access, and address mode;
- Zebin and SPIR-V SHA-256 values;
- exact target PCI IDs and revision range.

The JSON additionally retains the complete `.ze_info` execution environment,
source argument names, type spellings, qualifiers, tool versions, normalized
commands, the reviewed publication policy, and hashes for the source plus
transitive quoted headers. The lock also binds the resolved Clang/LLVM
implementation libraries, the complete Clang resource tree, and ocloc/IGC
compiler resources.

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

Strict reload never falls back to embedded bytes when the filesystem artifact
is missing, empty, unreadable, or cannot be read safely from the current
executor context. A repeated exact reload performs full admission and reuses
the resident allocation; it does not allocate/remap the same GPU VA and leak
the prior DMA object. Replacing a resident image needs a future quiescent
retirement protocol.

The first C++ selection is exact to the physical TestRig identity: ADL-S device
`0x4680`, revision `0x0c`, at BDF `00:02.0`. Its dedicated
`adls-4680-r0c-cpp.json` profile is compiled into the generated contract, so
the C++ artifact is rejected on every other revision even if the PCI device ID
matches. The shared legacy ADL-S target remains unchanged and revision-broad;
this narrowing applies only when `intel_gpu_cpp_aot` selects the generated C++
contract. ADL-N, Raptor Lake, and other ADL-S steppings require their own
reviewed profile and checked artifact instead of inheriting compatibility by
family resemblance.

### 3. First selection and hardware conformance

`copy_rect_rgba8.clcpp` is the first source-side twin. It retains the existing
entry symbol and argument order and uses a tiny freestanding header to prove
that `constexpr`, namespaces, and templates remain compile-time facilities.

The C++ pair lives under `kernels/artifacts/adls/cpp/` and is selected by
default in the normal Make product/development lane through the
`intel_gpu_cpp_aot` Cargo feature. Direct Cargo builds remain legacy unless
the feature is requested. Both frontends feed the same Rust payload,
surface-state, interface-descriptor, walker, and GuC code.

The host actions are deliberately separate:

```sh
# Uses the pinned host compiler stack and republishes only after a double bake.
make intel-gpu-bake-copy-cpp

# Standard-library-only verification of the checked-in artifact and contract.
make intel-gpu-verify-copy-cpp

# Verifies first, builds TRUEOS with C++ selected, and scans the linked ELF.
make kernel

# Carries the same feature through the complete bootable ISO workflow, emits
# canonical bld/trueos.iso, and verifies its extracted stripped runtime ELF.
make iso

# Isolated retained OpenCL C comparison/fallback.
make iso-legacy-opencl-c
```

The OpenCL-shaped bridge reports this boundary truthfully:
`known_source_aot_lookup=true`, because an exact known source string can select
an already baked program, and `source_compile=false`, because no compiler is
linked into or loaded by TRUEOS.

`kernel` scans the final linked ELF for the complete selected Zebin and
requires the complete alternate copy Zebin to be absent. `iso` records the
normalized selection and effective Cargo flags in `BUILD_INFO`, extracts
`/TRUEOS.elf` from the completed ISO, proves it is byte-identical to the
stripped runtime ELF, and repeats that scan. This guards the deployment
boundary itself rather than inferring feature selection only from source-level
`cfg` declarations. `kernel-cpp-aot` and `iso-cpp-aot` remain compatibility
aliases; `INTEL_GPU_CPP_AOT=0` and `iso-legacy-opencl-c` provide the explicit
fallback.

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

Build with `make iso`, boot `bld/trueos.iso` on the exact TestRig
(`00:02.0`, `8086:4680`, revision `0x0c`), and run:

```text
gpgpu probe copy-rect
```

Save the complete summary and four case lines, then verify the transcript on
the development host:

```sh
make intel-gpu-verify-copy-cpp-hardware-log \
  INTEL_GPU_CPP_PROBE_LOG=/path/to/copy-rect-probe.log
```

The verifier rejects missing, duplicated, reordered, or contradictory output.
It pins the BDF, PCI ID, revision, C++ feature/frontend, artifact identity and
hash, all four case geometries and readback counts, both retirement markers,
and the 250 ms submission timeout.

Hardware conformance requires the summary to contain
`ok=1 reboot_required=0 frontend=cpp-for-opencl feature_enabled=1 verified=1
device=00:02.0-0x4680-r0C
hash=b36d1c7742003591a5074663d81a4162412618ae425c47d30be6d068ee144a25
cases=4/4 retired=4 passed=4 first_failure=none`; every case must report
`submitted=1 retired=1 ok=1` and markers
`[0xC0DEA701,0xC0DEA702]`. If a submitted case does not retire, do not run the
probe again. The lane is quarantined and reports `reboot_required=1`; recover
the engine or reboot the machine first.

### 4. Native C++ application kernel and Shell2 surface

`cpp_demo_rgba8.clcpp` is the first C++ publication that is not a legacy ABI
twin. The bakery labels it `variant=cpp-native` under reviewed policy
`cpp-native-aot-v1`: an artificial legacy reference is forbidden, while the
toolchain lock, complete input hashes, exact kernel set, `ocloc validate`,
sibling/embedded SPIR-V identity, generated contract, and two-root
reproducibility gates remain required.

One stable SIMD16 entry exposes six scalar-selected Shell2/UI4 modes:
`gallery`, `aurora`, `julia`, `sdf`, `voronoi`, and `retro-sun`. The dedicated
advertised F4 command is `cpp`; `cpp` starts the four-panel gallery, direct mode
names start full-window workloads, and `cpp status`/`cpp stop` use the existing
on-demand preview service. Retro Sun remains a standalone composition rather
than becoming a fifth gallery panel. The kernel is uploaded once, while time,
mode, seed, and destination facts are ordinary launch payload values.

The runtime uses the same serialized direct-RCS/GuC service lane and exact
surface-release boundary as the established UI4 compute producers. A dispatch
must retire marker `0xC0DEC902` before UI4 receives the release token. Accepted
timeouts quarantine the context and retain the write lease. C++/Clang/IGC
remain absent at runtime.

The native demo contract is exact to `8086:4680` revision `0x0c`, SIMD16,
128 GRFs, zero scratch/SLM, 128 cross-thread bytes, 96 local-ID bytes, and one
read/write stateful BTI. The Zebin hash is:

```text
75e5a83b3e74e3b5da59756bc5a804cbb742314389bb60559474586050ce66ac
```

`make kernel` and `make iso` now require this complete native Zebin in the
linked and ISO-extracted runtime ELF independently of which copy frontend the
comparison lane selects. Detailed commands and TestRig coverage live in
`crates/trueos-shader/gpgpu/kernels/CPP_DEMO_SUITE.md`.

### 5. Spirit becomes an ABI-preserving C++ visual suite

The next application step reuses Spirit's already-proven cursor-plane
architecture instead of introducing another presentation path.
`spirit_vfx_background_rgba8.clcpp` and `spirit_vfx_sprite_rgba8.clcpp` compile
the retained compositions as exact ABI twins, then enable C++-only templated
secondary-detail layers. All stable effect IDs, the original control layout
plus append-only clock dword 32,
binding counts, payload sizes, one/two-walker selection, ordered dependency,
GuC post-sync release, and `CUR_SURFLIVE` proof remain unchanged.

The C++ background is 109,624 bytes with SHA-256
`4c82592a441b88902491b6d3cf8f99a3524a41178a347928cf5845e0872841f7`.
The C++ sprite is 656,728 bytes with SHA-256
`2ee466aa00e631119e8de1eb9fa2d53a1b39d46cc56b4ce2e16ff18f653343ac`.
Both are exact to `8086:4680` revision `0x0c`, SIMD16, 128 GRFs, and zero
scratch/SLM.

Their larger instruction images required moving the sprite mapping from
`0x0D440000` to `0x0D450000` and the independent demo mapping to
`0x0D600000`; the background remains at `0x0D430000`. The two-walker sprite
entry is consequently relative `0x20040`. Compile-time constants and runtime
hash/byte checks bind these layout facts.

The host offline tools can now consume the published SPIR-V directly through
`clCreateProgramWithIL`, allowing reference/C++ PNG comparison on the actual
UHD 770. Shell2 adds `cpp spirit`: all `9 × 16` combinations can be selected
with authored defaults and palettes while the live Spirit worker continues to
own submission and presentation. Detailed visual and TestRig commands live in
`crates/trueos-shader/gpgpu/kernels/SPIRIT_CPP_REPASS.md`.

### 6. One live-audio kernel becomes a resizable UI4 instrument

`cpp_audio_visualizer_rgba8.clcpp` deliberately moves away from a grid of
small variants. One C++/IGC entry composes waveform ribbons, a stereo
phase-scope field, logarithmic spectrum architecture, circular waveform
prism, bass bloom, onset rings, and sparse high-frequency particles.

The source signal is not a second playback client. An allocation-free atomic
ring observes the exact interleaved 48 kHz stereo s16 slice after software
mix/limit and immediately before the existing HDA DMA copy. The speaker
samples are unchanged. The UI4 producer performs one ordinary 2048-point
mid/side FFT outside the HDA callback and uploads a bounded 4096-byte snapshot
containing 128 samples per channel, 64 bands, and twelve features.

PrismQ/QFT was rejected for this streaming lane: a 16-qubit state represents
65,536 amplitudes, adds about 1.37 seconds of 48 kHz input span, and offers no
advantage for a classical PCM spectrum. The existing Symphonia radix-2 FFT is
smaller, deterministic, and already part of the kernel build.

The render remains one GPU dispatch. Each x lane writes two adjacent pixels,
so a maximized 2560x1440 window launches 1,843,200 lanes—exactly half the
ordinary per-pixel walker count. The default cadence is 50 ms rather than a
stress-test refresh target. C++ preview windows now use UI4 application
interaction; maximize and restore notifications replace the double-buffered
Frame at the actual extent and the kernel derives aspect from every launch.

The final exact-r0C artifact is 77,592 bytes, SIMD16, 128 GRFs, zero
scratch/SLM, with SHA-256
`951e0cb30b42a755812b00eb0c3871f52c765ee74295dc3cb48b84f8361c1b19`.
It is required by linked and packaged ELF verification. Shell2 exposes
`cpp audio`, `cpp status`, and `cpp stop`; detailed ABI and physical promotion
steps live in
`crates/trueos-shader/gpgpu/kernels/CPP_AUDIO_VISUALIZER.md`.

## Findings that changed the implementation

### General math can expand the loader contract

The first C++ demo bake used ordinary `sin`, `cos`, `exp`, `log`, and `pow`.
IGC emitted a valid main kernel, but also added an internal callable support
entry, symbol table, global constants, and constant/private base payloads.
Those records describe a wider runtime linker/global-data contract than
TRUEOS direct-RCS currently supports, so the bakery correctly rejected the
unexpected kernel set instead of weakening inspection.

The demo now uses OpenCL `native_*` operations plus explicit vector helpers.
The resulting reproducible Zebin contains exactly `cpp_demo_rgba8` and needs
no auxiliary global-data loader. This is a reviewed source constraint; future
support for general math should deliberately add and test those loader
capabilities rather than silently allowlist the compiler helper.

### The canonical Make lane must carry the selection

The first physical TestRig transcript passed all four direct-RCS copy cases
with `reboot_required=0`, but reported `frontend=opencl-c` and
`feature_enabled=0`. The operator had correctly booted the newest ISO by
timestamp: a later ordinary `bld/trueos.iso` build had superseded the separate
`bld/trueos-cpp-aot.iso` test image. The result is useful legacy hardware
baseline evidence, but its wrapped/truncated transport is not accepted by the
strict C++ transcript verifier.

The normal Make product/development lane therefore owns the staged selection:
`INTEL_GPU_CPP_AOT` defaults to `1`, its effective Cargo flags are recorded
directly in `BUILD_INFO`, and `make kernel`/`make iso` prove the selected
artifact at the linked and packaged boundaries. `make iso` again produces the
single canonical `bld/trueos.iso` consumed by the existing QEMU, provenance,
release, and physical-development tooling. The legacy comparison uses
`INTEL_GPU_CPP_AOT=0` or `make iso-legacy-opencl-c` and isolated output paths.

The subsequent canonical-ISO run on the same TestRig reported
`ok=1 reboot_required=0 frontend=cpp-for-opencl feature_enabled=1`. Each of
`even-small`, `odd-small`, `even-multigroup`, and `odd-multigroup` reported
`attempted=1 submitted=1 retired=1 ok=1`. This is the physical functional pass:
the C++ frontend artifact was admitted, submitted through the direct-RCS/GuC
path, retired, and matched the CPU oracle across all four geometries. The
terminal copy was again wrapped and truncated, so it does not preserve the
complete hash/device/marker/retirement fields required by
`verify_probe_log.py`; an archival machine-verifiable transcript remains a
separate evidence-quality task, not a functional failure.

Cargo's global default feature set was deliberately not changed. This keeps
direct Cargo behavior and its unrelated default features stable, avoids a
brittle `--no-default-features` reconstruction for the fallback, and bounds
the exact-r0C policy to the product lane being exercised. The compiler bakery
also remains offline: ordinary builds run its standard-library-only verifier,
not Clang, `llvm-spirv`, ocloc, or IGC.

### Revision exactness is runtime admission policy

The tested `ocloc` interface accepts `-device 0x4680` but no revision argument
from this profile. Consequently, changing the reviewed range from
`0x00..0xff` to exact revision `0x0c` must not change LLVM bitcode, SPIR-V, or
Zebin bytes. The dedicated C++ profile instead narrows the generated manifest
and the Rust admission contract. The canonical two-root rebake confirmed the
same SPIR-V and Zebin hashes; any code-byte drift during this policy-only
change would have been treated as an unexpected compiler-input change.

The physical transcript additionally pins BDF `00:02.0`. BDF is a TestRig
identity check, while runtime artifact admission intentionally uses PCI device
ID plus revision so slot enumeration is not confused with binary
compatibility.

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

Resolved compiler locations are execution details, not artifact identity. The
published manifest and reviewed toolchain lock retain executable/library
hashes and versions but omit absolute host paths, so an equivalent installation
in a different directory does not dirty the generated provenance. The initial
exploration note keeps its developer-machine paths only as historical context.

### Compiler identity includes resources and a sanitized environment

The Clang executable is only a small driver on the proof host. Its frontend
implementation lives in `libclang-cpp`/`libLLVM`, and the canonical OpenCL
compile implicitly reads the resource header
`lib/clang/21/include/opencl-c-base.h` even though `-MMD` omits it from the
quoted-header depfile. Executable hashing alone was therefore insufficient.
The lock now records path-independent hashes for resolved compiler libraries
and a deterministic relative-path/content digest of the complete Clang
resource tree.

Clang also honors ambient inputs such as `CCC_OVERRIDE_OPTIONS`, `CPATH`, and
`COMPILER_PATH`. The bakery constructs a minimal environment rather than
copying the caller environment; only the modeled ocloc/IGC library roots,
locale, system command path, UTC, and `SOURCE_DATE_EPOCH=0` survive. The
profile adds `-Wdate-time`, so source time macros cannot evade a same-host
two-root reproducibility check.

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

### Completion markers must prove ordered retirement

The first copy probe used a cache flush followed by `MI_STORE_DATA_IMM`.
That store could show command-stream progress without being the established
dataport/cache-release proof. Copy completion now uses the shared ordered
PIPE_CONTROL post-sync epilogue. Its QWord occupies result slots 4–5 and the
diagnostic pre-marker moved to slot 6, with compile-time overlap/alignment
assertions.

A software submission timeout cannot cancel a physical GuC request. Before
completion bookkeeping releases the submit lock, TRUEOS therefore quarantines
the affected persistent system-service or execution lane. State access then
fails before batch/result/PPGTT/scratch reuse. This prevents TRUEOS from
rewriting memory a late request may still fetch, but cannot cancel that
request; engine recovery or reboot remains mandatory.

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
- target BDF `00:02.0`, PCI device `0x4680`, revision `0x0c`

Developer-machine paths do not participate in the generated manifest or lock
identity. The bakery accepts explicit tool paths/environment variables and
records versions and binary hashes in the output manifest.

## Tailoring points for later review

The first implementation intentionally leaves these decisions explicit:

- whether source/SPIR-V should remain embedded in production kernel images or
  move to a detached provenance bundle;
- how to sign and admit external artifact updates;
- how a later stepping earns its own exact profile and hardware evidence;
- whether the bakery should move from a reviewed minimal environment to a
  fully containerized/hermetic compiler image;
- when scratch and SLM programming become supported runtime capabilities;
- whether an RPL-S profile should be a separate artifact or a proven,
  documented compatibility set;
- how much generated argument metadata should replace the existing OpenCL
  registry declarations;
- when the exact-r0C Make default can broaden to other devices or the retained
  C fallback can be removed.
