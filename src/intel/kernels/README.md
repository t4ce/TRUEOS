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
- `fill_rect_worklist_rgba8.cl`: descriptor worklist RGBA8 fills; one SIMD16 walker consumes up to 16 rect descriptors and each lane handles one descriptor stream
- `fill_circle_rgba8.cl`: parameterized RGBA8 circle fill clipped by a rect
- `blit_rgba8_nearest.cl`: nearest-neighbor RGBA8 rect blit
- `alpha_blend_rgba8_over.cl`: source-over RGBA8 blend
- `alpha_blend_worklist_rgba8.cl`: descriptor worklist source-over RGBA8 blends; source/destination rects are unscaled and batched like the fill worklist
- `glyph_mask_rgba8.cl`: 8-bit coverage mask blended with packed RGBA8 color
- `present_rgba8_to_primary_xrgb_rect.cl`: RGBA8 scene rect to primary XRGB rect with optional source Y flip
- `stamp_mandel_rgba8.cl`: ten-iteration Mandelbrot stamp using destination x/y as both stamp origin and view offset
- `sprite64_worklist_rgba8.cl`: fixed 64x64 sprite descriptors copied/blended from atlas to destination; shell path batches descriptor slices as multiple walkers in one command buffer
- `canvas3d_project_rgba8.cl`: Q16 vec3 projection into packed XY/RGBA point records with source/output ranges and dynamic canvas dimensions
- `canvas3d_transform_q16.cl`: range/subset Q16 vec3 fused scale, quaternion rotation, and translation from source int4 vertices to destination int4 vertices
- `canvas3d_clip_box_q16.cl`: idempotent Q16 vec3 source-to-sink box clip for presentation-safe geometry before projection

The canvas3d projector and transform kernels use the same SIMD16 lane-stride
shape. Their OpenCL cross-thread argument order is:

```text
vertices_q16, out_points, src_first_vertex, out_first_point, vertex_count, canvas_width, canvas_height
src_vertices_q16, dst_vertices_q16, src_first_vertex, dst_first_vertex, vertex_count, scale_q16, quat_q16, delta_q16
src_vertices_q16, dst_vertices_q16, src_first_vertex, dst_first_vertex, vertex_count, min_q16, max_q16
```

For the transform kernels, vector arguments in the cross-thread payload are 16-byte
aligned. After the three `uint` fields, the CPU payload leaves one dword of
padding before the first `int4`, then writes each additional `int4` on the next
16-byte slot. The current artifact metadata reports by-value vector offsets at
80, 96, and 112 bytes.

The rect worklist evo kernels share a descriptor-driven shape with the
`sprite64_worklist_rgba8.cl` path:

- the CPU owns clipping, surface binding, descriptor allocation, and descriptor
  chunking
- one SIMD16 walker receives a descriptor slice through `desc_base` and
  `desc_count`
- lane `N` processes descriptors `desc_base + N`, `desc_base + N + 16`, and so
  on
- `fill_rect_worklist_rgba8.cl` descriptors are `{ dst_xy, size, color_rgba }`
- `alpha_blend_worklist_rgba8.cl` descriptors are `{ src_xy, dst_xy, size }`
- packed coordinates use 16-bit lanes; destination coordinates are signed

These are intended to replace the old single-rect stage-1 fill/alpha path for
batched UI2 chrome/overlay subsets while keeping the smaller kernels available
for targeted bring-up.

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

`artifacts/adls/fill_rect_worklist_rgba8.bin` is the descriptor fill evo build.
Its SHA-256 is:

```text
07a38da4fc0272f8ed9bffd2833965f2cc937da52af8f353e5543d77b280e246
```

`artifacts/adls/alpha_blend_worklist_rgba8.bin` is the descriptor source-over
evo build. Its SHA-256 is:

```text
3485f2283c1510df619a1159d454af871cb847e0c79ee012f9e95da079d088c9
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
