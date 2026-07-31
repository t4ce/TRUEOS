# Native Kokoro oracle

This is a host-only driver for the same allocation-free AOT executor, typed
memory bridge, and CPU dispatcher used by the TRUEOS kernel. It deliberately
lives outside the kernel package so filesystem, timing, WAV, and CLI code do
not leak into the `no_std` runtime crates.

With no input argument it runs the pinned `af_heart`, speed-1 IPA vector. The
accepted native result is 416 decoder frames and 249,600 mono 24-kHz samples
(600 samples per frame). Two independent runs produced the same raw SHA-256
`a24f5fc04d52729f93d47dd517d4aeb5fbf772764bd8588c29bd69866bbdecf4`.
The separate RTen diagnostic resolves 412 frames; raw sample identity is not a
sound readiness gate across these different floating-point runtimes.

```sh
cargo run --offline --release \
  --manifest-path tools/kokoro-native-oracle/Cargo.toml --
```

The default assets are read from
`crates/ttstt/.ttstt/models/kokoro`. The runner writes
`/tmp/trueos-kokoro-native-oracle.f32le` and
`/tmp/trueos-kokoro-native-oracle.wav`, prints phase timings and both SHA-256
digests, and fails if the accepted native frame/sample counts or WAV digest
differ.

Arbitrary deterministic input is supported without invoking espeak:

```sh
cargo run --offline --release \
  --manifest-path tools/kokoro-native-oracle/Cargo.toml -- \
  --ipa 'həlˈoʊ fɹʌm ɹʌst' --voice af_heart --speed 1
```

Use `--text` (or `--text-file`) to exercise the resident G2P and Misaki
lexicon. Run `--help` for output paths and optional expectation gates.
