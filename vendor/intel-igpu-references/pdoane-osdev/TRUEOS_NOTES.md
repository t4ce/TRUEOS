# TRUEOS Notes: pdoane/osdev Intel iGPU Path

`pdoane/osdev` is valuable because it shows a complete minimal Intel iGPU path
inside a hobby OS, not just isolated register writes. The repository identifies
the target hardware as Intel Ivy Bridge with HD Graphics 4000.

In `gfx/gfx.c`, `CreateTriangle()` writes exactly three `vec4` vertices into a
vertex buffer. `CreateTestBatchBuffer()` then:

- switches to the 3D pipeline,
- programs `STATE_BASE_ADDRESS`,
- sets CC, blend, and depth/stencil pointers,
- sets binding tables and sampler pointers,
- programs CC and SF/clip viewports,
- programs URB state,
- sets the vertex buffer and vertex elements,
- programs the VS,
- disables HS, TE, DS, and GS,
- programs streamout, clip, drawing rectangle, SF, SBE, WM, and PS,
- programs sample mask, multisample, and null depth/stencil/hier-depth state,
- emits a dummy `_3DPRIMITIVE`,
- emits a real `_3DPRIMITIVE` with three vertices,
- writes a debug value with `MI_STORE_DATA_INDEX`,
- ends the batch with `MI_BATCH_BUFFER_END`.

`GfxStart()` also includes the scanout side of the path. It initializes PCI,
GTT, the graphics memory manager, display state, primary plane, cursor plane,
pipe size, render ring, render context, state heaps, shaders, triangle vertex
buffer, and batch buffer, then submits commands to the render ring. This makes
the code an end-to-end hardware bring-up reference from modeset/plane setup to
a visible 3D draw.

One important difference from the current TRUEOS Xe-LP/Xe path: the visible
batch does not contain an explicit `PIPE_CONTROL` fence inside
`CreateTestBatchBuffer()`. The author relies on the dummy draw after
`PIPELINE_SELECT`. That does not prove modern Xe hardware can avoid additional
flushes or invalidations; it only shows that this Gen7 path used a surprisingly
small 3D batch once the surrounding ring/context machinery was in place.
