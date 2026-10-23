# Retained Microfont coverage atlas

`src/r/services/microfont_mask_service.rs` owns one set of GPU geometry buffers
for the boot lifetime, independently of either preview window. The first request
uploads the box/block quads, line strips with adjacency, and width-independent
join recipes. Later requests only change dispatch constants and native line width.
Endpoint frames, tags and join modes also stay in a retained GPU constant
buffer. Rebuilds write only width and transform fields; no fixed glyph data
is staged again.

`with_warmed_atlas(AtlasConfig, consumer)` ensures the single R8 atlas is ready
before calling the consumer. `CoverageAtlas::surface()` returns the coverage
surface; `glyph_rect(char)` identifies its pixel rectangle. The callback owns
exclusive access: complete all GPU/CPU reads before returning. Surface handles
must not be retained by asynchronous readers. The source is zero outside ink,
255 inside solid ink, and fractional at native-AA line edges, analytic round
ends, and join edges. Native AA coverage reaches R8 through white RGB and
SRC_ALPHA blending. Overlapping coverage follows the existing source-over rule.

Atlas dimensions and default scale follow the supported repertoire. The internal
atlas uses 20 columns to keep packed line-join coordinates within their seven-bit
X / eight-bit Y bounds; the CP850 demo independently uses 16 columns and 16 rows.
The quad shader derives the column count from the target width and physical cell
pitch, including one-column standalone masks. The atlas stores:

| Slots | Source |
| --- | --- |
| 0–94 | Basic Latin U+0020–U+007E, authored line bins and the `|`/`/` strips |
| 95–222 | Box Drawing U+2500–U+257F, quads |
| 223–254 | Block Elements U+2580–U+259F, quads |
| 255 onward | Extra line characters, including shared lowercase compositions |
| After extra line characters | All 256 Braille patterns |

Missing line bins and the four unsupported rounded box corners stay empty.
New bins enter through the existing discovery step in `make iso`; runtime source
geometry is immutable. Mask dimensions and storage follow `AtlasConfig` and
`SLOT_COUNT` rather than a fixed byte count.

`AtlasConfig` controls uniform glyph scale and pixel line width. Width is rounded
to the native 1/128px step. Scale is 1–23; width must be 1px through the scale in
pixels, keeping strokes within their cells. Identical normalized configuration
reuses the mask without GPU submission. Width changes redraw the same allocation;
size changes replace the mask allocation while retaining all geometry.

`stamp_preview` is the current CPU sink. Both existing UI4 specimens copy their
cells from this atlas, with the quad specimen retaining its 24x44 display size.
It accepts straight RGB/opacity plus a premultiplied background, producing
premultiplied RGBA. Transparent black background produces transparent tinted
stamps. Color and opacity changes do not invalidate the atlas. No preview writes
to disk and existing font consumers are unchanged.

Build normally with `make iso`. Both shaders live in `vendor/microfont/shaders/`;
the Make dependency checks/bakes the native artifacts automatically.

Validation: `tools/testpy/test_microfont_line_preview.py` tests the production
service at mocked GPU/frame boundaries, including upload lifetime, busy retries,
configuration caching, size changes, failed retirement, join expansion and actual
color/opacity copying. `test_microfont_line_end_shader.py` checks the final VS/PS
constant packets, native shader ABI and Make integration. Physical evidence is
recorded in `bld/microfont-mask-experiments/2026-10-20/`.
