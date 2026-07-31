# Native Kokoro oracle

This is a host-only driver for the same allocation-free AOT executor, typed
memory bridge, and CPU dispatcher used by the TRUEOS kernel. It deliberately
lives outside the kernel package so filesystem, timing, WAV, and CLI code do
not leak into the `no_std` runtime crates.

With no input argument it runs the pinned `af_heart`, speed-1 IPA vector whose
RTen reference is exactly 412 decoder frames and 247,200 mono 24-kHz samples
(600 samples per frame):

```sh
cargo run --offline --release \
  --manifest-path tools/kokoro-native-oracle/Cargo.toml --
```

The default assets are read from
`crates/ttstt/.ttstt/models/kokoro`. The runner writes
`/tmp/trueos-kokoro-native-oracle.f32le` and
`/tmp/trueos-kokoro-native-oracle.wav`, prints phase timings and both SHA-256
digests, and fails if the reference frame/sample counts or WAV digest differ.

Arbitrary deterministic input is supported without invoking espeak:

```sh
cargo run --offline --release \
  --manifest-path tools/kokoro-native-oracle/Cargo.toml -- \
  --ipa 'həlˈoʊ fɹʌm ɹʌst' --voice af_heart --speed 1
```

Use `--text` (or `--text-file`) to exercise the resident G2P and Misaki
lexicon. Run `--help` for output paths and optional expectation gates.
