# Microfont to the moon

The boot service `src/r/services/microfonttothemoon.rs` renders two grayscale
inspection sheets through the existing kernel-owned native triangle pipeline,
then writes `microfonttothemoon-4x.png` and `microfonttothemoon-han-4x.png` to a writable TRUEOSFS root. It waits for
GuC, Render0 and filesystem readiness. GPU work and PNG encoding run on a
blocking service lane; the BSP performs the asynchronous filesystem write.
Only rejected queue/render admission and disk writes retry. An accepted,
incomplete GPU pass is never replayed or read back.

The sheet contains 224 packed Microfont slots (byte 0x20 through 0xFF),
256 Braille patterns, 32 blocks and the two studio icons, in that order.
There are 32 columns of 80×64 inspection tiles. Each source unit is 4 pixels:
a normal 6×11 cell occupies 24×44 pixels; the map and keyboard keep their
18×11 and 12×11 source dimensions. The complete PNG is 2560×1088.
The catalog records the slot, character, route and mesh statistics.
The second sheet contains all 512 entries of the vendored GUI Han subset,
including its punctuation, in ascending Unicode order. It uses the same
4× dimensions and Draft tissue/triangle renderer as the first sheet.
The generator obtains masks by compiling and running the vendored Rust
decoder itself, preserving all 66 source bits.

## Actual Draft tessellation

Geometry comes from the HTML studio's pure `buildShape` functions. The
build tool executes those functions in an isolated JavaScript context with
`detail=6`, the studio's lowest Draft setting, and the shared `recipe.json`
parameters and forced general-field routing. This changes the generated
vertices/triangles: general-field sampling spacing is 1/6 source unit.
All source masks, including Braille, use the general-field route. It does
not draw diagnostic mesh lines.

This stage fixes tessellation at **build time**. The kernel embeds vector
geometry, uploads it once, and performs the raster pass at boot. No RGBA
atlas, font cache, font warmup, or Shell3 routing is added. It reuses the
native triangle VS/constant-RGBA PS, with white draw color and black clear;
it does not introduce a procedural per-pixel replacement for the mesh.
The renderer's 4-sample antialiasing is separate from the sheet's 4× scale.

Regenerate and validate from the repository root:

```sh
node tools/microfonttothemoon/generate.mjs 6
mkdir -p bld/microfonttothemoon
rustc --edition=2024 -Awarnings tools/microfonttothemoon/export_han.rs -o bld/microfonttothemoon/export_han
bld/microfonttothemoon/export_han > bld/microfonttothemoon/han-masks.txt
node tools/microfonttothemoon/generate.mjs 6 src/intel/gpu_font/moon/han han
python3 tools/microfonttothemoon/validate.py
python3 tools/microfonttothemoon/validate.py src/intel/gpu_font/moon/han
cargo build --offline
```

Compare Fine in a separate output directory:

```sh
node tools/microfonttothemoon/generate.mjs 12 /tmp/moon-fine
python3 tools/microfonttothemoon/validate.py src/intel/gpu_font/moon /tmp/moon-fine
```

Supported detail values match the studio: 6, 12, 24, 40. The generated
`config.rs` keeps boot diagnostics synchronized with the baked geometry.
The validator checks content hashes, vertex bounds, triangle winding,
index validity and actual sample spacing. Float32 quantization can collapse
microscopic threshold slivers; the catalog records removed faces and any
winding normalization. No geometry is rasterized by the build tool.

On a booted machine, look for the `microfonttothemoon: saved` log. It reports
the actual disk ID and file path. A successful host build is not evidence
that the GPU ran or that the disk artifact exists on the rig.

The shared recipe uses Directional tissue, reach 1.76, sigma 0.45, threshold
0.75, density balance 1.0, tangent stretch 1.75, neutral connection bias,
and zero diagonal assist/centerline snap. Shortcut pruning remains enabled;
cross-character links remain disabled. These values affect field geometry;
box, block and Braille source masks also use the forced general-field route. In Tissue mode,
stretch is `1 + 1.75 * local_coherence`, not a uniform 1.75× screen scaling.
