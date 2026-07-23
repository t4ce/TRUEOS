# Lilly RIFE experiment

This directory is a self-contained, non-destructive experiment for generating
pixel-art in-betweens from the canonical Lilly PNG frames.

The wrapper deliberately does not feed transparency to RIFE as a separate
image. It asks RIFE to estimate one motion field from the two sprites composited
over the same neutral background, then uses that exact flow and blend mask to
warp an edge-extended RGBA payload. This keeps RGB and alpha motion coupled.

The default model is Practical-RIFE 4.25, pinned by both source commit and model
archive SHA-256. Runtime dependencies, upstream source, model weights, reports,
and generated frames remain inside this directory and are ignored by Git.

## Bootstrap

```bash
cd Lilly_Exp
./setup.sh
./run.sh doctor
```

`setup.sh` creates `.venv`, installs the Python package, checks out the pinned
Practical-RIFE source under `.runtime`, and downloads its 4.25 checkpoint. It
does not modify the canonical `../../Lilly/Lilly` asset tree.

## Waving-smile experiment

Generate the three internal midpoints:

```bash
./run.sh sequence \
  ../../Lilly/Lilly/Waving/waving-smile_frames \
  outputs/waving-smile
```

Generate the three internal midpoints plus the loop-closing midpoint:

```bash
./run.sh sequence \
  ../../Lilly/Lilly/Waving/waving-smile_frames \
  outputs/waving-smile-loop \
  --loop
```

The sequence layout is:

```text
frame_01.png  untouched original 1
frame_02.png  generated midpoint 1 -> 2
frame_03.png  untouched original 2
frame_04.png  generated midpoint 2 -> 3
frame_05.png  untouched original 3
frame_06.png  generated midpoint 3 -> 4
frame_07.png  untouched original 4
frame_08.png  generated midpoint 4 -> 1 (only with --loop)
```

Original frames are copied byte-for-byte. Each output directory also contains
`report.json`, `contact-sheet.png`, and a transparent animated `preview.png`.

## Pair experiment

```bash
./run.sh pair \
  ../../Lilly/Lilly/Waving/waving-smile_frames/frame_01.png \
  ../../Lilly/Lilly/Waving/waving-smile_frames/frame_02.png \
  outputs/wave-01-02.png
```

Useful tuning switches:

```text
--work-scale 8        nearest-neighbour working enlargement
--timestep 0.5        interpolation position
--alpha-threshold 0.5 hard-alpha coverage threshold
--quantize pair       restrict opaque RGB to endpoint palette
--quantize none       retain RIFE's new colours
--background 127      neutral flow-estimation background, 0..255
--blend-mode rife      use RIFE's learned occlusion blend for all RGBA
--blend-mode temporal-alpha
                       experimentally retain one-sided sprite occupancy
--ensemble medoid      default: most representative of 12 intact candidates
--ensemble median      combine the robust middle result per working pixel
--ensemble none        fast single-pass mode
--face-only            change only the inner face; preserve the carrier body
```

The defaults favor canonical pixel-art output: box-filtered reduction followed
by endpoint-palette quantization and strictly binary alpha. The experimental
`temporal-alpha` mode still uses RIFE's shared bidirectional flow, but prevents
the learned mask from deleting a limb that exists at only one endpoint. That
can retain disoccluded parts at the cost of double edges, so it is deliberately
not the default.

The default `medoid` mode spends roughly twelve times the RIFE compute without
changing the final 128x128 dimensions. It evaluates three neutral backgrounds,
both temporal directions, and original/horizontally mirrored inputs, then keeps
the intact candidate nearest to their consensus. `median` combines the robust
middle result per working pixel instead. Use `--ensemble none` for the original
fast single-pass behavior.

## Face-only HighSettings

For the uniform seven-frame playback design—four untouched sources plus exactly
three generated facial states—use:

```bash
./run.sh sequence \
  ../../Lilly/Lilly/Waving/waving-smile_frames \
  outputs/HighSettings/waving-smile \
  --face-only \
  --work-scale 16
```

Do not pass `--loop`: that would intentionally add an eighth, loop-closing
midpoint. Each generated frame uses the preceding canonical frame as its body
carrier. RIFE selection is scored only inside the conservative inner-face mask;
the carrier alpha and every pixel outside that mask remain exact.

Generate the complete mirrored asset tree with one model load:

```bash
./run.sh library \
  ../../Lilly/Lilly \
  outputs/HighSettings/library-staging \
  --face-only \
  --work-scale 16
```

The library command accepts only exact canonical four-frame layouts, or existing
seven-frame refresh layouts whose original frames are at positions 1/3/5/7. It
writes seven-frame sets under the same relative paths and records every source
hash, selected candidate, and invariant in `library-report.json`. It never
alters the input tree.

After reviewing and backing up the canonical tree, preflight and promote the
staged frames directly into the existing frame directories:

```bash
./run.sh promote \
  outputs/HighSettings/library-staging \
  ../../Lilly/Lilly

./run.sh promote \
  outputs/HighSettings/library-staging \
  ../../Lilly/Lilly \
  --apply
```

Promotion refuses mismatched source hashes, noncanonical directory contents,
invalid RGBA/alpha, or a non-HighSettings manifest. Each frame directory is
swapped atomically, and a failed multi-directory operation is rolled back.

## Held-out accuracy check

Before trusting a model or setting across the library, predict the existing
middle keyframes from their two neighbours:

```bash
./run.sh evaluate \
  ../../Lilly/Lilly/Waving/waving-smile_frames \
  outputs/evaluate-waving-smile \
  --loop
```

This predicts source frame 2 from frames 1 and 3, source frame 3 from frames 2
and 4, and optionally the two loop-boundary cases. The report records alpha
IoU, one-pixel-tolerant edge F1, shared-opaque RGB error, exact RGBA agreement,
and silhouette area ratio. These are proxy measurements because artistic
keyframes are not guaranteed to have linear timing, but they are useful for
comparing model versions and settings on Lilly herself.

The initial automated gate requires every held-out case to have alpha IoU at
least 0.85, edge F1 at least 0.70, RGB MAE no more than 0.05, and silhouette
area ratio between 0.85 and 1.15. Passing means “safe to review”, not “approved”.

## Safety and acceptance

Every generated canonical frame must pass these invariants:

- 128 x 128 RGBA8;
- alpha values are exactly `{0, 255}`;
- transparent pixels are exactly `(0, 0, 0, 0)`;
- source files remain unchanged;
- output metadata records model/source pins and input SHA-256 hashes.

This tool establishes technical validity, not artistic approval. Fingers, eyes,
mouths, hair tips, and loop closure still need visual review before a frame is
promoted into the canonical Lilly repository.
