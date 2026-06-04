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

The next embedded API seed artifacts are compiled for focused UI/GPGPU bring-up:

- `fill_rect_rgba8.cl`: parameterized RGBA8 fill
- `fill_circle_rgba8.cl`: parameterized RGBA8 circle fill clipped by a rect
- `blit_rgba8_nearest.cl`: nearest-neighbor RGBA8 rect blit
- `alpha_blend_rgba8_over.cl`: source-over RGBA8 blend
- `glyph_mask_rgba8.cl`: 8-bit coverage mask blended with packed RGBA8 color
- `stamp_mandel_rgba8.cl`: ten-iteration Mandelbrot stamp using destination x/y as both stamp origin and view offset
- `sprite64_worklist_rgba8.cl`: fixed 64x64 sprite descriptors copied/blended from atlas to destination; shell path batches descriptor slices as multiple walkers in one command buffer
- `canvas512_3d_project_rgba8.cl`: fixed 512x512 Q16 vec3 projection into packed XY/RGBA point records
- `canvas512_3d_translate_q16.cl`: range/subset Q16 vec3 translation from source int4 vertices to destination int4 vertices
- `canvas512_3d_scale_q16.cl`: range/subset Q16 vec3 component scale from source int4 vertices to destination int4 vertices
- `canvas512_3d_rotate_quat_q16.cl`: range/subset Q16 vec3 quaternion rotation from source int4 vertices to destination int4 vertices

The canvas512 transform kernels use the same SIMD16 lane-stride shape. Their
OpenCL cross-thread argument order is:

```text
src_vertices_q16, dst_vertices_q16, src_first_vertex, dst_first_vertex, vertex_count, delta_q16
src_vertices_q16, dst_vertices_q16, src_first_vertex, dst_first_vertex, vertex_count, scale_q16
src_vertices_q16, dst_vertices_q16, src_first_vertex, dst_first_vertex, vertex_count, quat_q16
```

The final `int4` argument is 16-byte aligned in the cross-thread payload.
After the three `uint` fields, the CPU payload leaves one dword of padding
before writing the `int4` lanes.

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
