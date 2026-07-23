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

