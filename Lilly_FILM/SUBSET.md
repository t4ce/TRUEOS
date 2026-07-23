# Large-arm-motion subset

This experiment contains only Lilly animations whose adjacent keyframes make
large arm or hand pose jumps and whose held-out keyframes fail the single-pass
RIFE quality gate.

The strict pilot cutoff is:

1. the motion is visibly arm/hand driven;
2. minimum adjacent-frame silhouette IoU is below 0.80, including loop closure;
3. single-pass RIFE fails the existing Lilly quality gate.

| Copied frame set | Minimum adjacent alpha IoU | RIFE alpha IoU | RIFE edge F1 | RIFE RGB MAE |
| --- | ---: | ---: | ---: | ---: |
| `Happy/cheer_frames` | 0.5472 | 0.6171 | 0.3831 | 0.0921 |
| `Angry/fists_frames` | 0.6254 | 0.7476 | 0.3881 | 0.1568 |
| `Cry/crying-two-hands_frames` | 0.6953 | 0.8084 | 0.6071 | 0.0786 |
| `Silly/silly-roar_frames` | 0.7285 | 0.8410 | 0.6230 | 0.0565 |
| `Waving/waving-excited_frames` | 0.7673 | 0.8190 | 0.5883 | 0.0495 |
| `Taunt/taunt-tongue-out_frames` | 0.7787 | 0.8403 | 0.6791 | 0.0366 |

The RIFE columns are held-out four-frame means. They are selection evidence,
not a direct FILM-versus-RIFE comparison.

Not copied:

- `Waving/waving-smile_frames`: RIFE already passes with edge F1 0.8471.
- `Waving/waving-neutral_frames` and `Waving/waving-two-hands_frames`: RIFE
  passes the existing gate.
- `Idea/finger-up_frames` and later-ranked gestures: adjacent silhouette IoU is
  at least 0.80, so they remain outside this strict first pilot.
- face-, hair-, mouth-, or whole-body-dominant animations.

