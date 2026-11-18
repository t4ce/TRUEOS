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

The background panel and one-shot capture render only the background kernel.
They do not load LilyDS or the sprite kernel:

```sh
make -C tools/lilly/spirit-vfx-offline panel-background
make -C tools/lilly/spirit-vfx-offline capture-background PREFIX=bld/spirit-vfx-preview
```

The capture uses the reviewed scales and default controls, with animation time
1.5 s (frame 90 at 60 Hz) and clock time 10:09:42 UTC. Supply `TIME` and `CLOCK`
to sample another frame. Both headless background targets use the reviewed
scales even if the interactive panel has saved local overrides.

| Output | Contents |
| --- | --- |
| `PREFIX.controls` | 12 consecutive 33-dword little-endian control pages for IDs 2 through 13 |
| `PREFIX.bgra` | 12 consecutive 256×256 cells in the same order, copied directly as premultiplied BGRA8, top-down, 1024 bytes per row |
| `PREFIX.png` | The same cells in a 4×3 grid, with straight RGBA and transparency |

Pass the control pages to the on-demand `lilyvfx` Blueprint in
`TRUEOS-Blueprints/apps/lilyvfx`. The native capture runs one background
walker per effect on the TRUEOS execution lane with a private render target.
It copies the pixels only after GPU retirement. The comparator composites both
sides against the same backdrop and reports raw per-channel differences
including alpha.

```sh
python3 tools/lilly/spirit-vfx-offline/compare_backgrounds.py \
  bld/spirit-vfx-preview.bgra renderer.bgra \
  --output bld/spirit-vfx-comparison.png
```

The host dependencies are an OpenCL ICD loader/runtime with SPIR-V IL support,
`libpng`, X11, and Xrender. Asset preparation and the comparison script use
Python 3 with Pillow.

Each numbered cell has its own scale slider below the untouched 256×256 stage.
Drag to adjust from 0.25 to 3.00 in 0.01 steps. Press **S** to save all twelve
values to `bld/spirit-vfx-scales.txt` (shader ID followed by scale). The panels
and composite PNG renderer reload that file at startup. The built-in values
match the reviewed runtime defaults. Slider changes apply to the preview
until reviewed. Runtime transitions fade opacity at fixed size.
