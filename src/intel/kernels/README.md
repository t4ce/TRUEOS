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

The CPU side owns resource lifetime, bounds/scissor clipping, GPU address/state
binding, parameter packing, and walker submission.

The next embedded API seed artifacts are compiled for focused UI/GPGPU bring-up:

- `fill_rect_rgba8.cl`: parameterized RGBA8 fill
- `fill_rect_worklist_rgba8.cl`: descriptor worklist RGBA8 fills; one walker consumes the descriptor slice serially
- `gradient_rect_worklist_rgba8.cl`: descriptor worklist procedural RGBA8 gradients; each descriptor writes one horizontal or vertical rect from two endpoint colors
- `fill_circle_rgba8.cl`: parameterized RGBA8 circle fill clipped by a rect
- `blit_rgba8_nearest.cl`: nearest-neighbor RGBA8 rect blit
- `alpha_blend_rgba8_over.cl`: source-over RGBA8 blend
- `alpha_blend_worklist_rgba8.cl`: descriptor worklist RGBA8 composites; source/destination rects are unscaled and batched like the fill worklist
- `glyph_mask_rgba8.cl`: 8-bit coverage mask blended with packed RGBA8 color
- `present_rgba8_to_primary_xrgb_rect.cl`: RGBA8 scene rect to primary XRGB rect with optional source Y flip
- `stamp_mandel_rgba8.cl`: ten-iteration Mandelbrot stamp using destination x/y as both stamp origin and view offset
- `sprite64_worklist_rgba8.cl`: fixed 64x64 sprite descriptors copied/blended from atlas to destination; shell path batches descriptor slices as multiple walkers in one command buffer
- `mandel64_worklist_rgba8.cl`: fixed 64x4 Mandelbrot row-band descriptors; shell path expands each 64x64 tile into 16 bands
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
- one walker receives a descriptor slice through `desc_base` and `desc_count`
- the current bring-up kernel shape has work-item 0 walk the slice serially so
  multi-descriptor probes prove the CPU/GPGPU ABI before lane sharding returns
- `fill_rect_worklist_rgba8.cl` descriptors are `{ dst_xy, size, color_rgba }`
- `gradient_rect_worklist_rgba8.cl` descriptors are `{ dst_xy, size, color0_rgba, color1_rgba, flags }`, with `flags bit0` selecting vertical instead of horizontal
- `alpha_blend_worklist_rgba8.cl` descriptors are `{ src_xy, dst_xy, size, flags, color_rgba }`, with flags for direct copy, source-over, RGB tint, alpha tint, and premultiplied source
- packed coordinates use 16-bit lanes; destination coordinates are signed

These are intended to replace the old single-rect stage-1 fill/alpha path for
batched UI2 chrome/overlay subsets while keeping the smaller kernels available
for targeted bring-up.

`artifacts/adls/copy_rect_rgba8.bin` is the current Alder Lake S build produced
with Intel `ocloc`/IGC. Its SHA-256 is:

```text
10866024aaffae96f92cfc25a5fb188ca421994789afbc4dba3ddc290bd583ab
```

`artifacts/adls/fill_rect_worklist_rgba8.bin` is the descriptor fill evo build.
Its SHA-256 is:

```text
5e28e1a39c3b154ea6d7bc55fbbc99cfdca340eaf7a521b06bc7529b7a1c532b
```

`artifacts/adls/gradient_rect_worklist_rgba8.bin` is the descriptor gradient
evo build for UI2 chrome bands and procedural strips. Its SHA-256 is:

```text
d3e6d5ec26c2b789d43d3308cf740977ce52f5b4df2325a27c92a687796d9149
```

`artifacts/adls/alpha_blend_worklist_rgba8.bin` is the descriptor composite
evo build. Its SHA-256 is:

```text
636bd6dd2dde9e184d26c185ea04f6692476c1dec2c5fa26bf5f5b670cc1eb7e
```

`artifacts/adls/mandel64_worklist_rgba8.bin` is the descriptor Mandelbrot
tile worklist build with 64x4 row-band descriptors, 32-bit Q12 arithmetic, and
256-iteration grayscale escape coloring. Its SHA-256 is:

```text
8c98b459a26fb4a2f2f42e64683f6009f75c499cfa5f68fa81044012b11a4833
```

Regenerate it with:

```sh
tools/intel_gpgpu/build_copy_rect_rgba8.sh
```

Generate the embedded API seed artifacts with:

```sh
tools/intel_gpgpu/build_rect_api_artifacts.sh
```
