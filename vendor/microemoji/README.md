# microemoji

Microemoji reads a borrowed binary of filled color glyph outlines. Like
[microfont](../microfont), it streams SVG to `core::fmt::Write` and offers line
geometry for a renderer. The library is `#![no_std]` and needs no allocator or
dependencies. The TrueType reader runs in the host exporter.

## Export the initial set

From the TRUEOS root, with a sibling TRUEOS-Blueprints checkout, run this on
Ubuntu with the host Rust toolchain.

```sh
cargo +1.95.0 run --target x86_64-unknown-linux-gnu -p microemoji --example export -- \
  ../TRUEOS-Blueprints/apps/lico/assets/twemoji.ttf tgt/microemoji-export \
  --selection vendor/microemoji/selection.txt
```

The exporter writes `microemoji.bin` and one standalone SVG per selected
scalar. Use a fresh or empty output directory. The starting selection contains
the first 256 COLR-backed single-codepoint glyphs in Unicode order. Edit
`selection.txt` to hand-select glyphs with one `U+HEX` entry per line. Blank
lines and `#` comments are accepted. Omitting `--selection` uses the first 256
directly. Pass the font path followed by `--list-default` to print that list.

The host exporter reads COLR v0 with the first CPAL palette. The bundle indexes
individual Unicode scalars. Source glyphs composed through font shaping, such
as ZWJ sequences and country flag pairs, are outside this format.

The supplied Twemoji Mozilla font contains artwork by Twitter, Inc. and other
Twemoji contributors, licensed under CC BY 4.0. The exporter preserves the
outlines and palette as a binary and SVG representation.
[Mozilla font notice](https://github.com/mozilla/twemoji-colr/blob/v0.7.0/LICENSE.md) ·
[Twemoji graphics license](https://github.com/twitter/twemoji/blob/v14.0.2/LICENSE-GRAPHICS)

## Use the binary

Call `Bundle::parse(bytes)` once to validate the image. Look up scalars with
`bundle.glyph(ch)` or visit the whole index with `bundle.glyphs()`. The
`svg::write_glyph` helper emits a standalone document with a viewBox that
includes the line metrics and outline bounds. The `svg::write_fragment`
helper emits an embeddable group positioned relative to the baseline.

For line rasterization, visit `glyph.layers()` in order. Construct
`raster_line::LayerEdges::new(layer, glyph.metrics(), height_px)` with the
desired pixel height of the ascender-to-descender span. It emits directed
edges in font units with Y up. Fill each layer as a single nonzero-winding path
and composite its RGBA paint before moving to the next layer. Curves subdivide
toward a quarter-pixel tolerance with a size-based cap of 4 to 16 segments per
curve. Line commands and contour closures preserve their endpoints.

## R8 coverage in TRUEOS

`raster_mask::scan_layers` fills flattened contours by nonzero winding and
emits spans on a 4×4 sample grid. `raster_mask::rasterize` composites layer
alpha at each sample and averages the result into one byte per pixel. Both
use caller-provided scratch and center the glyph in the target using its SVG
bounds.

The kernel MicroEmoji mask service loads the
[alpha-only outline bundle](data/README.md) on first use. It shares the
existing blocking service lanes with MicroFont and retains a bounded cache
of R8 masks keyed by Unicode scalar and square tile size. For opaque layers
on supported Intel renderers, the service sends span quads through the
existing MicroFont R8 shader and averages its fenced readback. The CPU
producer uses the same span generator when that backend is unavailable.

UI4 font sprite requests select this service with
`UI4_FONT_SPRITE_MICROEMOJI` (font ID 5) and zero extra stroke. A cold
request reports busy while its job runs. Tint is applied at sprite creation
and is independent of the shared R8 mask.

In Shell3, choose **Microemoji** from the `font` menu. Its Matrix specimen
shows the bundled glyphs in Unicode order, 16 per row, fitted to the shell's
current cells.

## Binary format v1

All integers are little-endian. Offsets are absolute from the start of the
file, and records follow one another without padding.

| Record | Fields in order |
| --- | --- |
| Header (16 bytes) | `MEOJ`, version `u16` = 1, units per em `u16`, ascender `i16`, descender `i16`, glyph count `u32` |
| Index entry (16 bytes) | Unicode scalar `u32`, payload offset `u32`, payload length `u32`, horizontal advance `u16`, paint layer count `u16` |
| Paint layer | straight RGBA (4 bytes), path length `u32` in bytes, path commands |

Index entries are strictly ascending by scalar. Glyph payloads start
immediately after the index and follow index order. Each glyph contains one or
more paint layers ordered from back to front. A path contains one or more
closed contours and is filled using the nonzero winding rule. Each contour
begins with Move and ends with Close.

| Opcode | Command | Points |
| --- | --- | --- |
| 0 | Move | start |
| 1 | Line | endpoint |
| 2 | Quadratic | control, endpoint |
| 3 | Cubic | first control, second control, endpoint |
| 4 | Close | none |

An opcode is one byte. Each point is a signed 26.6 `i32` X followed by a
signed 26.6 `i32` Y. Coordinates are in font space with the baseline at
Y = 0 and positive Y up. Font metrics and advance are in whole font units.
