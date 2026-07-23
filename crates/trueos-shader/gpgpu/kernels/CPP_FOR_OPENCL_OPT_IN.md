# C++ for OpenCL opt-in: `copy_rect_rgba8`

## Status and identity

`copy_rect_rgba8.clcpp` is a source-side opt-in twin of the production
`copy_rect_rgba8.cl`. The OpenCL C source and its checked-in artifacts remain
the default.

The stable side-by-side identity is:

| Layer | OpenCL C production | C++ opt-in |
| --- | --- | --- |
| source | `copy_rect_rgba8.cl` | `copy_rect_rgba8.clcpp` |
| kernel entry | `copy_rect_rgba8` | `copy_rect_rgba8` |
| ADL-S artifacts | `artifacts/adls/copy_rect_rgba8.{spv,bin}` | `artifacts/adls/cpp/copy_rect_rgba8.{spv,bin}` |

Keeping the exported entry name and placing only the artifact pair in a
separate directory lets the Rust catalog choose the implementation at compile
time without changing any RCS payload or dispatch code.

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

## ADL-S comparison

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

The C++ source was compiled with the commands above. The legacy `.cl` source
was freshly compiled by the same `ocloc` installation, and both Zebins were
validated and inspected with `readelf`. The resulting `.ze_info` contracts
matched exactly:

| Contract field | C and C++ result |
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

The checked-in production Zebin has `.ze_info` version `1.70`; this local IGC
emitted version `1.64`. Apart from that schema-version line, the checked-in C
contract and the C++ contract were identical. This is a toolchain-version
pinning concern, not an ABI difference.

IGA disassembly also showed that the checked-in C artifact and the C++ artifact
have the same Gen12 instruction stream through the end-of-thread send. The
first 632 bytes are byte-for-byte identical. Only eight bytes in two immediate
values after EOT differ; they are in the compiler bookkeeping/padding tail.
The complete `.text` hashes therefore differ even though the executable
instruction prefix is identical.

The local validator emitted two non-fatal tool-version warnings:

- `.note.intelgt.metrics` is not handled by that validator build
- emitted `.ze_info` minor 64 is newer than decoder minor 54

It still reported the binary as valid and decoded all contract fields above.

## Remaining promotion gate

This is enough to opt the artifact into a compile-time catalog path, but it is
not yet permission to delete or overwrite the production OpenCL C artifact.
Promotion should require:

1. generated-manifest validation of every field in the table above;
2. an ADL-S hardware copy test covering even and odd widths, non-zero
   source/destination origins, different pitches, and guard pixels;
3. equality of the destination against the existing C artifact for those
   cases;
4. a pinned Clang, `llvm-spirv`, IGC, and `ocloc` provenance record.

The C source should remain the fallback until that hardware comparison passes.
