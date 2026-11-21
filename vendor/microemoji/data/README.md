# Microemoji kernel outlines

`outline-alpha.bin.lz4` is an LZ4 frame containing a microemoji v1 bundle. Each
layer keeps its outline and opacity, with the three RGB fields set to zero.
The kernel uses this source to produce R8 coverage at the requested size.

To regenerate the bundle from the TRUEOS repository root, use the supplied
Twemoji font and the scalar list in `vendor/microemoji/selection.txt`.

```sh
work_dir=$(mktemp -d)
cargo +1.95.0 run --target x86_64-unknown-linux-gnu -p microemoji --example export -- \
  ../TRUEOS-Blueprints/apps/lico/assets/twemoji.ttf \
  --alpha-bundle "$work_dir/outline-alpha.bin" --selection vendor/microemoji/selection.txt &&
  lz4 -9 -T1 -q -f --content-size "$work_dir/outline-alpha.bin" \
    vendor/microemoji/data/outline-alpha.bin.lz4
```

The outlines are derived from Twemoji artwork by Twitter, Inc. and other
Twemoji contributors under CC BY 4.0. The bundle preserves the artwork's
geometry and opacity and removes its RGB colors.
[Mozilla font notice](https://github.com/mozilla/twemoji-colr/blob/v0.7.0/LICENSE.md) ·
[Twemoji graphics license](https://github.com/twitter/twemoji/blob/v14.0.2/LICENSE-GRAPHICS)
