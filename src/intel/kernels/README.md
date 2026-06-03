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

`clear_rect_rgba8_white.cl` is the smallest clear/fill target:

- destination: linear RGBA8
- fixed clear color: opaque white
- no source buffer
- no color parameter yet
- rectangular write only

`empty_eot.cl` is the smallest no-op target:

- no arguments
- no memory writes
- useful as a compiled EOT/scheduler bring-up artifact

`artifacts/adls/copy_rect_rgba8.bin` is the current Alder Lake S build produced
with Intel `ocloc`/IGC. Its SHA-256 is:

```text
c68cd718dcd2241db1d0f44db54b7b9b1c707aa89c524eddc8bd2b9f6978e249
```

Regenerate it with:

```sh
tools/intel_gpgpu/build_copy_rect_rgba8.sh
```

Generate the clear kernel with:

```sh
tools/intel_gpgpu/build_clear_rect_rgba8_white.sh
```

Generate the empty EOT kernel with:

```sh
tools/intel_gpgpu/build_empty_eot.sh
```
