# Codex collapse into Lilly

UI4's compact tile is natively **64 × 64** (`collapse::Collapse::project`).
Other apps retain their usual movable compact tile. A live Codex Blueprint
(`codex.bp`, including its leased Shell3 terminal) instead folds into Lilly:

1. Finish the existing UI4 collapse animation, then wait 500 ms.
2. Slide the complete gray-backed tile from its current screen position toward
   Lilly's chest. The destination follows the applied 256 × 256 cursor plane.
   The slide takes 320–1100 ms depending on distance. Lilly can cross her arms
   during the slide; landing waits for the crossed pose.
3. For each pixel of the tile, UI4 draws the portion outside the cursor plane;
   Spirit draws the portion inside, over Lilly. Both use the same integer
   screen position and the same straight RGBA raster. UI4 premultiplies only
   its own portion for its frame format. The original window opacity is baked
   into both halves once, so translucent windows agree across the boundary.
4. On landing, blend the native tile into the packed chest stamp over 100 ms,
   then play the selected Wish C sequence, ending in crossed static. The
   authored stamp has no gray backdrop; the procedural arriving tile does.
5. Keep the compact Codex window hidden. A primary click anywhere in the
   existing radius-150 Lilly/Spirit dock circle restores and focuses it in
   its original placement. This uses the same live circle as docking. Existing
   drag/drop, menus and resize gestures take precedence. Movement beyond the
   existing click tolerance, another button, or denied cursor authority cancels
   restoration. If multiple Codex windows were folded, restore the latest first.

The window's original producer frame, geometry and interaction stay owned by
UI4. Restoration, close, icon replacement and owner cleanup cancel the retained
handoff. No new buildin or Blueprint ABI is introduced.

## Ownership and presentation

`src/ui4/codex_handoff.rs` identifies the live Blueprint and publishes the
outside portion as an immutable UI4 frame. `src/spirit/codex_handoff.rs` owns
the slide and Wish C playback. Only Spirit's existing single GPU producer
writes a persistent 256 KiB DMA source scratch, after its previous source read
has completed. It copies immutable resident sprites, never a GPU destination.
The scratch reserves GPU VA `[0x3ffc0000,0x40000000)`; the asset loader excludes
that interval. The buffer remains allocated across cancellations so a submitted
read cannot outlive it. GPU Logger pauses the handoff clock.

UI4 consumes only cursor poses acknowledged by the matching CUR_SURFLIVE
sequence. This rejects stale cursor completion and partitions the geometry
exactly, but **does not atomically latch the desktop and cursor planes together**.
The next UI4 submission follows that acknowledgement. Hardware testing is still
needed to assess a possible one-frame seam at plane entry and output edges.
No hardware boot or pixel-perfect cross-plane timing is claimed by host tests.

During the procedural effect, sprite translation, rotation, alpha cutoff, edge
fade and decorative shader mode are reset to their clean values so they cannot
distort the incoming tile. The user's Spirit background remains enabled.

## Native tile and regeneration

`codex-tile.svg` is the user-provided design, including its translucent gray
rounded background and white edge fade. `codex-tile64.png` and
`codex-tile64.rgba` are the complete native raster, without palette reduction or
alpha thresholding. The raw asset is straight RGBA, 16,384 bytes, SHA256:

`4c20e1fa5add0f399194ba4e3477dbe46ced1f0579dcc60c3db126075d649dc5`

Reproduce with Pillow, librsvg2, Cairo and GObject installed:

```sh
python3 tools/lilly/build-codex-tile.py --check --codex-root ../codex
# Regenerate both checkouts after an intentional SVG change:
python3 tools/lilly/build-codex-tile.py --codex-root ../codex
```

Codex CLI registers that same 64 × 64 raster in
`codex-rs/tui/src/tui/trueos_collapse.rs`. UI4 also selects its canonical tile
for the identified Codex Blueprint, so older CLI registrations cannot produce
a different handoff icon. The CLI source change requires its normal Blueprint
rebuild before it is present in a deployed CLI binary.

## Packed animation and crop alignment

[lily-pack/WISH-C.md](lily-pack/WISH-C.md) documents the shared32 palette,
binary alpha, sharpening, metadata crop, ZIP and tile16/XZ embedded archive
pipeline. These assets are already resident in `Lilly.lpak`; no PNG decoder or
animation generation runs during collapse.

The runtime deliberately bottom-centers metadata-cropped sprites. The source
static crop `[20,8,203,248]` therefore moves six pixels right on the 256 canvas.
The procedural tile's **top-left** chest anchor is `(113,156)`, matching the
original native `(107,156)` stamp after that runtime adjustment. Do not apply
the original crop offset a second time. Wish C durations are
`[450,180,180,180,180,160,240,220,450]` ms (2240 ms total).

## Checks

```sh
cargo check --release --bin TRUEOS
python3 tools/testpy/test_codex_handoff.py
python3 tools/testpy/test_ui4_dock_collapse.py
python3 tools/testpy/test_spirit_activity_gestures.py
python3 tools/testpy/test_ui4_collapse_animation.py
```

The handoff harness imports both production modules and checks identity,
collapse-settle delay, stale acknowledgements, live destination movement,
crossed-pose gating, Wish C completion, multiple folded windows and cancellation.
Geometry tests cover all eight entry directions and every tile pixel. The dock
harness exercises the production restore input block, including the shared circle
edge, rejected drags, extra buttons and denied authority.
