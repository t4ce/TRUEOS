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
does not modify `../tools/Lilly`.

## Waving-smile experiment

Generate the three internal midpoints:

```bash
./run.sh sequence \
  ../tools/Lilly/Waving/waving-smile_frames \
  outputs/waving-smile
```

Generate the three internal midpoints plus the loop-closing midpoint:

```bash
./run.sh sequence \
  ../tools/Lilly/Waving/waving-smile_frames \
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
  ../tools/Lilly/Waving/waving-smile_frames/frame_01.png \
  ../tools/Lilly/Waving/waving-smile_frames/frame_02.png \
  outputs/wave-01-02.png
```

Useful tuning switches:

```text
--work-scale 4        nearest-neighbour working enlargement
--timestep 0.5        interpolation position
--alpha-threshold 0.5 hard-alpha coverage threshold
--quantize pair       restrict opaque RGB to endpoint palette
--quantize none       retain RIFE's new colours
--background 127      neutral flow-estimation background, 0..255
```

The defaults favor canonical pixel-art output: box-filtered reduction followed
by endpoint-palette quantization and strictly binary alpha.

## Safety and acceptance

Every generated canonical frame must pass these invariants:

- 128 x 128 RGBA8;
- alpha values are exactly `{0, 255}`;
- transparent pixels are exactly `(0, 0, 0, 0)`;
- source files remain unchanged;
- output metadata records model/source pins and input SHA-256 hashes.

This tool establishes technical validity, not artistic approval. Fingers, eyes,
mouths, hair tips, and loop closure still need visual review before a frame is
promoted into `tools/Lilly`.

