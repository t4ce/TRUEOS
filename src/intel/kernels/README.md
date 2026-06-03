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
- one SIMD16 walker/subgroup copies up to 32 pixels, two adjacent pixels per lane/work-item

`copy_rect_rgba8_wide.cl` is the opt-in wider sibling:

- same linear RGBA8 copy contract
- one SIMD16 walker/subgroup copies up to 256 pixels, sixteen adjacent pixels per lane/work-item
- kept separate from `copy_rect_rgba8.cl` so the small, proven bring-up artifact remains available

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
10866024aaffae96f92cfc25a5fb188ca421994789afbc4dba3ddc290bd583ab
```

`artifacts/adls/copy_rect_rgba8_wide.bin` is the wider Alder Lake S build. Its
SHA-256 is:

```text
f8097f59d7525c4fd6e4f6659e58d5279238a4f9652fea58065098540fa45bba
```

Regenerate it with:

```sh
tools/intel_gpgpu/build_copy_rect_rgba8.sh
tools/intel_gpgpu/build_copy_rect_rgba8_wide.sh
```

Generate the clear kernel with:

```sh
tools/intel_gpgpu/build_clear_rect_rgba8_white.sh
```

Generate the empty EOT kernel with:

```sh
tools/intel_gpgpu/build_empty_eot.sh
```
