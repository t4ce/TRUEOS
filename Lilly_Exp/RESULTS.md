# Initial experiment results

Date: 2026-07-23

Environment:

- NVIDIA GeForce RTX 3060 Ti, 8 GiB
- PyTorch 2.12.1 + CUDA 13.0
- Practical-RIFE 4.25
- RIFE source commit `17d8c7a1005b37f4c97bfee04e316aaec7fdc536`
- 8x nearest-neighbour working scale
- standard RIFE occlusion blend
- endpoint-palette quantization
- binary alpha threshold 0.5

Held-out keyframe means for the original single-pass baseline:

| Pilot | Difficulty | Alpha IoU | Edge F1 | RGB MAE | Exact RGBA |
| --- | --- | ---: | ---: | ---: | ---: |
| `Idle/Crossed-Arms/idle-5` | low | 0.9999 | 1.0000 | 0.0127 | 0.9396 |
| `Waving/waving-smile` | medium | 0.9325 | 0.8359 | 0.0165 | 0.9006 |
| `Happy/cheer` | high | 0.6171 | 0.3831 | 0.0921 | 0.5745 |

The technical pipeline succeeds: output is 128x128 RGBA8, alpha remains binary,
transparent pixels are zero, endpoint palettes are enforced, and original
copies are byte-identical.

The artistic result is transition-dependent. Low motion is excellent and the
supplied wave is clean enough for focused review. `Happy/cheer` contains major
pose and occlusion changes; RIFE produces ghosting and should be rejected or
sent to a generative/manual fallback. This confirms that the quality gate must
run before a library-wide bake.

The experimental `temporal-alpha` blend retains limbs that occur in only one
endpoint, but it creates double edges around fingers on the supplied wave. The
standard RIFE blend remains the default.

## Refined default

The default now evaluates twelve intact candidates: dark/mid/light neutral
backgrounds, both temporal directions, and original/horizontally mirrored
inputs. It selects the candidate nearest to their working-resolution consensus
before the final 128x128 reduction and hard-alpha cleanup.

| Pilot | Alpha IoU | Edge F1 | RGB MAE | Exact RGBA |
| --- | ---: | ---: | ---: | ---: |
| `Idle/Crossed-Arms/idle-5` | 1.0000 | 1.0000 | 0.0125 | 0.9397 |
| `Waving/waving-smile` | 0.9326 | 0.8471 | 0.0165 | 0.9032 |
| `Happy/cheer` | 0.6488 | 0.3977 | 0.1057 | 0.5892 |

On the supplied looped wave, measured RIFE inference increased from 0.36 s for
four single-pass midpoints to 2.77 s for four refined midpoints. This is twelve
model evaluations per midpoint and about 7.7x observed GPU inference time due
to fixed overheads. Both paths still emit only 128x128 output.

The refined mode improves wave edge agreement and exact RGBA agreement, remains
effectively neutral on the low-motion set, and substantially improves silhouette
IoU on the hard set. The hard `cheer` transition still fails the quality gate:
extra inference cannot reconstruct a pose absent from both adjacent endpoints.

Increasing the working scale from 8x to 16x did not provide a consistent win.
On the wave it slightly improved alpha IoU and RGB error but reduced edge F1;
8x therefore remains the default while 16x stays available as an experiment.

## Face-only HighSettings pilot

The revised playback design keeps all four canonical frames and inserts exactly
three facial midpoints. The pilot command used 16x internal resolution, the
12-pass medoid ensemble, no loop midpoint, and the conservative inner-face
region `[44, 43, 85, 70]`.

For `Waving/waving-smile`, the three midpoints required 9.40 s of measured GPU
inference. All seven outputs are 128x128 RGBA8. Every generated frame retained
binary alpha, zero transparent RGB, the complete alpha plane of its preceding
canonical carrier, and exactly zero changed pixels outside the face region.
The four canonical frames in output positions 1, 3, 5, and 7 are byte-identical
copies of their sources.

## Full asset refresh

The face-only HighSettings profile was run across all 68 animation sets in
`/home/t4ce/REPOS/Lilly/Lilly` with a single model load:

- 204 generated facial midpoints;
- 592.93 s measured RIFE inference;
- 476 final canonical PNGs;
- exactly seven frames in every existing `*_frames` directory;
- originals preserved byte-for-byte at positions 1/3/5/7;
- generated states placed at positions 2/4/6;
- zero changed pixels outside the face region;
- zero alpha, transparent-RGB, source-hash, or staging-hash failures.

The staged manifest and atomic promotion report are under
`outputs/HighSettings/library-staging`. The pre-refresh four-frame asset tree is
recoverable from
`outputs/HighSettings/backups/Lilly-before-face-refresh-20260723.tar.gz`;
its SHA-256 is
`a40839bbe9736fe5bf372bd7922a604dd3518ea15d9ab13e9ad850dd438e8d21`.
