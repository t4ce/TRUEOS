# TRUEOS H.264 encode proof

This `no_std` crate owns the codec-only half of the
`trueos_h264_encode_probe` service. Its boot workload expands a 60-byte
embedded scenario into 60 deterministic 512x512 I420 frames. The pattern
covers rotating full-spectrum bars, a legal-range luma ramp, high-frequency
chroma tiles, registration crosshairs, and a moving checker. Expanding the raw
frames at runtime avoids embedding roughly 23 MiB in the kernel image.

Every frame uses Baseline-profile `I_PCM` macroblocks and is emitted as an IDR
slice after one SPS/PPS pair. It is a real, lossless software H.264 encoder
proof, but deliberately provides no useful compression. Fifteen seconds after
boot the kernel audits Intel media-encode readiness, runs this software
fallback because hardware encode is not wired yet, and writes
`trueosfs:/video_encode_<timestamp>.h264`.

The service is in the default feature set. It can also be selected explicitly:

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
