# C++ for OpenCL architecture

## Status and identity

C++ for OpenCL is the only maintained Intel GPGPU source and artifact
architecture. Sources use `.clcpp`; generated artifacts live exclusively in
`artifacts/adls/cpp/`. Cargo and Make do not expose an alternate frontend or a
fallback selection.

The published artifacts are admitted only on the physical TestRig identity
`00:02.0`, `8086:4680`, revision `0x0c`.

`include/trueos_clcpp.hpp` is deliberately freestanding. It uses only OpenCL
device types, `constexpr`, `static_assert`, and an inline template. It does not
introduce a host C++ runtime, standard library, exceptions, RTTI, allocation,
static initialization, or an extra callable symbol in the Zebin.

## Required offline frontend route

With the locally tested Clang 21 toolchain, the metadata-faithful path is:

```text
C++ for OpenCL
  -> Clang LLVM bitcode (SPIR64)
  -> llvm-spirv with OpenCL argument metadata preservation
  -> OpenCL Kernel SPIR-V
  -> ocloc/IGC -spirv_input
  -> ADL-S Zebin
```

The essential commands are:

```sh
kernel_dir=/path/to/TRUEOS/crates/trueos-shader/gpgpu/kernels
build_root=/path/to/disposable-build-root
mkdir -p "$build_root/cpp"

(
  cd "$kernel_dir"
  clang \
    --target=spir64 \
    -x clcpp \
    -cl-std=CLC++ \
    -cl-kernel-arg-info \
    -fno-discard-value-names \
    -Wall -Wextra -Werror \
    -emit-llvm -c \
    copy_rect_rgba8.clcpp \
    -o "$build_root/copy_rect_rgba8.bc"
)

(
  cd "$build_root"
  llvm-spirv \
    --preserve-ocl-kernel-arg-type-metadata-through-string \
    copy_rect_rgba8.bc \
    -o cpp/copy_rect_rgba8.spv

  ocloc compile \
    -file cpp/copy_rect_rgba8.spv \
    -spirv_input \
    -device 0x4680 \
    -64 \
    -output copy_rect_rgba8 \
    -out_dir cpp \
    -output_no_suffix

  ocloc validate -file cpp/copy_rect_rgba8.bin
)
```

Keep the `ocloc` current working directory inside the disposable build root.
Some packages write `IGC_REVISION` and `NEO_REVISION` there even when
`-out_dir` points elsewhere.

Do not replace the two-stage Clang/`llvm-spirv` frontend with the direct
`clang --target=spirv64 -c` route without re-running the contract comparison.
The direct backend generated executable SPIR-V locally, but did not carry the
OpenCL kernel argument type/qualifier strings that this IGC consumes:

- `uint` appeared as diagnostic `int`
- the `const` source pointer appeared as `readwrite`
- argument names required explicit preservation

The physical payload layout still matched, but that loss is unacceptable for
an exact generated-manifest gate. The
`--preserve-ocl-kernel-arg-type-metadata-through-string` translator flag
restored the complete type, qualifier, and read-only information. Adding
`--spirv-preserve-auxdata` was tested and is unnecessary; it only enlarged the
embedded SPIR-V for this kernel.

## ADL-S ABI

The probe used:

- `/usr/bin/clang`: Ubuntu Clang `21.1.8`
- `/home/t4ce/REPOS/blender-default-cube-toggle/lib/linux_x64/dpcpp/bin/llvm-spirv`:
  LLVM `21.0.0git`
- `/home/t4ce/REPOS/blender-default-cube-toggle/lib/linux_x64/dpcpp/lib/ocloc/bin/ocloc`
- Intel IGC library
  `/home/t4ce/REPOS/blender-default-cube-toggle/lib/linux_x64/dpcpp/lib/igc/lib/libigc.so.2.30.0+0`
- ADL-S device ID `0x4680`

`LD_LIBRARY_PATH` contained the sibling `ocloc/lib`, `igc/lib`, and
`igc/lib/igc2` directories. These absolute paths record this particular probe;
the bakery must discover or receive its tool root rather than embed a
developer-machine path.

The system Clang bitcode was accepted by the bundled translator because both
are LLVM major 21. LLVM bitcode is not a version-stable interchange format, so
the release bakery must pin a compatible Clang/`llvm-spirv` pair (preferably
the same distribution and LLVM major). Bitcode is only a transient build file;
SPIR-V remains the durable compiler boundary and packaged provenance input.

Two clean builds in different temporary output directories were byte-for-byte
reproducible at the bitcode, SPIR-V, and Zebin stages when Clang received the
same source-path spelling. Passing the same file once by an absolute path and
once by a repository-relative path changed all three hashes because Clang
embeds that spelling in the bitcode and SPIR-V. The bakery must therefore
canonicalize the frontend invocation—for example, run Clang from the source
directory and pass `copy_rect_rgba8.clcpp` as a basename—or use and verify a
stable compiler prefix map.

The C++ source is validated and inspected through the generated `.ze_info`
contract:

| Contract field | Published result |
| --- | --- |
| entry and text section | `copy_rect_rgba8`, `.text.copy_rect_rgba8` |
| text offset / symbol size | `0x40` / `712` bytes |
| SIMD / GRF count | `16` / `128` |
| binding table | argument 0 at BTI 0; argument 1 at BTI 1 |
| cross-thread / per-thread | `96` / `96` bytes |
| pointer payloads | arguments 0 and 1 at offsets 48 and 56, size 8 |
| scalar payloads | arguments 2 through 9 at offsets 64 through 92, size 4 |
| pointer access | source `readonly`; destination `readwrite` |
| argument names/types | exact names, `uint*`/`uint`, source `const` |
| scratch / SLM | none |

`ocloc validate` decoded both as valid single-kernel binaries with two binding
table entries and 96-byte cross-thread and per-thread payloads.

The local validator emitted two non-fatal tool-version warnings:

- `.note.intelgt.metrics` is not handled by that validator build
- emitted `.ze_info` minor 64 is newer than decoder minor 54

It still reported the binary as valid and decoded all contract fields above.

## Hardware conformance

The physical copy proof covers even and odd widths, non-zero
source/destination origins, different pitches, and guard pixels. Publication
also requires manifest validation and pinned Clang, `llvm-spirv`, IGC, and
`ocloc` provenance.

The complete physical transcript is checked on the host with:

```sh
make intel-gpu-verify-copy-cpp-hardware-log \
  INTEL_GPU_CPP_PROBE_LOG=/path/to/copy-rect-probe.log
```
