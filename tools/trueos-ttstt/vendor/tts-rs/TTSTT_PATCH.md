# trueos-ttstt local patch

This directory is a source copy of MIT-licensed `tts-rs` 2026.2.3. It is kept
inside the repository so `trueos-ttstt` can preserve the Kokoro host reference without
patching the Cargo registry or installing source elsewhere.

The local changes add `KokoroEngine::synthesize_streaming` and its typed
`KokoroStreamError`. Kokoro still performs one blocking ONNX call per sequence
of at most 510 phonemes. Between calls, the API transfers owned audio buffers
to a callback while retaining exactly 240 samples for the existing 10 ms
crossfade. Flattening the callback buffers is tested to be sample-for-sample
identical to the original collected result.

For progressive playback, punctuation is only chosen as a split point in the
final fifth of a 510-phoneme window. An earlier punctuation mark falls back to
the full window instead of producing a short chunk that can drain while the
next ONNX call is still running.

The upstream non-streaming `SynthesisEngine::synthesize` API remains intact.
The example's stale `transcribe_rs` import was also corrected so workspace-wide
tests compile.

## Native Rust reference backend

The local patch optionally enables RTen with the `kokoro-rten` feature. RTen is
pinned to commit `7be5539edf37012eea7b79f582652fe536d7d087`, which contains
the ONNX Runtime contrib implementations needed by this Kokoro graph. The
runtime expects `kokoro-rten.onnx`, produced by
`tools/prepare_kokoro_rten.py` in the enclosing `trueos-ttstt` tool.

`KokoroModelParams::backend` selects ONNX Runtime (the default/oracle) or RTen.
Pre-phonemized input APIs are also exposed so backend and kernel comparisons do
not depend on espeak-ng output.
