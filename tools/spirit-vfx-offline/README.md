# Spirit VFX offline OpenCL comparison grid

This userspace program plays the production Spirit VFX shaders without booting
TRUEOS. It is a GPU replay, not a CPU rewrite:

1. `7z` extracts all seven frames of the fixed `idle.crossed.soft_blink` asset
   directly from `tools/Lilly.7z`.
2. The host OpenCL compiler consumes the production
   `spirit_vfx_background_rgba8.cl` and `spirit_vfx_sprite_rgba8.cl` sources.
3. Every display tick renders the nine selected procedural backgrounds in
   stable ID order, 2 through 10.
4. Each cell uses the kernel's `256x256`, local `16x1` dispatch and exact
   32-dword control page.
5. Shader time advances at Spirit's 60 Hz target while Lilly independently
   advances one asset frame every 110 ms, exactly as recorded in
   `tools/Lilly.catalog`.
6. One centered, borderless 768x768 ARGB window presents the nine
   premultiplied cursor surfaces in a three-column grid. Escape or the window
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

A static PNG of the same nine-cell grid remains available for comparisons:

```sh
make -C tools/spirit-vfx-offline render TIME=2.25
```

The host dependencies are an OpenCL ICD loader/runtime, `libpng`, `7z`, and
X11/Xrender (Xwayland works). The program carries the small subset of OpenCL
1.2 declarations it uses, so a separate OpenCL header package is not required.
