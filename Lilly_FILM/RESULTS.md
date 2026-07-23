# FILM strict-corpus pilot

Date: 2026-07-23

Configuration:

- official FILM Style SavedModel;
- one direct midpoint step at `t=0.5`;
- native 128x128 inference;
- black/white matte reconstruction;
- hard-alpha threshold 0.4;
- endpoint-palette quantization;
- four held-out predictions per looped frame set.

## Held-out comparison

| Frame set | Model | Alpha IoU | Edge F1 | RGB MAE | Exact RGBA |
| --- | --- | ---: | ---: | ---: | ---: |
| `Angry/fists` | RIFE | 0.7476 | 0.3881 | 0.1568 | 0.5498 |
|  | FILM | **0.7503** | **0.4153** | 0.1737 | 0.5245 |
| `Cry/crying-two-hands` | RIFE | **0.8084** | **0.6071** | **0.0786** | **0.7340** |
|  | FILM | 0.7983 | 0.5780 | 0.0940 | 0.6919 |
| `Happy/cheer` | RIFE | 0.6171 | **0.3831** | **0.0921** | **0.5745** |
|  | FILM | **0.6492** | 0.3755 | 0.1289 | 0.5456 |
| `Silly/silly-roar` | RIFE | 0.8410 | 0.6230 | **0.0565** | **0.7253** |
|  | FILM | **0.8530** | **0.6544** | 0.0695 | 0.6974 |
| `Taunt/taunt-tongue-out` | RIFE | 0.8403 | **0.6791** | **0.0366** | **0.7759** |
|  | FILM | **0.8554** | 0.6717 | 0.0402 | 0.7612 |
| `Waving/waving-excited` | RIFE | **0.8190** | **0.5883** | **0.0495** | **0.7288** |
|  | FILM | 0.8112 | 0.5808 | 0.0534 | 0.7053 |

FILM improves alpha IoU in four of six sets and improves edge F1 in two. It is
worse on RGB MAE and exact RGBA agreement in all six. No FILM set passes the
complete quality gate.

## Tuning result

On `Happy/cheer`, native-scale black/white matte reconstruction reached alpha
IoU 0.6492. Increasing the nearest-neighbour work scale to 2x or 4x did not
improve the full metric balance. Separate silhouette inference and gray
compositing were also worse.

The model does move arms over longer distances, but review frames still show
ghost limbs, torn sleeves, and incomplete hands. FILM is useful only as a
candidate large-motion proposal for a later hybrid; it is not a final-output
replacement for RIFE.

## Compute-heavy generic refinement

The `stable` profile tests 12 direct proposals per midpoint: scales 1x, 2x, and
4x, both endpoint orders, and horizontal mirror on/off. Black/white matte
reconstruction makes that 24 FILM calls per generated frame. Their
premultiplied colour median is combined with the original native-forward alpha
coverage. There is still no recursive interpolation.

| Frame set | Baseline RGB MAE | Stable RGB MAE | Baseline exact RGBA | Stable exact RGBA |
| --- | ---: | ---: | ---: | ---: |
| `Angry/fists` | 0.1737 | **0.1658** | 0.5245 | **0.5362** |
| `Cry/crying-two-hands` | 0.0940 | **0.0841** | 0.6919 | **0.7105** |
| `Happy/cheer` | 0.1289 | **0.1207** | 0.5456 | **0.5583** |
| `Silly/silly-roar` | 0.0695 | **0.0686** | 0.6974 | **0.7113** |
| `Taunt/taunt-tongue-out` | 0.0402 | **0.0377** | 0.7612 | **0.7740** |
| `Waving/waving-excited` | 0.0534 | **0.0511** | 0.7053 | **0.7194** |
| **Corpus mean** | 0.0933 | **0.0880** | 0.6543 | **0.6683** |

RGB MAE and exact RGBA improve in all six sets. Alpha IoU (0.7862 corpus
mean), alpha area ratio (0.9869), and edge F1 (0.5459) are identical to the
baseline because `stable` deliberately retains its alpha estimate. Median or
medoid aggregation of alpha was tested and made edge recovery less reliable.

The complete six-set sequence plus held-out evaluation took 901.0 seconds on
CPU, versus 9.9 seconds for the baseline. The large cost comes mainly from 2x
and 4x spatial inference, not only the number of candidates.

This is a safer generic FILM proposal, but not a quality approval: all six sets
still fail the complete gate, and visual review still finds detached or missing
hands on the hardest Cheer transitions. Animation-specific adaptation can be
evaluated next without confusing its gain with this generic test-time
consensus.
