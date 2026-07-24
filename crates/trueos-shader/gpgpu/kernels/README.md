# Intel GPGPU Kernels

This directory holds the small OpenCL C and C++ for OpenCL kernels baked into
embedded Gen12/Alder Lake artifacts for TRUEOS.

`copy_rect_rgba8.clcpp` is the first C++ for OpenCL source selected by the
normal Make product/development lane. Its side-by-side artifact identity,
exact offline frontend flags, ADL-S ABI comparison, and hardware conformance
gate are recorded in
[`CPP_FOR_OPENCL_OPT_IN.md`](CPP_FOR_OPENCL_OPT_IN.md).

`cpp_demo_rgba8.clcpp` is the first native C++/IGC application kernel rather
than an ABI twin. One resident entry exposes five Shell2/UI4 generative modes
through the dedicated `cpp` command. Its workload map, ABI, publication policy,
and TestRig procedure are recorded in
[`CPP_DEMO_SUITE.md`](CPP_DEMO_SUITE.md).

`cpp_audio_visualizer_rgba8.clcpp` is the single-kernel live audiovisual
instrument behind `cpp audio`. It consumes a compact FFT/PCM snapshot from an
allocation-free pre-HDA tee and composes waveform, phase, spectrum, prism,
onset, bass, and particle ideas into one resizable UI4 surface. Its audio
boundary, 50% horizontal-pair walker shape, ABI, and TestRig procedure are in
[`CPP_AUDIO_VISUALIZER.md`](CPP_AUDIO_VISUALIZER.md).

`spirit_vfx_background_rgba8.clcpp` and
`spirit_vfx_sprite_rgba8.clcpp` are exact-ABI C++ repasses of Spirit's retained
9-background and 16-sprite collections. They preserve the established
two-walker cursor-plane path while adding compile-time-specialized secondary
detail. The design, visual replay, hashes, and TestRig commands are recorded in
[`SPIRIT_CPP_REPASS.md`](SPIRIT_CPP_REPASS.md).

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
- `alpha_blend_worklist_rgba8.cl`: descriptor worklist RGBA8 composites; source/destination rects are unscaled and batched like the fill worklist
- `glyph_mask_rgba8.cl`: 8-bit coverage mask blended with packed RGBA8 color
- `sprite_quad_worklist_rgba8.cl`: arbitrary sprite-quad descriptors sampled from RGBA8 or XRGB source surfaces and copied or source-over blended into RGBA8/XRGB destinations
- `mandel64_worklist_rgba8.cl`: clipped 64x4 Mandelbrot row-band descriptors; each descriptor can either mirror across the real axis or compute an unmirrored viewport
- `chart_sine_rgba8.cl`: full-frame analytical 2D scope plot with grid, axes, border, anti-aliased sine line, and optional glow; available as the `gpgpu preview start chart` arbitrary-surface UI4 compute node
- `pixel_plasma_rgba8.cl`: full-frame procedural scalar-field pixel kernel with a FluidX3D-inspired scientific palette, vignette, radial interference, and scanlines; available as the `gpgpu preview start plasma` arbitrary-surface UI4 compute node
- `cpp_demo_rgba8.clcpp`: one exact-target C++ for OpenCL/IGC application kernel with gallery, aurora, Julia-set, signed-distance, and Voronoi modes; available through the dedicated `cpp` Shell2 command
- `cpp_audio_visualizer_rgba8.clcpp`: one exact-target C++/IGC audiovisual composition driven by the final 48 kHz stereo HDA-bound mix, a 2048-point mid/side FFT, 64 bands, and 128-point channel waveforms; available through `cpp audio`
- `lab256_multiphase.cl`: hash-locked 256x256 three-entry experimental artifact retained for the live `gpgpu test lab256` Shell2/UI4 preview through the vGPU/GuC GPGPU lane; it contains a centered grayscale smoke ripple, Gray-Scott pointer trail, compact GPU telemetry, and one half-second CUR_SURFLIVE-rate status dot
- `spirit_vfx_background_rgba8.clcpp` and `spirit_vfx_sprite_rgba8.clcpp`: TrueOS-Spirit's continuous 60 Hz C++/IGC cursor-plane producer; the default clean-Lilly batch dispatches only the sprite presentation walker, while enabling a procedural background adds the background walker and ordered source-over dependency; the retained `.cl` sources are its reviewed semantic and ABI references
- `font_outline_mesh.cl`: allowlisted Skrifa outline consumer used by `gpgpu probe font-tessel`; it audits the packed command stream, flattens quadratic/cubic curves, and emits indexed contour-stroke triangles without CPU geometry math
- `font_outline_coverage_r8.cl`: production Skrifa-afterpath consumer; it evaluates non-zero winding plus nearest-edge distance in final mask-pixel coordinates and writes reusable fractional R8 coverage with bounded low-ppem optical bias

The rect and sprite worklist kernels share a descriptor-driven shape:

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

`ui4_nv12_tile64_to_rgba8_frame.cl` is the SIMD16 video Frame producer. It
converts a decoder-owned Tile64 NV12 source into the complete, exact UI4 RGBA8
lease (opaque black outside the selected native viewport); it neither reads a
display backbuffer nor programs a plane. The Alder Lake S artifact SHA-256 is
`f33f0f2f531aa4df74b932fd519d5c096f9576b94c09cf1e20b742151092e0b5`.

`artifacts/adls/cpp/copy_rect_rgba8.bin` is the Make-default C++ for OpenCL
Alder Lake S build produced with Intel `ocloc`/IGC. Its SHA-256 is:

```text
b36d1c7742003591a5074663d81a4162412618ae425c47d30be6d068ee144a25
```

`artifacts/adls/cpp/cpp_demo_rgba8.bin` is the unconditional C++/IGC demo
artifact for exact target `8086:4680`, revision `0x0c`. Its SHA-256 is:

```text
19f7067fa19ba34a640d1f3d67de3df82d29f484700a274bc4bb31c4b00b7009
```

`artifacts/adls/cpp/cpp_audio_visualizer_rgba8.bin` is the unconditional
single-kernel audiovisual artifact for that same exact target. Its SHA-256 is:

```text
951e0cb30b42a755812b00eb0c3871f52c765ee74295dc3cb48b84f8361c1b19
```

The two unconditional Spirit C++/IGC artifacts are exact ABI twins of the
retained OpenCL C binaries:

```text
artifacts/adls/cpp/spirit_vfx_background_rgba8.bin  6e1f90a2af800103f95fcca3de25320f0b9b7b73fbf941d7852ec408b1375f19
artifacts/adls/cpp/spirit_vfx_sprite_rgba8.bin      2ee466aa00e631119e8de1eb9fa2d53a1b39d46cc56b4ce2e16ff18f653343ac
```

`artifacts/adls/copy_rect_rgba8.bin` is the retained legacy OpenCL C
comparison/fallback. Its SHA-256 is:

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

The two retained Spirit VFX artifacts are hash-locked ABI references for the
C++ repass. Background-enabled submissions
use one ordered two-walker batch; the clean default omits the background
walker. The background artifact implements the selected ten-mode set:
`Energy ring`, `Magic circle`, `Nebula smoke`, `Cyber grid`, `Portal vortex`,
`Speed lines`, `Bokeh field`, `Water ripples`, `Pixel burst`, and the
C++ `Magic time circle`. The sprite
artifact implements the complete stable ID 0–15 preview set from
`Original / clean` through `Dream bloom`. Their legacy ADL-S binary hashes are:

```text
spirit_vfx_background_rgba8.bin  527042d30fdfeaf111d491b9497ad7d6f0fb5c51369da2968a53b85344da752f
spirit_vfx_sprite_rgba8.bin      f1264ac062d5645c8d4da55e1585ee22c56cfb7a341d28407d3b934e97821ddc
```

The exact control-page, UI JSON, artifact, and display-release contracts are
recorded in [`SPIRIT_VFX_EXPLORE.md`](SPIRIT_VFX_EXPLORE.md).

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
gpgpu/bake_adls_artifacts.sh alpha_blend_worklist_rgba8 sprite_quad_worklist_rgba8
```

With no arguments, the script rebuilds every kernel source that has a matching
`artifacts/adls/*.bin` output:

```sh
gpgpu/bake_adls_artifacts.sh
```

The script accepts `OCLOC=/path/to/ocloc` for a system toolchain. If `OCLOC` is
not set, it uses the local extracted toolchain under `bld/intel-tools/root`.
