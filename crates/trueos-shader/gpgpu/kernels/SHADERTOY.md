# ShaderToy reviewed Image catalog

This directory carries a deliberately narrow first ShaderToy integration. The
review inputs are in `shadertoy/*.glsl`; the generated C++ for OpenCL kernels
are `shadertoy_*.clcpp`; and the exact ADL-S artifacts and ABI contracts are in
`artifacts/adls/cpp/`.

Regenerate and reproducibly verify the complete catalog with:

```text
make intel-gpu-bake-shadertoy-cpp
```

The Blueprint ABI admits only catalog IDs 1 through 3 and a pointer-free,
64-byte ShaderToy uniform block. Source text, SPIR-V, Zebin, arbitrary dispatch
geometry, pointers, and GPU virtual addresses remain kernel-owned pending a
broader security analysis.

The current artifacts are:

| ID | Entry point | Zebin SHA-256 |
|---:|---|---|
| 1 | `shadertoy_mandelbrot` | `79e566ad2db01a1a2467e0289bd97e9c77c67be7bd4a59d957dadd84e0ec32d1` |
| 2 | `shadertoy_cube_field` | `0d48ef4d170eafe0cec5ae3952abdc6e57e865b195dbc3fc137ca7eb1b25d736` |
| 3 | `shadertoy_nguyen` | `1dbc80b468dd896073dd17c3963a5c7cccf814365e21f040e05a3522fea4cd9c` |

All three contracts are SIMD16 with 96 bytes of cross-thread data, 96 bytes of
per-thread local IDs, and no scratch or SLM. The cube-field artifact exposes
its read-only uniform argument as both a stateless pointer and BTI 1; direct RCS
dispatch binds the same kernel-owned block through both representations.
