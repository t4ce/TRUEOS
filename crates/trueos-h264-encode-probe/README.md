# TRUEOS H.264 encode proof

This `no_std` crate owns the codec-only half of the feature-gated
`trueos_h264_encode_probe` service. It generates a deterministic 1920x1080
I420 diagnostic frame, pads it to the 1920x1088 macroblock grid, and produces
one Annex-B access unit containing SPS, PPS, and an IDR slice.

The current slice uses Baseline-profile `I_PCM` macroblocks. It is a real,
lossless H.264 encoder proof, but deliberately provides no useful compression.
The kernel service records its size and execution time, discards the access
unit, and parks. It performs no filesystem or network operations; UDP port
8921 is reserved only as the boundary for a later independent transport step.

Enable the kernel service with:

```sh
cargo build --features trueos_h264_encode_probe
```

The host verifier feeds the generated access unit to FFmpeg and compares the
decoded I420 frame byte-for-byte with the source:

```sh
cd /tmp
cargo run \
  --manifest-path /path/to/TRUEOS/crates/trueos-h264-encode-probe/Cargo.toml \
  --target x86_64-unknown-linux-gnu \
  --release \
  --example verify_ffmpeg
```

Running from outside the repository avoids inheriting the kernel workspace's
custom target and `build-std` configuration.
