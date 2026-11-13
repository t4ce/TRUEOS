# Spirit knot background studies

Four experimental working-state backgrounds, alongside the production background
and sprite replay tools. Build a self-contained, offline browser preview:

```sh
make -C tools/lilly/spirit-knot-vfx-offline run
```

Or use `make ... all` and open `bld/spirit-knot-preview.html` yourself. The build
needs Python 3 and `lz4`; the viewer needs WebGL 2, no server or dependencies.

The 2×2 grid, in reading order:

1. **Quiet current:** a gently breathing ribbon knot with a slow specular sweep.
2. **Signal runner:** six phase-offset packets circulating along the strands.
3. **Resonance:** outward distance contours with a restrained rocking motion.
4. **Spark lattice:** deterministic shimmering motes on and near the ribbons.

`knot.frag` is the entire background shader. Six rotated arc/tangent/diagonal
paths with cyclic undercuts construct a knot inspired by the OpenAI mark; it is
a geometric interpretation, not an exact reproduction of its vector artwork.
There are no logo textures, SVG paths, masks, fonts, noise textures, or particle
buffers. All four variants reuse the same distance calculation. The loop has
six fixed iterations; there is no ray marching or multi-octave noise.

The comparison uses native 256×256 pixels, premultiplied alpha, background scale
1.171875, speed 1, intensity 2, and opacity 1. Shared controls match Spirit's
two-color background inputs and limits. Opacity zero clears the effect;
speed zero freezes the background while Lilly's clock continues. Space pauses
both clocks. Save PNG exports the four current cells with labels and transparent
backgrounds. Light, dark, slate, and checker backdrops help check compositing.

Lilly is a **separate clean sprite layer**, at scale 0.65, using the same seven
`idle.crossed.soft_blink` frames and 110 ms cadence as the production replay.
Hide her to compare complete knots, or disable her animation. This experiment
does not approximate or alter the production AuraBloom sprite pass.

The GLSL ES shader is an offline design study. Selecting a variation precedes
porting it to production C++ for OpenCL and baking/auditing its artifacts. No
kernel code, production mode IDs, settings, or working-state selection changes
are made here.
