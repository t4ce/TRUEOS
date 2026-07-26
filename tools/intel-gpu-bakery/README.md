# TRUEOS Intel GPU artifact bakery

The compiler bakery is an **opt-in host tool**. Run `make cpp` to refresh every
C++ artifact and perform the publication audit. Normal `make iso`, `make run`,
and kernel builds consume the checked-in artifacts and run only the
compiler-free verifier. TRUEOS embeds the resulting SPIR-V, Intel Zebin, and
compact generated Rust contract; Clang, `llvm-spirv`, `ocloc`, IGC, and C++ are
absent at runtime.

The C++ path is:

```text
C++ for OpenCL
  -> Clang spir64 LLVM bitcode
  -> llvm-spirv OpenCL Kernel SPIR-V
  -> ocloc/IGC Intel Zebin
  -> parsed ELF + .ze_info JSON audit manifest
  -> generated no_std Rust ABI contract
```

Direct Clang `--target=spirv64` output is deliberately not accepted for
publishing. On the tested Clang 21 toolchain it loses OpenCL kernel argument
type/access metadata. The LLVM bitcode translator path preserves it.

## Pinned copy-rectangle proof

Set the host tools if they are not on `PATH`:

```sh
export CLANG=/path/to/clang
export LLVM_SPIRV=/path/to/llvm-spirv
export OCLOC=/path/to/ocloc
export OCLOC_LD_LIBRARY_PATH=/path/to/ocloc/lib:/path/to/igc/lib
```

Then run:

```sh
tools/intel-gpu-bakery/bake_adls_cpp_copy_rect.sh
```

The wrapper:

- selects the dedicated `8086:4680` revision `0x0c` C++ proof profile without
  narrowing the shared legacy ADL-S profile;
- checks executable versions and SHA-256 values, resolved dynamic compiler
  libraries, the complete Clang resource-tree digest, and ocloc/IGC resources
  against `toolchains/adls-cpp-proof.lock.json`;
- compiles twice in distinct output roots and requires byte-identical
  bitcode, SPIR-V, and Zebin;
- captures the source and every quoted header from Clang's depfile;
- strips ambient compiler override/include/library variables, pins
  `SOURCE_DATE_EPOCH=0`, and rejects date/time macros through `-Wdate-time`;
- requires the generated C++ artifact ABI to exactly match the checked-in
  OpenCL C copy-rectangle Zebin;
- requires the sibling SPIR-V to be byte-identical to the `.spv` section
  embedded by IGC in the Zebin;
- invokes every `ocloc` command from the ignored build tree so query side
  files cannot leak into the repository root;
- publishes to `kernels/artifacts/adls/cpp/`, leaving legacy artifacts
  untouched.

Toolchain updates are intentional review events. After reviewing changed
compiler output and metadata, a maintainer can generate a candidate lock with:

```sh
python3 -B tools/intel-gpu-bakery/bake.py \
  --source crates/trueos-shader/gpgpu/kernels/copy_rect_rgba8.clcpp \
  --artifact-name copy_rect_rgba8 \
  --profile tools/intel-gpu-bakery/profiles/adls-4680-r0c-cpp.json \
  --variant cpp \
  --abi-reference-bin crates/trueos-shader/gpgpu/kernels/artifacts/adls/copy_rect_rgba8.bin \
  --expect-kernel copy_rect_rgba8 \
  --repro-check \
  --write-toolchain-lock /tmp/adls-cpp-proof.lock.json
```

Review both the candidate lock and generated artifact/manifest before
replacing the repository lock.

## Native C++ demo publication

The copy proof uses `variant=cpp` and must be an exact ABI twin of a reviewed
legacy artifact. New C++ application kernels use the separate
`variant=cpp-native` / `cpp-native-aot-v1` policy: there is no artificial
legacy ABI reference, but all toolchain-lock, dependency, exact-kernel-set,
two-root reproducibility, SPIR-V identity, parser, and `ocloc validate` gates
remain mandatory.

The canonical native demo bake is:

```sh
tools/intel-gpu-bakery/bake_adls_cpp_demo.sh
# or
make intel-gpu-bake-cpp-demo
```

It publishes `cpp_demo_rgba8.{bin,spv,manifest.json,contract.rs}` beside the
copy artifact. The normal compiler-free check audits both:

```sh
make intel-gpu-verify-cpp-artifacts
```

The source and runtime workload map are documented in
`crates/trueos-shader/gpgpu/kernels/CPP_DEMO_SUITE.md`.

## Native C++ audiovisual publication

The live PCM instrument is a second `cpp-native-aot-v1` publication with one
expected entry and no legacy ABI reference:

```sh
tools/intel-gpu-bakery/bake_adls_cpp_audio_visualizer.sh
# or
make intel-gpu-bake-audio-visualizer-cpp
```

It publishes
`cpp_audio_visualizer_rgba8.{bin,spv,manifest.json,contract.rs}`. The standard
compiler-free verifier reparses and regenerates its complete two-BTI,
eight-argument contract with the other C++ artifacts. Linked and packaged ELF
verification requires this complete Zebin independently of the selected copy
frontend.

The PCM boundary, snapshot layout, half-width walker, UI4 resize lifecycle,
and TestRig procedure are documented in
`crates/trueos-shader/gpgpu/kernels/CPP_AUDIO_VISUALIZER.md`.

## Stateful ParticleCraft publication

ParticleCraft publishes three entries in one reproducible exact-target artifact:

```sh
tools/intel-gpu-bakery/bake_adls_cpp_particle_craft.sh
# or
make intel-gpu-bake-particle-craft-cpp
```

The generated `particle_craft.contract.rs` records independent step, tile-bin,
and render entry contracts while all three refer to the same Zebin/SPIR-V
digests. Runtime consumes the bytes only; the C++ toolchain remains a build-time
dependency.

## Spirit native C++ suite

Spirit publishes both of its existing kernel ABIs through C++ for OpenCL:

```sh
tools/intel-gpu-bakery/bake_adls_cpp_spirit.sh
# or
make intel-gpu-bake-spirit-cpp
```

The paired bake compiles twice, checks the reviewed toolchain lock, validates
the exact expected kernel set, and publishes both artifacts under the
standalone `cpp-native-aot-v1` policy. The runtime selects both C++ images
unconditionally; no retained OpenCL C artifact participates in publication.
`make kernel` and `make iso` require both complete Spirit C++ Zebins in the
linked and packaged ELF alongside the native C++ demo.

The visual and physical review procedure is documented in
`crates/trueos-shader/gpgpu/kernels/SPIRIT_CPP_REPASS.md`.

## Compiler-free checks

CI and ordinary development machines do not need compiler tools:

```sh
python3 -B tools/intel-gpu-bakery/verify.py \
  --artifact-dir crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp

python3 -B -m unittest discover \
  -s tools/intel-gpu-bakery -p 'test_*.py'
```

Verification reparses ELF64, the symbol table, and `.ze_info`; checks the
Zebin/embedded/sibling SPIR-V identity, profile, reviewed toolchain lock,
two-root reproducibility result, ABI-reference, publication policy, source,
and transitive-header hashes; and regenerates the Rust contract in memory. It
needs only the Python standard library.

The normal `make kernel` lane selects the C++ copy implementation and proves
that the final TRUEOS ELF contains its complete Zebin, no complete copy of the
legacy copy Zebin, plus the independently required native C++ demo,
audiovisual instrument, and both Spirit C++ Zebins. The
compatibility target remains available:

```sh
make intel-gpu-verify-linked-copy-cpp
```

`make iso` goes one boundary further. It creates the canonical
`bld/trueos.iso`, extracts `/TRUEOS.elf` back from that ISO, requires byte
identity with the stripped/staged runtime ELF, and applies the same
selected-present/legacy-absent plus native-demo-required scan to the extracted
member.
`make iso-cpp-aot` is a compatibility alias for that canonical lane.

The retained comparison lane uses isolated outputs and reverses the byte-level
selection proof:

```sh
make iso-legacy-opencl-c
# bld/trueos-legacy-opencl-c.iso
```

After booting `bld/trueos.iso` on the physical `00:02.0`, `8086:4680`, revision
`0x0c` TestRig, run `gpgpu probe copy-rect` and save the complete output. The
host verifier turns that output into a strict promotion record:

```sh
make intel-gpu-verify-copy-cpp-hardware-log \
  INTEL_GPU_CPP_PROBE_LOG=/path/to/copy-rect-probe.log
```

It requires the canonical C++ Zebin hash and exact successful summary plus all
four source-defined case records. It rejects logs from another BDF, device,
revision, frontend, feature, artifact source, or geometry, and rejects
missing, duplicate, reordered, failed, or over-timeout cases.

Existing artifacts can receive a contract without rebaking:

```sh
python3 -B tools/intel-gpu-bakery/generate_existing.py \
  --bin path/kernel.bin \
  --spv path/kernel.spv \
  --source path/kernel.cl \
  --expect-kernel kernel
```

That mode labels compiler provenance unavailable and should only migrate
already-reviewed artifacts. New artifacts should always use `bake.py`.

## Contract guardrails

The parser fails rather than guessing when kernel names, `.text` sections, or
function symbols are ambiguous. It records section and entry ranges
separately; the entry is section file offset plus symbol value and must be
64-byte aligned for the TRUEOS interface descriptor. It also records SIMD/GRF,
scratch/SLM, the complete execution-environment map, cross/per-thread payload
sizes, implicit global/local/enqueued-local records, the local-ID record, BTIs,
by-value offsets, pointer access/address modes, and source argument metadata.

The ADL-S profile currently requires SIMD16 and zero scratch/SLM. Missing
`.ze_info` scratch/SLM fields mean zero; `.ze_info` minor versions are recorded
as data and are not hard-coded parser gates.

One important compiler nuance is intentionally pinned: do not add explicit
`-O0` casually. With the tested stack it caused IGC to remove both stateful
BTIs and `buffer_address` records, breaking the established direct-RCS ABI.
The warning policy is explicit (`-Wall -Wextra -Werror -Wdate-time`), while
the profile's exact option list deliberately contains no `-O*` flag.
