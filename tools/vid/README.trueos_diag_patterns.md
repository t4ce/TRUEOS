TRUEOS H.264 diagnostic pattern assets live here.

Files:
- `make_trueos_diag_pattern.py`: emits a native 2560x1440 coded PPM test image by default.
- `make_trueos_diag_pattern.sh`: wraps the image generator and encodes a single-IDR H.264 MP4.

Pattern variants:
- `mbgrid`: unique flat color per 16x16 macroblock plus `RxxCyy` labels.
- `luma`: grayscale-only macroblock grid to isolate luma geometry from chroma issues.
- `uv`: constant luma with U as a horizontal ramp and V as a vertical ramp.

Default host-side build:

```bash
tools/vid/make_trueos_diag_pattern.sh mbgrid
```

That emits:
- `tools/vid/trueos_h264_diag_mbgrid_2560x1440.ppm`
- `tools/vid/trueos_h264_diag_mbgrid_2560x1440.mp4`

To rebuild the old cropped 1080p diagnostic explicitly:

```bash
tools/vid/make_trueos_diag_pattern.sh mbgrid 1920 1088 1080
```

The generated image includes:
- 16x16 macroblock labels across the whole coded frame.
- A marked crop boundary when visible height is smaller than coded height.
- A bottom-center location hint for quick visual comparison.
