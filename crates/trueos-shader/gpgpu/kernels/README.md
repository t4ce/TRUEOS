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
- `alpha_blend_rgba8_over.cl`: single-dispatch 2D source-over RGBA8 blend with straight/premultiplied source and opacity modes
- `alpha_blend_worklist_rgba8.cl`: descriptor worklist RGBA8 composites; source/destination rects are unscaled and batched like the fill worklist
- `glyph_mask_rgba8.cl`: 8-bit coverage mask blended with packed RGBA8 color
- `present_rgba8_to_primary_xrgb_rect.cl`: RGBA8 scene rect to primary XRGB rect with optional source Y flip
- `stamp_mandel_rgba8.cl`: ten-iteration Mandelbrot stamp using destination x/y as both stamp origin and view offset
- `sprite64_worklist_rgba8.cl`: fixed 64x64 sprite descriptors copied/blended from atlas to destination; shell path batches descriptor slices as multiple walkers in one command buffer
- `sprite_quad_worklist_rgba8.cl`: arbitrary sprite-quad descriptors sampled from RGBA8 or XRGB source surfaces and copied or source-over blended into RGBA8/XRGB destinations
- `mandel64_worklist_rgba8.cl`: clipped 64x4 Mandelbrot row-band descriptors; each descriptor can either mirror across the real axis or compute an unmirrored viewport
- `chart_sine_rgba8.cl`: full-frame analytical 2D scope plot with grid, axes, border, anti-aliased sine line, and optional glow; available as the `gpgpu preview start chart` arbitrary-surface UI4 compute node
- `pixel_plasma_rgba8.cl`: full-frame procedural scalar-field pixel kernel with a FluidX3D-inspired scientific palette, vignette, radial interference, and scanlines; available as the `gpgpu preview start plasma` arbitrary-surface UI4 compute node
- `font_outline_mesh.cl`: allowlisted Skrifa outline consumer used by `gpgpu probe font-tessel`; it audits the packed command stream, flattens quadratic/cubic curves, and emits indexed contour-stroke triangles without CPU geometry math
- `font_outline_coverage_r8.cl`: production Skrifa-afterpath consumer; it evaluates non-zero winding plus nearest-edge distance in final mask-pixel coordinates and writes reusable fractional R8 coverage with bounded low-ppem optical bias
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
- `sprite_quad_worklist_rgba8.cl` descriptors are four `x/y/u/v` float corners plus `{ color_rgba, flags }`; flags select clear, source-over, premultiplied source RGB, and XRGB source/destination conversion
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

`artifacts/adls/alpha_blend_rgba8_over.bin` is the native two-dimensional
SIMD16 blend used by graphics consumers. Runtime overrides must
match its ABI-bound SHA-256:

```text
4b0f97f4f42f18792a82fe3e560589051b27a2db4e3b8af488798b7f4f3c5248
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

`artifacts/adls/sprite_quad_worklist_rgba8.bin` is the arbitrary sprite
quad worklist build. Its SHA-256 is:

```text
8dfc6217ff6346fe2660079fc905ed5e48187af48b0c90c5e0d5e56a80ef3437
```

`artifacts/adls/mandel64_worklist_rgba8.bin` is the descriptor Mandelbrot
tile worklist build with clipped 64x4 row-band descriptors, mirrored half-scanout,
optional full-height viewport work, 32-bit Q12 arithmetic, and
descriptor-controlled iteration cap plus grayscale scale. Its SHA-256 is:

```text
8b1746984f74156ccdbeb9431df9d25061285655067de8ebd5283b08de00d91f
```

`artifacts/adls/chart_sine_rgba8.bin` is the allowlisted analytical chart build.
Runtime filesystem overrides for this kernel are accepted only when their SHA-256
matches this embedded value:

```text
79eb20bc337e172a8ccddcdc6654eea992e89fb5fb67b2f32caad1c1afa1c0e4
```

`artifacts/adls/pixel_plasma_rgba8.bin` is the allowlisted procedural pixel
build. Its analytical field is intentionally buffer-free for bring-up; a later
FluidX3D field consumer can replace that scalar source while retaining the
palette, scanout, contract, and cadence path. It writes native premultiplied
ARGB8888 into a caller-owned composition surface. A UI4 frame producer can
publish that surface without a CPU format conversion or direct display-plane ownership. Runtime
overrides must match:

```text
42fb1dd0568bb244c44f87d146e036a72df60cb811715c370ec959de6d3af893
```

`artifacts/adls/font_outline_mesh.bin` is the allowlisted first font-geometry
compute build. Its input records are eight dwords: opcode, up to six IEEE-754
font-unit coordinates, and a reserved zero. The shell command exposes three
incremental hardware proofs:

- `audit`: validates opcodes, contour sequencing, finite coordinates, reserved
  fields, and the CPU/GPU FNV-1a checksum over the full `True OS §` stream
- `flatten`: expands every contour in the full `True OS §` stream into
  fixed-subdivision points entirely in compute
- `mesh`: emits four vertices and six indices per flattened segment for all
  glyph contours and checks every generated index before reporting success

The mesh stage is intentionally full-text outline-stroke geometry. It proves the
GPU-resident indexed-buffer shape and chains that same physical allocation into
the 3D raster pipeline, but does not claim hole-aware glyph fill yet. During
bring-up the CPU reads only the fixed report and index range to produce proof
logs; the generated geometry itself is not converted or used for CPU
tessellation. Runtime overrides must match:

```text
bf78e5d6870f2303b707d30320d8daa15554085a75d47a48b51fb932f4fa3d25
```

`artifacts/adls/font_outline_coverage_r8.bin` is the production analytical
font build used by the shared kernel font service, persisted GridPaper layers,
and the Draw3D TCP waiting scene. The CPU positions warmed Skrifa commands but
does not fill-tessellate them. Compute preserves contour orientation, applies
non-zero winding for holes, locally subdivides quadratic and cubic curves, and
encodes `clamp(0.5 + bias - signed_distance, 0, 1)` into R8. Every live mask
owns a distinct direct-RCS virtual range and passes a cold output audit before
`glyph_mask_rgba8.cl` supplies animated color source-over after scene resolve.
Runtime overrides must match:

```text
a4f0dddc7f2a9d9d67e5e71459d54da2e4a7ade8cd1af8c27283a884f221b836
```

Regenerate one or more ADL-S artifacts with the Intel IGC/`ocloc` toolchain:

```sh
gpgpu/bake_adls_artifacts.sh alpha_blend_worklist_rgba8 present_rgba8_to_primary_xrgb_rect sprite64_worklist_rgba8
```

With no arguments, the script rebuilds every kernel source that has a matching
`artifacts/adls/*.bin` output:

```sh
gpgpu/bake_adls_artifacts.sh
```

The script accepts `OCLOC=/path/to/ocloc` for a system toolchain. If `OCLOC` is
not set, it uses the local extracted toolchain under `bld/intel-tools/root`.
