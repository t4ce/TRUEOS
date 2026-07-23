# TRUEOS Intel GPU artifact bakery

This is an **opt-in host tool**. It is not called by `build.rs` or an ordinary
Cargo build. TRUEOS only embeds the resulting SPIR-V, Intel Zebin, and compact
generated Rust contract; Clang, `llvm-spirv`, `ocloc`, IGC, and C++ are absent
at runtime.

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
  --variant cpp \
  --abi-reference-bin crates/trueos-shader/gpgpu/kernels/artifacts/adls/copy_rect_rgba8.bin \
  --expect-kernel copy_rect_rgba8 \
  --repro-check \
  --write-toolchain-lock /tmp/adls-cpp-proof.lock.json
```

Review both the candidate lock and generated artifact/manifest before
replacing the repository lock.

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

After linking a feature-selected kernel, the Make target also proves that the
final TRUEOS ELF contains the complete C++ Zebin and no complete copy of the
legacy copy Zebin:

```sh
make intel-gpu-verify-linked-copy-cpp
```

`make iso-cpp-aot` goes one boundary further. It creates
`bld/trueos-cpp-aot.iso`, extracts `/TRUEOS.elf` back from that ISO, requires
byte identity with the stripped/staged runtime ELF, and applies the same
selected-present/legacy-absent scan to the extracted member.

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
