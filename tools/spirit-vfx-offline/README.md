# Spirit production VFX preview

This host tool previews the two maintained Spirit C++ for OpenCL artifacts
without booting TRUEOS. It contains no shader implementation and has no legacy
OpenCL-C fallback:

- background:
  `artifacts/adls/cpp/spirit_vfx_background_rgba8.spv`
- sprite:
  `artifacts/adls/cpp/spirit_vfx_sprite_rgba8.spv`

The Intel host OpenCL runtime consumes those checked-in SPIR-V images through
`clCreateProgramWithIL`. TRUEOS consumes the sibling audited Zebin images
generated from the same C++ sources. Consequently the preview and bare-metal
paths share the shader source, mode IDs, argument ABI, and control-page ABI;
only their submission environments differ.

The panel renders background IDs 2 through 11 in a five-column, two-row grid
at 60 Hz. Every cell uses the live fixed 256x256 allocation, Lilly scale
`0.65`, background presentation scale `1.171875`, and global animated
AuraBloom sprite layer. Opacity remains `1.00` so every background can be
reviewed. Left/Right adjusts the shared background Speed and Down/Up adjusts
Intensity, both in `0.01` steps.

Lilly uses all seven frames of `idle.crossed.soft_blink`, extracted directly
from `tools/Lilly.7z`, and advances every 110 ms independently of shader time.
Escape or the window close action exits.

From the repository root:

```sh
make -C tools/spirit-vfx-offline run
```

A static PNG of the same production-artifact grid is also available.
`MagicTimeCircle` is fixed at `10:09:42 UTC` unless an explicit
seconds-of-day value is supplied:

```sh
make -C tools/spirit-vfx-offline render TIME=2.25
bld/tools/spirit_vfx_offline --render-grid bld/magic-time-43.png 1.5 36583
```

The host dependencies are an OpenCL ICD loader/runtime with SPIR-V IL support,
`libpng`, `7z`, X11, and Xrender. The program carries the small subset of
OpenCL declarations it needs, so OpenCL development headers are not required.
