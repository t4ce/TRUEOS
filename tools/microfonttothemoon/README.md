# Microfont to the moon

At boot, `src/r/services/microfonttothemoon.rs` constructs glyph meshes on
the CPU from the existing Microfont crate, rasterizes them with the iGPU,
and writes one file to a writable TRUEOSFS root:

`microfonttothemoon-8x.tar.lz4`

The archive contains eight PNGs:

- `microfonttothemoon-8x-1.png` through `microfonttothemoon-8x-4.png`
- `microfonttothemoon-han-8x-1.png` through `microfonttothemoon-han-8x-4.png`

Each PNG is 2560×1152. A source unit is 8 pixels; a normal 6×11 cell
occupies 48×88 pixels. The original 32-column layout is split at whole-tile
boundaries into four parts. Parts 1/2 are top left/right and 3/4 bottom
left/right. The map and keyboard retain their 18×11 and 12×11 source sizes.
No individual PNGs are written to disk by this service.

Extract with a standard LZ4 and tar implementation:

```sh
lz4 -dc microfonttothemoon-8x.tar.lz4 | tar -xf -
```

## Runtime CPU meshing

The studio set contains all 224 packed CP850 atlas slots, 256 Braille
patterns, 32 blocks and two icons, in that order. The Han set contains the
512 GUI Han entries, including punctuation, in ascending Unicode order.
Masks and codepoints come directly from the crate's packed tables and
public accessors. No second mask table is embedded by this service.

The CPU mesher ports the studio's neutral-bias Convolved membrane:
finite-range Gabriel links, eased reach cutoff, midpoint quadrature,
endpoint caps, density normalization, Float32 field samples, conservative
quadtree merging, shared-edge conformity, threshold clipping and welded
indexed triangles. It builds each image part from source masks on the
blocking service lane, then uploads its geometry to the existing renderer.
The supported model is the currently selected neutral membrane/general
field configuration; unsupported models or directional bias are rejected.

The shared `recipe.json` supplies the kernel's small embedded default
configuration. Current settings are reach 1.70, influence width 0.20,
threshold 0.66, density balance 66%, neutral bias and High detail 24.
`long_reach: ["/"]` selects reach 2.54 for the slash. Selection happens
before meshing and both profiles share the usual image draw. Changing the
recipe requires a kernel rebuild, but no host mesh-generation step.

There are no `.mesh`, catalog, or generated `config.rs` inputs in the
kernel service. The optional host generator and its old meshes remain a
debug oracle. They are not compiled into the runtime service. The renderer
still uses native triangle shaders and 4-sample antialiasing, independently
of the 8× raster scale.

`cpu_mesh_us` measures CPU construction from crate masks. `generation_us`
measures construction, upload, rendering and completed RGBA8 readback.
Both exclude PNG encoding, tar/LZ4 packaging and filesystem storage.
The service packs and compresses the PNGs on a blocking lane using the
existing POSIX/PAX tar and standard LZ4-frame encoders. The BSP performs
only the final archive write; disk-write retries do not regenerate images.
Queue or render admission rejection may retry; an accepted, incomplete
GPU pass is never replayed or read back. A failed image aborts the export
instead of saving a partial archive.

## Checks and optional debug generation

The runtime CPU mesh test compares all eight image meshes, including all
1026 glyphs and both reach profiles, against the JS debug generator. It
checks vertex positions, index streams, glyph identities and layout.
Regenerate the oracle only when the recipe or source font changes:

```sh
node tools/microfonttothemoon/generate.mjs
mkdir -p bld/microfonttothemoon
rustc --edition=2024 -Awarnings tools/microfonttothemoon/export_han.rs -o bld/microfonttothemoon/export_han
bld/microfonttothemoon/export_han > bld/microfonttothemoon/han-masks.txt
node tools/microfonttothemoon/generate.mjs 24 src/intel/gpu_font/moon/han han
python3 tools/testpy/test_microfont_runtime_mesh.py
python3 tools/testpy/test_lz4_archive.py
cargo build --offline
```

The debug generator accepts detail overrides 6, 12, 24 or 40. Its catalog
validator remains available for offline mesh diagnostics. Kernel builds
need neither Node nor generated geometry.

Look for `microfonttothemoon: rendered` timing records and one
`microfonttothemoon: saved` archive record, including the actual disk ID.
A host build/test does not prove the GPU ran or that the disk artifact
exists on the rig.
