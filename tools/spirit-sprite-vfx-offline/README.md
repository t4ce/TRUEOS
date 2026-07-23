# Spirit Sprite shader OpenCL comparison grid

This folder maps the complete `preview.html` **3 · Sprite shader** collection
through the retained `spirit_vfx_sprite_rgba8.cl` reference or the production
C++ SPIR-V. It is not a CPU approximation and contains no duplicate shader
math.

The live renderer:

1. extracts all seven frames of Lilly's fixed `idle.crossed.soft_blink` asset
   from `tools/Lilly.7z`;
2. compiles the production OpenCL source on the host GPU;
3. dispatches stable Sprite shader IDs 0 through 15 with their exact demo
   defaults and colors;
4. leaves background ID zero in every control page, so the sprite kernel
   starts from transparent pixels and no procedural-background pass runs;
5. animates all sixteen premultiplied 256x256 results beside a compact control
   panel in one centered, borderless 1344x1024 ARGB window at Spirit's 60 Hz
   target. Lilly advances independently every 110 ms. Escape or the window
   close action exits.

The control panel deliberately exposes generic `Param 1` through `Param 4`
instead of the effect-specific browser labels. All four start centered, which
preserves each effect's authored default. Moving a shared control toward either
edge maps every effect toward that parameter's own original browser minimum or
maximum and retains its original step size.

Colors A and B initially remain per-effect, preserving the authored palettes.
Clicking either swatch opens a color picker and turns only that color into a
shared override across all sixteen cells. `Reset defaults` restores the four
authored parameter defaults and both per-effect palettes.

From the repository root:

```sh
make -C tools/spirit-sprite-vfx-offline run
```

A static transparent PNG grid remains available. Set a different animation
time or output path when comparing temporal modes:

```sh
make -C tools/spirit-sprite-vfx-offline render \
  TIME=0.75 OUTPUT=bld/spirit-sprite-vfx-grid-075.png
```

The published C++ repass can be replayed directly as SPIR-V on the Intel host
driver, without recompiling the OpenCL C source:

```sh
SPIRIT_VFX_SPRITE_SPV=crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.spv \
make -C tools/spirit-sprite-vfx-offline render
```

The dependencies are an OpenCL ICD loader/runtime, `libpng`, `7z`,
X11/Xrender (Xwayland works), and `zenity` for the two color pickers. The
program embeds the minimal OpenCL 1.2 declarations it needs, so OpenCL headers
are not required.

The grid is row-major:

| Row | Column 1 | Column 2 | Column 3 | Column 4 |
| --- | --- | --- | --- | --- |
| 1 | Original / clean | Aura bloom | Neon edge | Fire rim |
| 2 | Ice shimmer | Hologram | RGB glitch | Dissolve |
| 3 | Ghost trail | Electric arc | Rainbow prism | Hit flash |
| 4 | Pixel wave | Toon ink | Liquid warp | Dream bloom |
