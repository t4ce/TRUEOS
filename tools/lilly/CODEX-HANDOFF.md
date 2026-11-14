# Codex collapse into Lilly

UI4's compact tile is natively **64 × 64** (`collapse::Collapse::project`).
Other apps retain their usual movable compact tile. A live Codex Blueprint
(`codex.bp`, including its leased Shell3 terminal) instead folds into Lilly:

1. At the request, capture the compact icon's screen position and opacity as
   an alternative-collapse restore ticket. Release cursor selection and keyboard
   focus while retaining the original window and its cursor registrations.
   Disable its hit testing, movement and input during the initial shrink.
2. When the existing shrink finishes, remove the compact window from composition.
   The detached sprite owns the 500 ms settlement delay and subsequent flight.
   No compact-window drag is armed. A stale drag cannot cancel or move it.
3. Slot4, the existing software cursor plane, draws the complete native gray-backed
   tile using 2,439 precomputed RGBA horizontal runs. The cache is built once;
   each motion sample only translates and clips those runs. It creates no frame
   resource, publishes no window image, and does not query window state or dirty
   the application compositor during flight.
4. Absolute-time smoothstep motion runs on slot4's existing 60 Hz cadence, even
   with no mouse input or Lilly frame changes. It takes 320–1100 ms depending on
   distance. The destination follows the applied 256 × 256 cursor plane. Lilly
   can prepare crossed arms during the delay and flight.
5. At the plane overlap, Spirit stamps the inside portion after the character's
   GPU VFX pass; slot4 draws the outside portion. A 48 px approach margin temporarily raises Spirit's producer
   target to 60 Hz. Both portions use one acknowledged integer pose and identical
   native alpha/window opacity. Until the first overlap acknowledgement, slot4
   retains its last complete outside pose. Ordinary Lilly sprite timings remain
   independent and return to their usual producer cadence after landing.
6. Once crossed arms are ready, blend into the packed chest stamp over 100 ms
   and play Wish C, ending in crossed static. The authored stamp omits gray;
   the procedural arriving tile includes it.
7. The restore ticket remains after playback. A primary click anywhere in the
   existing radius-150 Lilly/Spirit dock circle restores and focuses the window
   at its original placement. This works during the shrink, delay and flight as
   well. Existing drag/drop, menus and resize gestures take precedence. Movement
   beyond click tolerance, another button, or denied cursor authority cancels the
   click. Multiple requests serialize; circle clicks restore the latest first.

`src/ui4/codex_handoff.rs` owns only the lifecycle boundary and restore tickets.
`src/spirit/codex_handoff.rs` owns the sprite motion and Wish C playback.
`src/ui4/slot4_service.rs` owns software-plane rendering and wake cadence.
Restoration, close, icon replacement and owner cleanup cancel the ticket and
sprite together; a late cursor acknowledgement cannot respawn a canceled icon.
No new buildin, app window, pointer source or Blueprint ABI is introduced.

## Ownership and presentation

Lilly and Wish C use the configured sprite VFX throughout. The native Codex
tile is applied afterwards to the completed premultiplied BGRA cursor back
buffer, through the existing `CPU_AND_GPU` barrier and `write_cpu` API. Ordering
is GPU completion/release, CPU cache invalidation, alpha-correct stamp, CPU
flush/release, then cursor eligibility. Ordinary frames keep their GPU-only
path. A stamp validates its sequence and visual before writing; canceled work
does not add a logo. The normal background settings are passed through too.

The existing barrier and explicit GPU-then-CPU stamping contract were introduced
by `t4ce` in commit `4e8e079098cf8f14e138afa484f87d73a4c26848`, 21 July 2026
23:33 CEST (`ok`).

Only the 100 ms landing crossfade needs the persistent 256 KiB DMA source
scratch, written by Spirit's single producer after its previous GPU source
read has completed. It blends immutable resident character sprites before VFX;
the native tile fades out in the post-pass as the authored chest stamp fades in.
Outside the hardware plane, the software sprite needs none of this copying or
GPU work. The scratch reserves GPU
VA `[0x3ffc0000,0x40000000)`; the asset loader excludes that interval. It remains
allocated across cancellations so an in-flight read cannot outlive it. GPU Logger
pauses the handoff clock.

Slot4 consumes poses acknowledged by matching CUR_SURFLIVE sequences only during
overlap. This rejects stale completion and partitions geometry exactly, but does
not atomically latch the software and hardware planes together. Slot4 follows
that acknowledgement on its next presentation. An on-device visual check is
still needed to assess the brief boundary hold and any one-frame seam; host tests
cannot establish cross-plane display timing or sustained hardware frame rate.

No handoff override disables the character's shader, transform, alpha cutoff
or edge fade. The post-pass keeps the incoming logo independent of those effects.

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
python3 tools/testpy/test_spirit_vfx_cpu_stamp.py
python3 tools/testpy/test_codex_slot4_sprite.py
python3 tools/testpy/test_ui4_pointer_groups.py
python3 tools/testpy/test_ui4_dock_collapse.py
python3 tools/testpy/test_spirit_activity_gestures.py
python3 tools/testpy/test_ui4_collapse_animation.py
```

The handoff harness imports both production modules and checks identity,
detached settlement, motion without cursor/sprite frames or broker queries,
zero application-compositor updates during flight, stale acknowledgements, live destination movement,
crossed-pose gating, Wish C completion, multiple folded windows and cancellation.
Geometry tests cover all eight entry directions and every tile pixel. The dock
harness exercises the production restore input block, including the shared circle
edge, rejected drags, extra buttons and denied authority.

The software raster suite compares all emitted pixels against the production
RGBA partition across eight directions, output edges and four opacity values.
The pointer-rig tests verify parking releases every selecting cursor while
preserving registrations for restoration and leaving unrelated windows alone.
If prepared buildin inputs are stale, the kernel-only check uses the usual
default subsystem features with `buildin-apps` omitted; no apps are rebuilt or
deployed by that check.
