# TRUEOS Lily asset pack decoder

Portable `no_std` + `alloc` Rust decoder for the `LILYPK01` format developed in
the LilyDS compression experiment. No host APIs, C libraries, PNG decoding or
video engine are needed to decode `tools/Lilly.lpak`.

The shipped pack is the exact shared-32, binary-alpha, cropped LilyDS library:
607 PNG assets compressed as a shared 16x16 index-tile dictionary with XZ/LZMA2.
Palette, relative names, original crop rectangles, backward references and
integrity checks are all inside the one 4,540,128-byte pack.

`decode(bytes)` reconstructs aligned 256x256 index frames. Call
`frame_rgba8(index)` to recover the original aligned RGBA8 frame,
`cropped_rgba8(index)` to recover its cropped PNG pixels, or
`frame_rgba8_bottom_center(index)` for Spirit's upload placement. The last method
uses crop width/height to position the retained rectangle at
`((256-width)/2, 256-height)` without changing colors or resampling pixels.

Spirit embeds `tools/Lilly.lpak` instead of `tools/Lilly.tar.lz4`. It expands each
frame once during the existing cold first job, writes all 607 RGBA8 surfaces to
its persistent DMA allocation, then releases the temporary index/decoder buffers.
The resident allocation is 159,121,408 bytes (151.75 MiB), fitting the existing
256 MiB execution VA reservation. The sprite walker presents this canvas at
unit scale with zero rotation: one source pixel per cursor pixel, upright and
unmirrored. Texel-center sampling preserves pixels with nearest and linear
sampling. Edge feather defaults to zero so the bottom row stays intact.
Explicit VFX transforms and effects remain available.

`tools/Lilly.catalog` is unchanged: 70 stable keys, seven logical frames each,
original timings, playback modes and source repeats. `tools/Lilly.sources` maps
its legacy directories to packed directories. Legacy source 1..7 maps to packed
frame_02..frame_08; catalog repetitions continue to reuse exactly the same surface.
The new library's optional static endpoint references and review-preview durations
are not inserted into the existing seven-slot player. The requested non-transition
`Waving_up/smile` also supplies the legacy `wave.two_hands` key. By user choice,
`Idle_up/Uncrossed-Arms/idle-quiet-laugh` also supplies `idle.uncrossed.scheming`
to preserve the existing uncrossed posture; the new crossed scheming frames
remain resident but are not selected by that key. New extra frames
are resident and directly addressable but are not added to scheduling logic.

The old archive remains beside the replacement for offline historical previews;
the kernel has no reference to or fallback to its frame bytes.

## Rebuild or inspect the pack

Use the host wrapper, which keeps the kernel Cargo target/build-std configuration out of the packer:

```sh
cd tools/lilly/lily-pack
./run.sh pack /path/to/LilyDS /tmp/Lilly.lpak
./run.sh verify /tmp/Lilly.lpak /path/to/LilyDS
./run.sh unpack /tmp/Lilly.lpak /tmp/lily-pngs
```

The tool uses this production Rust decoder for verification. It processes only
PNGs actually present in the source tree; stale alignment entries are ignored.
`pack` creates one file; `bench` retains the earlier experimental comparisons.
The tool's host encoders and their C dependencies are not kernel dependencies.

The decoder uses the vendored `lzma-rust2 0.21` dependency with its SHA-2
requirement pinned to `0.10.9`, sharing the kernel's existing digest stack.
The upstream XZ implementation and checksum validation are unchanged; this
avoids seven extra crypto build units from SHA-2 0.11 and its dependencies.

## Verify integration

From the TRUEOS root:

```sh
python3 tools/testpy/test_spirit_lily_pack.py
python3 tools/testpy/test_spirit_activity_gestures.py
cargo check --release --bin TRUEOS
```

The pack test imports the real cold loader and decoder, mocks only DMA/cache
operations, checks all 607 bottom-centered uploads against their cropped pixels,
and verifies every legacy key, semantic slot, repetition, playback boundary,
frame cadence and both path aliases. Full kernel checking needs the normally
prepared buildins for the selected profile. See FORMAT.md for the byte format.

The previous 599-frame pack was validated in a release ELF. The current 607-frame
pack adds only the selected Wish C seven interiors and its stamped opening
static; the existing final static is shared. All 599 previous cropped RGBA images
and crop rectangles were compared exactly after decoding both packs. The new
pack passes production decode/PNG comparison and the cold DMA loader harness;
all 70 keys/490 logical slots retain their existing playback behavior. Wish C
assets are resident/addressable but no new activity trigger is assigned here.
A new kernel ELF or hardware boot was not performed for this asset-only update.

Current pack SHA256:
`9fa2115ab8e06e41369bf98b413e3b5d0ddcdf184939913663f9bacf297b318a`.
See `tools/lilly/lily-pack/WISH-C.md` for the complete source-to-pack recipe,
including the lossy palette stage, original crop coordinates and nine-frame timing.
