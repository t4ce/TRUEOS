# Selected Wish C: authoring to embedded sprites

Selected source: `Character Lily - V1/Static_up/wish-c`. Seven original
1254x1254 RGBA interiors are unchanged. Authoring manifest:
`json/Static_up__wish-c__generation.json`. Its moved paths were repaired.
The original stamped opening was restored with `build-codex-static.py`:
SHA256 `2b25fdf8fba1d8b73fea2825c16080ee26606632eb613f8fb77c839869f64ea4`.
The final static is shared with the existing library.

## Actual production path

1. Run existing `LilyUpWorkflow/build-lilyds.py:convert` for only these nine
   sequence references. Premultiplied-alpha Lanczos to256x256; existing premultiplied
   RGB UnsharpMask(radius0.65, percent65, threshold3), clamp to alpha, unpremultiply.
2. Map without dithering to the existing `PNG_shared32/palette.json`:
   index0 transparent,31 visible opaque colors. Alpha threshold128. This is the
   lossy stage; faint VFX can disappear and soft edges become binary.
3. Crop to `(left,top,right,256)` after thresholding, keeping the original bottom.
   Save original top-left canvas rectangle `[x,y,w,h]` in `alignment.json`.
   Retain8-bit PNG indices but trim PLTE to32 entries; trimming adds no pixel loss.
4. Pack with repository-owned production `lily-pack`: globally shared16x16
   index tiles, then XZ/LZMA2. Palette, frame names, crop rectangles and CRCs are
   inside the self-contained `LILYPK01` pack. This stage is lossless relative
   to the indexed PNGs.
5. `src/spirit/lilly.rs` includes `tools/Lilly.lpak`. The production Rust
   decoder reconstructs256x256 indices and RGBA8, then uploads once to resident DMA.
   Current runtime deliberately places cropped sprites bottom-center. Original
   crop x/y are retained for original-alignment reconstruction; they must not
   be replaced with bottom-center offsets.

H.264 grayscale index video is an earlier verified compression experiment,
not this runtime's decoder path. `tools/Lilly.tar.lz4` is historical offline
preview data and remains untouched. No video codec dependency is added.

## Reproduce

The scoped exporter reuses the existing converter, preserves existing LilyDS
PNGs and crop entries, trims palette storage, verifies standalone pack decoding,
writes aligned and runtime-placement review PNGs, and creates a delivery ZIP.

```sh
cd "/home/t4ce/Repositories/TRUEOS"
tools/lilly/lily-pack/run.sh load tools/Lilly.lpak
cd "/home/t4ce/Repositories/Character Lily - V1"
python3 LilyUpWorkflow/export-wish-c.py --integrate
python3 build-codex-static.py --check
python3 build-preview-up.py
cd "/home/t4ce/Repositories/TRUEOS"
python3 tools/testpy/test_spirit_lily_pack.py
python3 tools/testpy/test_spirit_activity_gestures.py
```

Without `--integrate`, the exporter updates the scoped LilyDS PNGs/alignment and
standalone delivery, leaving TRUEOS's embedded source pack and library ZIP alone.
With it, the exporter rebuilds/validates `tools/Lilly.lpak` and
`tools/lilly/LilyDS.zip`. The host wrapper builds the current production packer
outside kernel Cargo configuration. No palette retraining or full-library
requantization occurs. Review PNGs stay outside the packer's recursive PNG
source directory so repeat exports cannot accidentally ingest them.

## Artifacts and measurements

- `/home/t4ce/Repositories/LilyDS/Static_up/wish-c`: seven selected indexed interiors.
- `LilyDS/Static_up/codex.png`: new stamped opening; existing `static.png` reused.
- `/home/t4ce/Repositories/LilyDS-WishC`: nine unique cropped PNGs with original
  relative paths, `alignment.json`, nine-frame `sequence.json`, `WishC.lpak`,
  `WishC.zip`, `review.gif`, `review.jpg` and conversion report.
- `/home/t4ce/Repositories/LilyDS-WishC-Review`: original-aligned and
  existing-runtime-bottom-centered 256px review PNGs, both horizontal sprite strips,
  and the shared palette strip.
- Full-resolution source references:14,654,340 bytes.
- Standalone cropped indexed PNGs:142,771 bytes; standalone pack:101,540 bytes.
- Full embedded pack:4,540,128 bytes, up from4,448,436 (+91,692).
- 607 assets,159,121,408 resident RGBA8 bytes (151.75MiB), within existing256MiB VA.
- Current full pack SHA256:
  `9fa2115ab8e06e41369bf98b413e3b5d0ddcdf184939913663f9bacf297b318a`.

Timings remain `[450,180,180,180,180,160,240,220,450]`ms in the delivery
`sequence.json`. LPAK is an image container, not a playback catalog. All70 existing
catalog keys and490 logical slots retain their exact semantics; the new assets
are resident/addressable with no new activity trigger assigned.

## Validation evidence

Production pack/verify/unpack compared all nine standalone RGBA images and crop
rectangles exactly. Full pack verification covered all607 source images.
Old and new embedded packs were separately decoded: every one of the599 old
cropped RGBA images and crop rectangles matched. The real production cold-loader
test passed607 exact bottom-centered DMA uploads;28 existing activity/sequencing
tests also passed. Delivery ZIP and library ZIP were read back and compared to
their source bytes. No new kernel ELF build or hardware boot is claimed.

The before/after packs, unpacked comparisons and `integration.json` evidence live
in `/home/t4ce/.codex/generated_images/lily-wish-c-production-20261113`.

