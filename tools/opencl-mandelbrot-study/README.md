# OpenCL Mandelbrot: three iGPU compile check

`mandelbrot.cl` is one OpenCL C 1.2 kernel with a dynamic escape loop. Each work-item writes one `uint` iteration count into a normal global buffer. No image, sampler, OpenGL, or display is required.

The example launch contract is 32 × 24 work-items, output size 3,072 bytes, `origin_x=-2.0f`, `origin_y=-1.125f`, `step=0.09375f`, `max_iterations=64`. Width, height, origin, step and iteration limit are runtime arguments; the compiler does not specialize these example values. Global IDs must fit `uint`, and the caller must allocate at least `width * height` output words with checked arithmetic. All work-items in the logical image must be covered; out-of-range work-items return before writing.

```sh
python3 tools/opencl-mandelbrot-study/compile.py
```

The script builds for the three physical device IDs and recorded revisions:

- Ubuntu host: A780 revision 04 (observed with local `lspci`).
- TRUEOS rig: 4680 revision 0c (checked-in hardware report).
- i7-1185G7 laptop: 9A49 revision 01 (repository laptop record and TGL admission contract; not freshly queried).

For each, it compares default compiler SIMD selection against `intel_reqd_sub_group_size(8)`. Both retain the same arithmetic with floating-point contraction disabled. It saves the OpenCL/SPIR-V/Zebin, metadata, full text-section disassembly, executable disassembly, compile/validation logs, exact commands and hashes. IGA must reassemble the native image through its last EOT/return byte-for-byte, including device callees after kernel EOT. The section trailer is saved separately in the full section, not counted as executable instructions.

Dependencies: `ocloc`, `iga64`, Python 3, and matching IGC libraries. On this host, ocloc 26.05.037020 needed local `libigc2` and `libigdfcl2` version 2.28.4-4. These were checksum-verified using the repository's `tools/intel-gpu-bakery/download_locked_package.sh` and extracted under `bld/opencl-mandelbrot-toolchain/root` without installing system packages. `compile.py` automatically adds that library directory; `--library-dir PATH` selects another. Archive SHA-256 values are:

```text
libigc2_2.28.4-4_amd64.deb
b7626c939d27434b3e107ad5b7d883c6115efe8cc59d6435b09ed09ce737e7c2
libigdfcl2_2.28.4-4_amd64.deb
4f5188166280a82356def0c305274cfe0c4b3bbc30e21c8413ee679f4b77ef8d
```

No GPU execution, driver loading, deployment, timing measurement or remote access occurs. A valid OpenCL device binary still needs the OpenCL runtime or an explicitly implemented compatible runtime contract to dispatch it.

See [the report](../../docs/experiments/opencl-mandelbrot-20261024.md) for results and annotated control flow.

## 512×512 subdivision experiment

```sh
python3 tools/opencl-mandelbrot-study/study_tiles.py
```

This builds `mandelbrot_tiles.cl` three ways: actual recursive device calls (`RECURSIVE_PROBE`), explicit depth-first stack (default), and 64 parallel leaf workgroups (`PARALLEL_LEAVES`). Each is built for all three GPUs with default width and explicit SIMD8. Actual recursion is accepted by this IGC version but is outside portable OpenCL C and emits stack-allocation warnings.

The recursive/stack variants launch one 256-work-item group (`global=256`, `local=256`) and accept nonzero dimensions at most 512. The parallel variant requires 512×512 and launches 64 groups (`global=16384`, `local=256`); each group derives a leaf from six path bits. These are one-dimensional NDRanges; each group uses only dimension-zero IDs. Each 64×64 leaf contains 4,096 pixels, so each local work-item calculates 16 pixels. No upward merge, intergroup barrier or recursive device enqueue is used.

`study_tiles.py` also compiles the exact kernel sources as C++ for CPU simulation, comparing every pixel with the original flat source, checking 144 partition geometries, output guards and rejected dimensions. AddressSanitizer and UndefinedBehaviorSanitizer are enabled; leak detection is disabled because the execution environment prevents LeakSanitizer from operating. These checks do not execute EU binaries or prove device stack sufficiency.

See [the subdivision report](../../docs/experiments/opencl-mandelbrot-subdivision-20261024.md).
