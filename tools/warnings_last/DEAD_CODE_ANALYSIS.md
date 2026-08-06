# Dead-code and feature-flag analysis

This follow-up audit is separate from the ordinary zero-warning `make iso`
build. The tracked item-local expectations intentionally retain dormant APIs;
`--force-warn` can audit through those expectations without changing normal
build output.

## Feature findings

- Default kernel features are `trueos_rdp`, `trueos_lumen`, and
  `trueos_h264_encode_stream`.
- `trueos_h264_encode_probe` only enables `trueos_h264_encode_stream`; no source
  directly tests the probe feature. It is currently an alias, not an
  independent code path.
- `.cargo/config.toml` injects `cfg(kmod)`, but first-party Rust sources do not
  currently test it. The flag appears inert.
- `src/net/iwl4965.rs` has a file-wide dead-code allow despite being connected
  from `net/wifi.rs`; it is the highest-value existing suppression for a future
  focused driver audit.
- Most explicit first-party dead-code allows outside this cleanup are under
  `src/spirit/`.

## Dormant source outside the build graph

`crates/trueos-lsd` has a large legacy tree which is not compiled. Its active
library includes only `glob`, `runtime_config`, and `runtime_config_parse`, and
`autobins = false` excludes the legacy binary. The dormant `main.rs`, `git.rs`,
`core.rs`, `display.rs`, `flags/`, and `meta/` tree is a strong future archive
candidate. Its `sudo` and `no-git` cfgs do not describe the active crate.

## Dependency candidates

The following direct root dependencies merit a dedicated removal check:
`hashbrown`, `parry2d`, and lower Kokoro crates already reached through
`trueos-kokoro-dispatch` (`conv`, `duration`, `f32`, `gemm`, `layout`, `lstm`,
`resize`, `scalar`, and `stft`). `core3`, `miniz_oxide`, `zune-core`, and
`zune-jpeg` are not candidates because the kernel path-includes
`crates/trueos-graphics`.

## Reproducible forced audit

Run this after an ordinary ISO build to reveal retained/suppressed code:

```sh
cargo rustc --bin TRUEOS -- \
  --force-warn dead-code \
  --force-warn dead-code-pub-in-binary \
  --force-warn unreachable-code \
  --force-warn unreachable-patterns \
  --force-warn unused-crate-dependencies \
  --force-warn unexpected-cfgs
```

For feature coverage, repeat with `--no-default-features`, each default feature
alone, and `--all-features`. A probe-only run currently adds no source coverage
beyond the stream feature because the probe feature has no direct cfg consumer.

## Forced-audit result (2026-08-06)

The command above completed successfully after the zero-warning ISO build. It
printed 2,763 warning blocks (Cargo reported 2,769 including seven duplicate
diagnostics). This deliberately includes retained APIs hidden by item-local
expectations and the HDA/GPGPU exceptions; it is an analysis count, not the
ordinary build baseline.

The forced categories were dominated by 837 functions, 549 constants, 381
methods, 264 fields, 218 variants, 194 structs, 91 associated functions, and
60 statics. It also found 13 unreachable expressions and 14 direct dependency
candidates:

`bytes`, `dma_api`, `half`, `hashbrown`, `parry2d`, `trueos-kokoro-conv`,
`trueos-kokoro-duration`, `trueos-kokoro-f32`, `trueos-kokoro-gemm`,
`trueos-kokoro-layout`, `trueos-kokoro-lstm`, `trueos-kokoro-resize`,
`trueos-kokoro-scalar`, and `trueos-kokoro-stft`.

These dependency results are compiler evidence for the root binary only. They
should be removed one at a time with `make iso` verification because path-
included code and build scripts can create dependencies which a simple source
search does not reveal.
