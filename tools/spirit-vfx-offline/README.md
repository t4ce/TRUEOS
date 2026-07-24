# Spirit VFX offline OpenCL comparison grid

This userspace program plays the production Spirit VFX shaders without booting
TRUEOS. It is a GPU replay, not a CPU rewrite:

1. `7z` extracts all seven frames of the fixed `idle.crossed.soft_blink` asset
   directly from `tools/Lilly.7z`.
2. By default, the host OpenCL compiler consumes the retained
   `spirit_vfx_background_rgba8.cl` and `spirit_vfx_sprite_rgba8.cl` reference
   sources. Environment-selected C++ SPIR-V exercises the production repass.
3. Every display tick renders the ten selected procedural backgrounds in
   stable ID order, 2 through 11. ID 11 is the C++ `MagicTimeCircle`; its cell
   also enables `AuraBloom`, matching Spirit's live Idle pairing.
4. Each cell uses the kernel's `256x256`, local `16x1` dispatch and exact
   33-dword control page. Dword 4 remains smooth 60 Hz animation time for the
   sprite pass; append-only dword 32 carries clock seconds for
   `MagicTimeCircle`.
5. Shader time advances at Spirit's 60 Hz target while Lilly independently
   advances one asset frame every 110 ms, exactly as recorded in
   `tools/Lilly.catalog`.
6. One centered, borderless 1280x512 ARGB window presents the ten
   premultiplied cursor surfaces in a five-column grid. Escape or the window
   close action exits the complete grid.
7. Background opacity is fixed at `1.00` and each effect retains its existing
   scale. The only runtime values are the HTML-matched `Speed` and `Intensity`
   controls, shared by all nine cells:
   - Left/Right decreases/increases Speed by `0.01` in `0.00..4.00`.
   - Down/Up decreases/increases Intensity by `0.01` in `0.10..2.50`.
   Both start at `1.00`; each accepted key step is printed to the terminal.

The tool prefers an Intel GPU platform when one is installed, but it can use
another conforming GPU OpenCL platform.

From the repository root:

```sh
make -C tools/spirit-vfx-offline run
```

A static PNG of the same ten-cell grid remains available for comparisons. Its
`MagicTimeCircle` cell is fixed at `10:09:42 UTC` for reproducible review:

```sh
make -C tools/spirit-vfx-offline render TIME=2.25
```

The executable also accepts an explicit seconds-of-day value after the normal
animation time, which makes adjacent one-second clock steps directly
comparable:

```sh
bld/tools/spirit_vfx_offline --render-grid bld/magic-time-42.png 1.5 36582
bld/tools/spirit_vfx_offline --render-grid bld/magic-time-43.png 1.5 36583
```

Set `SPIRIT_VFX_BACKGROUND_SPV` and `SPIRIT_VFX_SPRITE_SPV` to replay the
published C++ for OpenCL artifacts through `clCreateProgramWithIL` instead of
compiling the legacy sources. This is the visual-review lane used for the
Spirit C++ repass:

```sh
SPIRIT_VFX_BACKGROUND_SPV=crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_background_rgba8.spv \
SPIRIT_VFX_SPRITE_SPV=crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/spirit_vfx_sprite_rgba8.spv \
make -C tools/spirit-vfx-offline render
```

The host dependencies are an OpenCL ICD loader/runtime, `libpng`, `7z`, and
X11/Xrender (Xwayland works). The program carries the small subset of OpenCL
1.2 declarations it uses, so a separate OpenCL header package is not required.
