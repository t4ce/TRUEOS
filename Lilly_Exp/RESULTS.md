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

Held-out keyframe means:

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

