# Retained Microfont coverage atlas

`src/r/services/microfont_mask_service.rs` owns one set of GPU geometry buffers
for the boot lifetime. The one-time full-font request uploads the box/block quads, line strips with adjacency, and width-independent
join recipes. Consumers copy the retained coverage without further GPU production.
Rounded box corners retain the bounded 4–16 curve-quad variants. The default
physical size selects one combined corner batch; the others are not drawn.
Endpoint frames, tags and join modes also remain in GPU constant storage.

UI4's start button uses only the private `homebutton.bin` line asset
(`0x110200`). The service renders its independent 50mm R8 mask once, publishes
readiness, and retains the pixels for recoloring. Its temporary geometry and
GPU mask are released after readback; it occupies no shared font-atlas slot.
Missing or invalid home-button data fails without a Unicode/bitmap/`§` fallback.

After home-button readiness is published, `microfont-warm` prepares the shared
alphabet through the same serialized path as Shell3. It renders all available
line and quad characters once at **7.5mm em / 2.75px stroke**, reads back R8 tiles,
and retains that immutable snapshot for the service lifetime. R8 stores coverage:
zero outside ink, 255 inside solid ink, and fractional values at antialiased edges.
Shell3 caches tinted RGBA cells for CPU/BLT copies; UI4 menus, drag labels, LINK,
and the USB/IP status surface copy from the same R8 source. Early USB status is
retained and repainted when the atlas becomes ready.

TRUEOS enables only MicroFont's `raster` feature (`default-features = false`).
Its packed font asset contains line/quad data, without the hex bitmap/stamp path.
Both UI4 preview windows and their boot tasks have been removed.

Atlas dimensions and default scale follow the supported repertoire. The internal
atlas uses 20 columns to keep packed line-join coordinates within their seven-bit
X / eight-bit Y bounds.
The quad shader derives the column count from the target width and physical cell
pitch, including one-column standalone masks. The atlas stores:

| Slots | Source |
| --- | --- |
| 0–94 | Basic Latin U+0020–U+007E, authored line bins and the `|`/`/` strips |
| 95–222 | Box Drawing U+2500–U+257F, quads |
| 223–254 | Block Elements U+2580–U+259F, quads |
| 255 onward | Extra line characters, including shared lowercase compositions |
| After extra line characters | Extra quad symbols, including `▬` and `■` |
| After extra quad symbols | All 256 Braille patterns |

Missing line bins stay empty. Rounded `╭ ╮ ╯ ╰` tiles use binary kind-6 recipes;
their one-source-unit stroke matches the existing straight box-drawing tiles.
Physical display metrics supply the 10mm reference for a 4-at-10mm / 16-at-40mm
curve floor. Outer-arc pixel error may raise that count, with a hard cap of 16;
when physical metrics are unavailable it alone supplies the budget.
Cargo discovers mask-first bins and packs the selected representations into one
font image. TRUEOS embeds only its tar/LZ4 archive and installs the decoded image
after heap setup, before font consumers start. Runtime source geometry is
immutable. Mask dimensions and storage follow `AtlasConfig` and
`SLOT_COUNT` rather than a fixed byte count.

Shell3's frame, painting, scrolling and pointer mapping share the default physical
cell dimensions. Font-scale controls zoom relative to the 7.5mm default. Gridpape
keeps its 5mm size steps and scales the same retained R8 tiles at every size;
it no longer warms additional alphabets or individual large characters. Area
filtering keeps narrow strokes visible when a consumer shrinks a tile.

Build normally with `make iso`. Both shaders live in `vendor/microfont/shaders/`;
the Make dependency checks/bakes the native artifacts automatically.

Validation: `tools/testpy/test_microfont_mask_service.py` tests the production
service at mocked GPU/frame boundaries, including upload lifetime, busy retries,
failed retirement, join expansion, one-time publication, coverage copying and
small-size sampling. `test_shell3_font_service.py` checks RGBA caching and tinting;
`test_start_button_microfont.py` checks home-button signaling. `test_microfont_line_end_shader.py` checks the final VS/PS
constant packets, native shader ABI and Make integration. Physical evidence is
recorded in `bld/microfont-mask-experiments/2026-10-20/`.
