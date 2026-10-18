# Microfont offline mesh tools

The `microfonttothemoon` boot export service has been removed. These host tools
and generated assets remain available for offline inspection; the kernel does
not load their meshes, recipe or catalogs.

The replacement `src/r/services/microfont_raster_preview.rs` draws the crate's
precomputed Block Elements quads once into a service-owned UI4 frame. It uses a
744×88 image at 4× scale, with two rows in Unicode order and a blank cell between
glyphs. All 32 blocks are included, including the three shades. Escape closes the selected preview window; its
service then retires the frame. There is no PNG encoding, archive, filesystem
access, CPU meshing or readback.

A separate `src/r/services/boxdraw_raster_preview.rs` boot service opens a second
744×352 frame above the blocks. Its eight rows retain all 128 Unicode slots;
124 glyphs have quad geometry and the four rounded-corner slots stay blank.
Each precomputed four-corner quad becomes two triangles, rendered directly into
an immutable single-buffer UI4 scene. Diagonals retain their slanted edges.
The service owns its window, publishes once, and retires it after Escape closes
the selected frame. It performs no file writes, PNG encoding or readback.

For optional offline mesh generation:

```sh
node tools/microfonttothemoon/generate.mjs
mkdir -p bld/microfonttothemoon
rustc --edition=2024 -Awarnings tools/microfonttothemoon/export_han.rs -o bld/microfonttothemoon/export_han
bld/microfonttothemoon/export_han > bld/microfonttothemoon/han-masks.txt
node tools/microfonttothemoon/generate.mjs 24 src/intel/gpu_font/moon/han han
```

The generator accepts detail overrides 6, 12, 24 or 40. Kernel builds need
neither Node nor generated geometry.
