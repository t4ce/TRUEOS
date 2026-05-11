# Mandelbrot Fragment Side-Test Artifact

## Placement

This is intentionally isolated from the GPGPU matvec path and from the main
triangle pipeline wiring. I placed it under `.codex_tmp/` because the repo's
checked-in fragment bake helper is currently centered on the simple triangle
artifact, while `.codex_tmp/` already holds exploratory shader sources and proof
logs.

## Shader

- Source: `.codex_tmp/mandelbrot_fragment_1440p_parametric.frag`
- SPIR-V: `.codex_tmp/mandelbrot_fragment_1440p_parametric.spv`
- Stage: Vulkan GLSL fragment shader, `#version 450`
- Output: `layout(location = 0) out vec4 out_color`
- Coordinate source: `gl_FragCoord.xy`

## Resolution Handling

The shader is resolution-parametric. Resolution is passed through a fragment
push-constant block:

```glsl
layout(push_constant) uniform MandelbrotParams {
    layout(offset = 0) vec2 resolution;
    layout(offset = 8) vec2 center;
    layout(offset = 16) float scale;
    layout(offset = 20) uint max_iterations;
} pc;
```

For a 1440p render target, set `resolution = vec2(2560.0, 1440.0)`. The mapping
uses `resolution.y` as the scale denominator, so aspect ratio is preserved and
the same shader can cover smaller or larger targets without changing source.

The shader includes defensive defaults if a side-test runner supplies zeroed
push constants:

- `resolution = vec2(2560.0, 1440.0)`
- `center = vec2(-0.5, 0.0)`
- `scale = 2.6`
- `max_iterations = 192`

The SPIR-V was emitted with:

```sh
glslangValidator -V .codex_tmp/mandelbrot_fragment_1440p_parametric.frag \
  -o .codex_tmp/mandelbrot_fragment_1440p_parametric.spv
```

The boot sidequest embeds the SPIR-V bytes and logs their size/signature. That
proves the artifact is present and placeable as a `ShaderDesc`; it still does
not prove Intel fragment submit or EU-side presentation until the sidequest is
wired to an actual Intel render path.
