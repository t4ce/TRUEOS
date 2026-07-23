# Lilly FILM large-motion experiment

This is an isolated, non-destructive pilot for the Lilly transitions that make
arm and hand pose jumps too large for the existing RIFE path. It uses Google's
FILM (Frame Interpolation for Large Motion) exactly once at `t=0.5` per pair.
There is no recursive interpolation and no automatic handoff to RIFE yet.

Only the six strict large-arm-motion frame sets listed in `SUBSET.md` are copied
under `Lilly/`. Their relative paths and source PNG bytes are preserved.

## Runtime

```bash
cd Lilly_FILM
./setup.sh
./run.sh doctor
```

The setup is self-contained:

- standalone CPython 3.9.25, pinned by SHA-256;
- TensorFlow 2.6.2 on CPU, matching the official FILM runtime generation;
- official Google Research source at commit
  `69f8708f08e62c2edf46a27616a4bfcf083e2076`;
- official FILM Style SavedModel, with every downloaded file checked against
  the locally recorded SHA-256 from the public Google Drive artifact.

The Style model is used because its training objective targets sharper large
disocclusions. The host's current CUDA stack is intentionally not mixed with
FILM's legacy CUDA 11.2 runtime; 128x128 CPU inference is fast enough for this
pilot and much easier to reproduce.

## Run the strict corpus

```bash
./run.sh corpus Lilly outputs/pilot --loop
```

For each copied animation this writes:

- an eight-frame loop under `sequence/`, with four untouched originals and four
  single-step FILM midpoints;
- held-out keyframe predictions and metrics under `evaluation/`;
- `contact-sheet.png`, transparent `preview.png`, and JSON reports.

Run one frame set:

```bash
./run.sh sequence \
  Lilly/Happy/cheer_frames \
  outputs/cheer \
  --loop
```

## Alpha policy

FILM accepts RGB only. The default matte reconstruction therefore makes exactly
two direct FILM calls per pair: one composited over black and one over white.
Their difference estimates coverage; the black result estimates premultiplied
colour. The result is unpremultiplied, snapped to the endpoint palette, and
hardened back to binary alpha at a calibrated 0.4 cutoff. Transparent RGB is
forced to zero.

Experimental `--color-mode gray` and `--color-mode premultiplied` paths are
available, but the held-out pilot favored `matte`. `--work-scale 2` and `4`
were also tested; neither justified replacing native 128x128 inference.

This keeps FILM responsible only for the initial large-motion proposal without
recursively feeding generated frames back into FILM.

Every output must remain 128x128 RGBA8 with alpha values exactly `{0, 255}`.
Original copied frames are hash-checked byte-for-byte when sequences are made.

## Result boundary

FILM is not approved as a final Lilly renderer. On this corpus it occasionally
improves silhouette or edge recovery, especially for `Angry/fists` and
`Silly/silly-roar`, but it loses colour fidelity to RIFE and none of the six
sets passes the full quality gate. See `RESULTS.md`.

The evidence supports a future hybrid experiment where FILM proposes only the
large-motion correspondence or mask and another stage reconstructs the final
sprite. This folder deliberately stops before that handoff.

