# Small OpenCL Mandelbrot kernel, three Intel iGPU targets

On 24 October 2026, all six offline builds succeeded: three device targets, each with default SIMD selection and with SIMD8 requested. Each binary passed `ocloc validate`; Intel IGA disassembled and reassembled the executable byte-for-byte. No GPU was opened, dispatched, benchmarked or remotely accessed.

## Source and buffer contract

The [22-line OpenCL C kernel](../../tools/opencl-mandelbrot-study/mandelbrot.cl) computes an escape iteration count per pixel. A sample workload is a **32 × 24** rectangle, stored in a **3,072-byte `uint` buffer**, with origin (-2, -1.125), step 0.09375 and a maximum of 64 iterations. These are proposed launch arguments, not compile-time constants or executed results.

The kernel starts at `z = 0`, iterates `z = z² + c`, and writes the iteration count when the magnitude exceeds two or the limit is reached. Width/height bounds protect padded dispatches. No texture, sampler, display pipeline, shared-local memory or workgroup barrier is requested. Floating-point contraction is disabled and fast-math options are absent.

## Targets and measured compilation results

| Machine | Physical GPU ID / revision | Compiler family mapping | Default code | Explicit SIMD8 code |
|---|---|---|---|---|
| Ubuntu host, UHD 770 | A780 / 04 | `adl-s`, IP 12.2.0 | SIMD16, 904 bytes, 73 instructions | SIMD8, 896 bytes, 70 instructions |
| Physical TRUEOS rig | 4680 / 0c | `adl-s`, IP 12.2.0 | SIMD16, 904 bytes, 73 instructions | SIMD8, 896 bytes, 70 instructions |
| i7-1185G7 laptop | 9A49 / 01 | `tgllp`, IP 12.0.0 | SIMD16, 904 bytes, 73 instructions | SIMD8, 896 bytes, 70 instructions |

The host ID/revision was observed using `lspci`. Rig and laptop targets came from checked-in project records, not a fresh remote hardware query. The historical laptop alias to ADL-S is described in [the compatibility experiment](tgl-9a49-adls-alias.md); this study passes the actual TGL device ID to `ocloc`.

Within each SIMD variant, **the executable bytes and full 1,152-byte text sections are identical across all three targets**. This is a measured property of this source and installed compiler, not a general binary-compatibility guarantee. Container/target metadata and runtime dispatch still matter. A780's `adl-s` mapping is ocloc's own response to the physical PCI ID; `ocloc ids rpl-s` and `ocloc ids adl-s` both report 12.2.0.

The default executable has 33 compacted 8-byte instructions plus 40 uncompacted 16-byte instructions: `33 × 8 + 40 × 16 = 904`. SIMD8 has 28 compacted plus 42 uncompacted: `28 × 8 + 42 × 16 = 896`. Byte size is distinct from instruction count, useful SIMD elements, dynamic iteration count and cycle cost.

All six metadata records specify the 128-GRF allocation mode. That field is not a measured peak of live register values. They do not report per-thread scratch memory buffers. The validator reports one binding-table entry, 96 cross-thread payload bytes, and 96 per-thread payload bytes.

The validator also warns about an unhandled metrics note and metadata version 1.62 versus decoder minor version 1.61, then explicitly reports **“Binary is VALID (decoded successfully).”** The [retained validation log](opencl-mandelbrot-20261024/validator.log) preserves those warnings rather than suppressing them.

## What to look at in the generated instructions

The [SIMD16 assembly](opencl-mandelbrot-20261024/simd16.asm) includes byte offsets. The [SIMD8 assembly](opencl-mandelbrot-20261024/simd8.asm) provides a direct comparison. These files exclude the compiler trailer and alignment padding after the terminal EOT; the full sections remain in the local build directory.

| SIMD16 byte offset | Generated behavior | Why it matters |
|---|---|---|
| `0000–00B0` | Payload/global-ID setup, dimension checks, masked `if` | Runtime supplies dispatch payload; lanes outside the image do not write. |
| `0100–0190` | Convert IDs to coordinates and initialize recurrence | One source expression operates across 16 pixel values. |
| `01A0–01D0` | `mul`/`add` update real and imaginary values | This is the useful recurrence math, repeated on the GPU. |
| `01E0–0260` | Radius/iteration comparisons and predicated `break` | Pixels may escape at different iterations; execution masks control remaining lanes. |
| `0278` | `while ... L416` | A real device-side backward loop; the host does not submit once per iteration. |
| `02D0–0318` | Compute the output word/byte offset | The buffer layout is ordinary row-major `y * width + x`. |
| `0320` | `send.dc1 (16...)` | Surface write using binding-table entry zero. |
| `0348–0358` | `send.dc0` fence and dependency wait | Device memory/message ordering is present in the complete kernel. |
| `0378` | `send.ts ... {EOT}` | Terminates the EU thread; host completion still needs the runtime's queue protocol. |

There is one static `while` instruction, one buffer-write SEND, one fence SEND and one EOT SEND in each executable. Static counts do not count how many times instructions execute across pixels or loop iterations. The complete source is scalar per work-item, while the backend selects SIMD16; manually spelling a `float16` is unnecessary for this result.

## Relationship to Odin / TRUEOS

This is a **complete OpenCL kernel and runtime ABI**, whereas Odin's current proven Mandelbrot path exports an arithmetic-only SIMD8 body and lets TRUEOS add coordinates, stores, fence/EOT and dispatch state. The OpenCL kernel has dynamic control flow; the bounded Odin body unrolls literal loops. The workloads and contracts differ, so comparing their binary sizes directly would be misleading.

The study exposes concrete compiler features to examine: invocation IDs, payload layout, loop divergence/masks, buffer addressing, instruction compaction and complete-kernel termination. It does not imply that these OpenCL binaries can be inserted into Odin's arithmetic-body upload interface. They require the matching payload, binding table, address mapping, dispatch and completion handling.

A standard OpenCL host could enqueue the whole rectangle as one NDRange operation. This compilation exercise does not establish how many physical submissions a particular runtime would perform, or how much CPU/GPU overlap it achieves. It provides no CPU-versus-GPU speed comparison.

## Reproduce and inspect

```sh
python3 tools/opencl-mandelbrot-study/compile.py
```

The [runner and setup notes](../../tools/opencl-mandelbrot-study/README.md) describe the checksum-pinned local compiler libraries. Outputs are under `bld/opencl-mandelbrot-study`: six binaries/SPIR-V modules, all metadata and assembly, validation logs, exact commands and hashes. The compiler is `ocloc` 26.05.037020 with local IGC 2.28.4-4 libraries; IGA identifies itself as 1.1.0.

The compact retained evidence is [results.json](opencl-mandelbrot-20261024/results.json), [SIMD16 metadata](opencl-mandelbrot-20261024/simd16.yaml), [SIMD8 metadata](opencl-mandelbrot-20261024/simd8.yaml), [SIMD16 EU bytes](opencl-mandelbrot-20261024/simd16.bin) and [SIMD8 EU bytes](opencl-mandelbrot-20261024/simd8.bin). Representative native files come from the host build; all six executable hashes were compared.

Target references: [rig execution report](odin-runtime-mandelbrot.md), [laptop physical-ID record](tgl-9a49-adls-alias.md), and local `ocloc compile --help` / `ocloc ids` output. These are compile checks, not fresh execution evidence for any machine.
