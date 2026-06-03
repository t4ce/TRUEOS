# Intel GPGPU Kernels

This directory holds the small OpenCL C kernels intended to become embedded
Gen12/Alder Lake artifacts for TRUEOS.

`copy_rect_rgba8.cl` is the first standalone graphics value target:

- source: linear RGBA8
- destination: linear RGBA8
- no scaling
- no format conversion
- no blending
- rectangular copy only

The CPU side owns resource lifetime, bounds/scissor clipping, GPU address/state
binding, parameter packing, and walker submission.

`artifacts/adls/copy_rect_rgba8.bin` is the current Alder Lake S build produced
with Intel `ocloc`/IGC. Its SHA-256 is:

```text
c68cd718dcd2241db1d0f44db54b7b9b1c707aa89c524eddc8bd2b9f6978e249
```

Regenerate it with:

```sh
tools/intel_gpgpu/build_copy_rect_rgba8.sh
```
