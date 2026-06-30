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
- `alpha_blend_rgba8_over.cl`: source-over RGBA8 blend
- `alpha_blend_worklist_rgba8.cl`: descriptor worklist RGBA8 composites; source/destination rects are unscaled and batched like the fill worklist
- `glyph_mask_rgba8.cl`: 8-bit coverage mask blended with packed RGBA8 color
- `present_rgba8_to_primary_xrgb_rect.cl`: RGBA8 scene rect to primary XRGB rect with optional source Y flip
- `stamp_mandel_rgba8.cl`: ten-iteration Mandelbrot stamp using destination x/y as both stamp origin and view offset
- `sprite64_worklist_rgba8.cl`: fixed 64x64 sprite descriptors copied/blended from atlas to destination; shell path batches descriptor slices as multiple walkers in one command buffer
- `sprite_quad_worklist_rgba8.cl`: arbitrary UI3 SpriteQuad descriptors sampled from RGBA8 source surfaces and source-over blended into RGBA8 destinations
- `mandel64_worklist_rgba8.cl`: clipped 64x4 Mandelbrot row-band descriptors; shell scanout computes the top half and mirrors it across the real axis
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
- `sprite_quad_worklist_rgba8.cl` descriptors are four `x/y/u/v` float corners plus `{ color_rgba, flags }`; the current flag bit selects source-over
- packed coordinates use 16-bit lanes; destination coordinates are signed

These are intended to replace the old single-rect stage-1 fill/alpha path for
batched UI chrome/overlay subsets while keeping the smaller kernels available
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
evo build for UI chrome bands and procedural strips. Its SHA-256 is:

```text
d3e6d5ec26c2b789d43d3308cf740977ce52f5b4df2325a27c92a687796d9149
```

`artifacts/adls/alpha_blend_worklist_rgba8.bin` is the descriptor composite
evo build. Its SHA-256 is:

```text
74e2f00828973323f4bebb4b9c513ef249fc15080fddbd39a1b8a9e412b646a7
```

`artifacts/adls/present_rgba8_to_primary_xrgb_rect.bin` is the RGBA scene to
primary XRGB present rect build. Its SHA-256 is:

```text
11afc516532bc0f48e9b9ede0e282fc3eb50c64ebc02dba06e38646e3b20e54a
```

`artifacts/adls/sprite64_worklist_rgba8.bin` is the fixed-size sprite worklist
build. Its SHA-256 is:

```text
7942acab497d8fd3b7d406679f1b2a614f3f4eef78df2e667b9f404e34a822fb
```

`artifacts/adls/sprite_quad_worklist_rgba8.bin` is the arbitrary UI3 sprite
quad worklist build. Its SHA-256 is:

```text
9382139a63a33c0e4618171158759513418696f912e56610c0dfa5099bdcbdd7
```

`artifacts/adls/mandel64_worklist_rgba8.bin` is the descriptor Mandelbrot
tile worklist build with clipped 64x4 row-band descriptors, mirrored half-scanout,
32-bit Q12 arithmetic, and descriptor-controlled iteration cap plus grayscale
scale. Its SHA-256 is:

```text
79c7d4170540650417489a882e52c52b1a47f85182790dfc1c3a22ad64a6248d
```

Regenerate one or more ADL-S artifacts with the Intel IGC/`ocloc` toolchain:

```sh
tools/intel_gpgpu/bake_adls_artifacts.sh alpha_blend_worklist_rgba8 present_rgba8_to_primary_xrgb_rect sprite64_worklist_rgba8
```

With no arguments, the script rebuilds every kernel source that has a matching
`artifacts/adls/*.bin` output:

```sh
tools/intel_gpgpu/bake_adls_artifacts.sh
```

The script accepts `OCLOC=/path/to/ocloc` for a system toolchain. If `OCLOC` is
not set, it uses the local extracted toolchain under `bld/intel-tools/root`.
