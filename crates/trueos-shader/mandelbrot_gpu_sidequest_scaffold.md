# Mandelbrot GPU Sidequest Scaffold

## Status

This is a cold scaffold. It compiles into the kernel, but nothing calls it from
boot, shell, UI, Lumen, CGP, or the GPGPU matvec ladder.

The intended task entry is:

- `src/tst/mandelbrot_gpu_sidequest.rs`
- `spawn_mandelbrot_gpu_sidequest(spawner)`
- `mandelbrot_gpu_sidequest_task()`

## Artifact Shape

The current plan names the existing side-test fragment shader source:

- source: `.codex_tmp/mandelbrot_fragment_1440p_parametric.frag`
- planned SPIR-V path: `.codex_tmp/mandelbrot_fragment_1440p_parametric.spv`
- stage: fragment
- target: `2560x1440`
- push constants: 24 bytes for resolution, center, scale, and iteration count

The scaffold exposes `mandelbrot_fragment_shader_desc()` so a future caller can
turn real SPIR-V bytes into a `trueos_gfx_core::ShaderDesc` without inventing
another naming convention.

## Render And Present Options

Preferred first probe: render to an offscreen RGBA buffer/image first.

That gives a CPU-visible validation point before touching scanout. A future
caller can checksum or sample the buffer, then publish it through one of the
existing display paths.

Possible present paths already visible in the tree:

- Buffer-first capture/publication: `crate::gfx::publish_screenshot_rgba_buffer`.
- Copy/present as overlay: `crate::intel::present_rgba_overlay_top_right`.
- Direct framebuffer/primary path: possible only after explicitly owning pitch,
  format, GGTT mapping, flush, and scanout re-arm rules from `src/intel/display.rs`.

The direct framebuffer route should stay last. The display code already treats
scanout handoff as separate from proving that the render backend actually wrote
the pixels.

## Not Yet Implemented

- No shader artifact bytes are embedded.
- No fragment pipeline is created.
- No compute shader path is available through `trueos_gfx_core::ShaderStage`
  today; the enum only exposes vertex/fragment stages.
- No GPGPU matvec path is touched.
- No task is spawned unless a future caller explicitly invokes
  `spawn_mandelbrot_gpu_sidequest`.
