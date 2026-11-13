# TRUEOS Lily asset pack decoder

Portable `no_std` + `alloc` Rust decoder for the `LILYPK01` format developed in
the LilyDS compression experiment. No host APIs, C libraries, PNG decoding or
video engine are needed to decode `tools/Lilly.lpak`.

The shipped pack is the exact shared-32, binary-alpha, cropped LilyDS library:
599 PNG assets compressed as a shared 16x16 index-tile dictionary with XZ/LZMA2.
Palette, relative names, original crop rectangles, backward references and
integrity checks are all inside the one 4,448,436-byte pack.

`decode(bytes)` reconstructs aligned 256x256 index frames. Call
`frame_rgba8(index)` to recover the original aligned RGBA8 frame,
`cropped_rgba8(index)` to recover its cropped PNG pixels, or
`frame_rgba8_bottom_center(index)` for Spirit's upload placement. The last method
uses crop width/height to position the retained rectangle at
`((256-width)/2, 256-height)` without changing colors or resampling pixels.

Spirit embeds `tools/Lilly.lpak` instead of `tools/Lilly.tar.lz4`. It expands each
frame once during the existing cold first job, writes all 599 RGBA8 surfaces to
its persistent DMA allocation, then releases the temporary index/decoder buffers.
The resident allocation is 157,024,256 bytes (149.75 MiB), fitting the existing
256 MiB execution VA reservation. Existing shader transforms and presentation
settings still apply to these bottom-centered source canvases.

`tools/Lilly.catalog` is unchanged: 70 stable keys, seven logical frames each,
original timings, playback modes and source repeats. `tools/Lilly.sources` maps
its legacy directories to packed directories. Legacy source 1..7 maps to packed
frame_02..frame_08; catalog repetitions continue to reuse exactly the same surface.
The new library's optional static endpoint references and review-preview durations
are not inserted into the existing seven-slot player. The requested non-transition
`Waving_up/smile` also supplies the legacy `wave.two_hands` key. New extra frames
are resident and directly addressable but are not added to scheduling logic.

The old archive remains beside the replacement for offline historical previews;
the kernel has no reference to or fallback to its frame bytes.

## Rebuild or inspect the pack

Run Cargo from the standalone tool directory (not the TRUEOS root configuration):

```sh
cd tools/lilly/lily-pack
cargo run --release -- pack /path/to/LilyDS /tmp/Lilly.lpak
cargo run --release -- verify /tmp/Lilly.lpak /path/to/LilyDS
cargo run --release -- unpack /tmp/Lilly.lpak /tmp/lily-pngs
```

The tool uses this production Rust decoder for verification. It processes only
PNGs actually present in the source tree; stale alignment entries are ignored.
`pack` creates one file; `bench` retains the earlier experimental comparisons.
The tool's host encoders and their C dependencies are not kernel dependencies.

## Verify integration

From the TRUEOS root:

```sh
python3 tools/testpy/test_spirit_lily_pack.py
python3 tools/testpy/test_spirit_activity_gestures.py
cargo check --release --bin TRUEOS
```

The pack test imports the real cold loader and decoder, mocks only DMA/cache
operations, checks all 599 bottom-centered uploads against their cropped pixels,
and verifies every legacy key, semantic slot, repetition, playback boundary,
frame cadence and both path aliases. Full kernel checking needs the normally
prepared buildins for the selected profile. See FORMAT.md for the byte format.
