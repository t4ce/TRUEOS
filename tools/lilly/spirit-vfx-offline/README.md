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

The panel renders background IDs 2 through 13 in a four-column, three-row grid
at 60 Hz. Every cell uses the live fixed 256x256 allocation, Lilly scale
`1.0`, zero rotation, no edge feather, reviewed per-effect background scales,
and the global animated AuraBloom sprite layer. Generic opacity is `1.00`; Codex knots use runtime opacity `0.75`.
Spark lattice uses speed 3 and the runtime seven-second intensity cycle 1 → 4 → 1;
Resonance uses speed 1 and intensity 2. The preview shows settled states;
it does not replay status-driven entry/exit transitions.
Left/Right adjusts shared Speed and Down/Up adjusts Intensity in `0.01` steps.
For Spark lattice these controls multiply its runtime values relative to the
shared defaults (speed 1, intensity 2).

Lilly uses the new LilyDS set from `../Lily/LilyDS` (override with
`LILLY_SOURCE=/path/to/LilyDS`). `prepare_lilly.py` resolves
`idle.crossed.soft_blink` through `tools/Lilly.catalog` and `tools/Lilly.sources`
to `Idle_up/Crossed-Arms/idle-1/frame_02..08.png`. It restores cropped PNGs
using `alignment.json` onto transparent 256×256 canvases in `bld/lilly-preview`.
The seven frames advance every 110 ms independently of shader time.
Run `make ... assets` to refresh these frames when launching the binary directly.
Escape or the window close action exits.

From the repository root:

```sh
make -C tools/lilly/spirit-vfx-offline run
```

A static PNG of the same production-artifact grid is also available.
`MagicTimeCircle` is fixed at `10:09:42 UTC` unless an explicit
seconds-of-day value is supplied:

```sh
make -C tools/lilly/spirit-vfx-offline render TIME=2.25
bld/tools/spirit_vfx_offline --render-grid bld/magic-time-43.png 1.5 36583
```

The host dependencies are an OpenCL ICD loader/runtime with SPIR-V IL support,
`libpng`, Python 3 with Pillow, X11, and Xrender. The program carries the small subset of
OpenCL declarations it needs, so OpenCL development headers are not required.

Each numbered cell has its own scale slider below the untouched 256×256 stage.
Drag to adjust from 0.25 to 3.00 in 0.01 steps. Press **S** to save all twelve
values to `bld/spirit-vfx-scales.txt` (shader ID followed by scale). The panel
and PNG renderer reload that file at startup. The built-in values match the reviewed runtime defaults; additional slider changes
remain preview overrides until reviewed. Runtime transitions fade opacity at fixed size.
