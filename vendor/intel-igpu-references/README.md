# Intel iGPU Reference Snapshots

These files are vendored as read-only bring-up references. They are not wired
into the TRUEOS build.

## pdoane/osdev

- Source: https://github.com/pdoane/osdev
- Commit: `423da913cbdd558fca0de652125c359c686b4ba3`
- Local path: `pdoane-osdev/gfx/`
- License/provenance: see `pdoane-osdev/LICENSE` and `pdoane-osdev/AUTHORS`
- TRUEOS notes: `pdoane-osdev/TRUEOS_NOTES.md`

Useful Intel iGPU material:

- `gfx/gfx.c`: manual Ivy Bridge render setup, including display handoff,
  render ring/context setup, state heaps, shader upload, vertex buffer,
  binding tables, URB programming, 3D state packets, dummy draw, and triangle
  draw.
- `gfx/gtt.c`, `gfx/gfxmem.c`, `gfx/gfxring.c`: GGTT allocation/mapping and
  render-ring submission model.
- `gfx/gfxdisplay.c`, `gfx/reg.h`: display and register constants used by the
  sample.
- `gfx/shaders/`: passthrough VS and solid PS EU binaries used by the triangle.

## drm/igt-gpu-tools

- Source: https://gitlab.freedesktop.org/drm/igt-gpu-tools
- Commit: `d34395a7ede75d0b83edac965e27c0512bc0fe6e`
- Local path: `igt-gpu-tools/lib/`
- License/provenance: see `igt-gpu-tools/COPYING`

Useful Intel iGPU material:

- `lib/rendercopy_gen7.c`: Gen7 render-copy pipeline setup with surface state,
  sampler/binding tables, vertex data, shaders, URB, 3D state, and primitive
  submission.
- `lib/gen7_render.h`: Gen7 packet and state-field definitions used by
  `rendercopy_gen7.c`.
- `lib/intel_batchbuffer.*`, `lib/rendercopy.h`, `lib/intel_reg.h`: helper and
  register context needed to read the render-copy code coherently.
