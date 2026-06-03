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
- one SIMD16 walker/subgroup copies up to 64 pixels across 64 rows, four adjacent pixels per lane/work-item
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

The next embedded API seed artifacts are compiled but not exercised at boot:

- `fill_rect_rgba8.cl`: parameterized RGBA8 fill
- `fill_circle_rgba8.cl`: parameterized RGBA8 circle fill clipped by a rect
- `blit_rgba8_nearest.cl`: nearest-neighbor RGBA8 rect blit
- `alpha_blend_rgba8_over.cl`: source-over RGBA8 blend
- `glyph_mask_rgba8.cl`: 8-bit coverage mask blended with packed RGBA8 color
- `stamp_mandel_rgba8.cl`: ten-iteration Mandelbrot stamp using destination x/y as both stamp origin and view offset

`artifacts/adls/copy_rect_rgba8.bin` is the current Alder Lake S build produced
with Intel `ocloc`/IGC. Its SHA-256 is:

```text
10866024aaffae96f92cfc25a5fb188ca421994789afbc4dba3ddc290bd583ab
```

`artifacts/adls/copy_rect_rgba8_wide.bin` is the wider Alder Lake S build. Its
SHA-256 is:

```text
c94853560fdcad31703b8d556f303df1922ec645c236b55113a08b1ac367badd
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

Generate the embedded API seed artifacts with:

```sh
tools/intel_gpgpu/build_rect_api_artifacts.sh
```
