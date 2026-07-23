# Spirit Sprite shader OpenCL comparison grid

This folder maps the complete `preview.html` **3 · Sprite shader** collection
through the production `spirit_vfx_sprite_rgba8.cl` kernel. It is not a CPU
approximation and contains no duplicate shader math.

The renderer:

1. extracts Lilly's fixed `idle.crossed.soft_blink` frame from
   `tools/Lilly.7z`;
2. compiles the production OpenCL source on the host GPU;
3. dispatches stable Sprite shader IDs 0 through 15 with their exact demo
   defaults and colors;
4. writes the sixteen premultiplied 256x256 results into one transparent 4x4
   PNG grid.

From the repository root:

```sh
make -C tools/spirit-sprite-vfx-offline render
```

Set a different animation time or output path when comparing temporal modes:

```sh
make -C tools/spirit-sprite-vfx-offline render \
  TIME=0.75 OUTPUT=bld/spirit-sprite-vfx-grid-075.png
```

The dependencies are an OpenCL ICD loader/runtime, `libpng`, and `7z`. The
program embeds the minimal OpenCL 1.2 declarations it needs, so OpenCL headers
are not required.

The grid is row-major:

| Row | Column 1 | Column 2 | Column 3 | Column 4 |
| --- | --- | --- | --- | --- |
| 1 | Original / clean | Aura bloom | Neon edge | Fire rim |
| 2 | Ice shimmer | Hologram | RGB glitch | Dissolve |
| 3 | Ghost trail | Electric arc | Rainbow prism | Hit flash |
| 4 | Pixel wave | Toon ink | Liquid warp | Dream bloom |
