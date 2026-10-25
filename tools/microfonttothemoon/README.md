# Microfont offline mesh tools

The `microfonttothemoon` boot export service has been removed. These host tools
and generated assets remain available for offline inspection; the kernel does
not load their meshes, recipe or catalogs.

The UI4 font previews have also been removed. The runtime now warms the private
home-button glyph first, then one shared line/quad R8 alphabet at 7.5mm / 2.75px.
See [the retained font service](../microfont-mask/README.md).

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
