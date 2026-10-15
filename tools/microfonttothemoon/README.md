# Microfont to the moon

The boot service `src/r/services/microfonttothemoon.rs` renders four studio
images and four Han images through the same native triangle pipeline, then
writes them to a writable TRUEOSFS root:

- `microfonttothemoon-8x-1.png` through `microfonttothemoon-8x-4.png`
- `microfonttothemoon-han-8x-1.png` through `microfonttothemoon-han-8x-4.png`

Each image is 2560×1152. Every source unit is 8 pixels, so a normal 6×11
cell occupies 48×88 pixels. The map and keyboard keep their 18×11 and
12×11 source dimensions. The original 32-column sheet order is retained,
with 160×128 inspection tiles. The sheet is split into four quadrants at
tile boundaries, padded to 18 rows so no glyph is cut between images.
Parts 1 and 2 are top left/right; parts 3 and 4 are bottom left/right.
Each part fits the existing Render0 target and uses one geometry pass.

The studio sheet contains 224 packed Microfont slots (byte 0x20 through
0xFF), 256 Braille patterns, 32 blocks and the two studio icons, in that
order. The Han sheet contains all 512 entries of the vendored GUI Han
subset, including punctuation, in ascending Unicode order. The generator
obtains Han masks by compiling and running the vendored Rust decoder,
preserving all 66 source bits. Each root catalog lists its four parts;
part catalogs record glyph positions and mesh statistics.

The service waits for GuC, Render0 and filesystem readiness. GPU work and
PNG encoding run on a blocking service lane; the BSP performs the async
filesystem write. Only rejected queue/render admission and disk writes
retry. An accepted, incomplete GPU pass is never replayed or read back.

## Actual Draft tessellation

Geometry comes from the studio's pure `buildShape` functions. The build
tool runs those functions with `detail=6` (Draft) and the shared
`recipe.json` parameters. All source masks use the general-field route,
with sample spacing 1/6 source unit. Increasing raster scale to 8× keeps
this tessellation detail; it does not add diagnostic lines or resample a
previous PNG.

The shared recipe uses Directional tissue, reach 1.76, sigma 0.45,
threshold 0.75, density balance 1.0, tangent stretch 1.75, neutral connection
bias, and zero diagonal assist/centerline snap. Shortcut pruning remains
enabled; cross-character links remain disabled. In Tissue mode, stretch
is `1 + 1.75 * local_coherence`.

Tessellation is fixed at build time. The kernel embeds vector geometry,
uploads each part once, and rasterizes it at boot using the native triangle
VS/constant-RGBA PS, white draw color and black clear. No RGBA atlas,
font cache, font warmup, or Shell3 routing is added. The renderer's
4-sample antialiasing is separate from the sheets' 8× scale.

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

Supported detail values match the studio: 6, 12, 24, 40. Generated
`config.rs` files supply all eight kernel requests and their diagnostics.
The validator checks content hashes, bounds, winding, indices, spacing,
tile containment, and complete glyph coverage across all four parts.
Float32 quantization can collapse microscopic threshold slivers; catalogs
record removed faces and any winding normalization. The build tool does
not rasterize geometry.

On a booted machine, look for `microfonttothemoon: saved` logs containing
the actual disk ID and each image path. A host build does not prove the
GPU ran or that files exist on the rig.
