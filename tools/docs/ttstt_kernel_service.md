# TTSTT kernel service

TRUEOS warms the TTSTT model assets from the primary TRUEOSFS root and keeps
them resident for CPU inference jobs. The BSP service is registered as
`ttstt-cpu-service` and starts after the root, root index, and an AP2+ worker
are ready.

The expected filesystem layout matches the proven host CLI, rooted directly
below `trueosfs:/models`:

```text
models/
├── kokoro/
│   ├── kokoro-quant-convinteger.onnx
│   └── voices-v1.0.bin
└── whisper/
    └── ggml-base.bin
```

If the preferred Kokoro filename is absent, exactly one `.onnx` file in the
Kokoro directory is accepted. Missing assets leave the service dormant and it
retries without blocking boot.

## Residency and scheduling contract

- The BSP reads each file once through an open TRUEOSFS range handle.
- Reads are limited to 256 KiB and yield to Embassy after every chunk. The
  files are read sequentially to avoid boot-time device pressure and excess
  scratch memory.
- Inference workers never access TRUEOSFS. They share one immutable resident
  `ModelSet`.
- Workers wait for the CPU topology to settle, then start one pinned Embassy
  task on every registered AP2+ P-core executor. E/unknown AP2+ lanes are a
  fallback only when the complete topology contains no eligible P-core lane.
- Each `InferenceJob::run_slice` call is one bounded unit of CPU work. Pending
  work is requeued, and the worker yields before taking another slice.
- Kokoro jobs have one active owner and remain FIFO even though the shared
  pool has multiple workers. Up to eight more TTS jobs may wait; STT work can
  still use the remaining workers concurrently.
- `WorkerContext::matvec_bf16` uses TRUEOS's runtime AVX2/FMA dispatch with its
  established SSE2/scalar fallback.

## Backend boundary

The host CLI's model engines are not kernel dependencies: Kokoro currently
uses ONNX Runtime, and Whisper uses whisper.cpp. The kernel service therefore
does not pretend that loading ONNX/GGML bytes decodes those formats. A native
decoder implements `InferenceJob`, consumes the resident `ModelSet`, and uses
the provided CPU kernels in bounded slices. This keeps model parsing and
filesystem traffic out of the request path while avoiding a `std`, host-thread,
or C-runtime dependency in the kernel.

The decoder installs one `SpeechBackend`. Its cooperative warm job parses both
resident model images on the AP2+ pool and marks the backend ready; the BSP
supervisor retries deferred or failed warm jobs. Typed TTS/STT factories then
serve shell and later UI clients without teaching those consumers about ONNX
or GGML. The service refuses speech requests with `BackendUnavailable` until
this adapter is installed, and with `BackendWarming` until warm completes;
model residency alone is never reported as decoded inference.

For the packaged quantized Kokoro graph this is a substantial, explicit port:
the ONNX file is opset 20 with 3,614 nodes across 56 operator kinds and 775
initializers. It includes integer convolution/matmul, six
`DynamicQuantizeLSTM` nodes, Microsoft fused operators, resize, and STFT. The
existing BF16 matvec dispatch is useful infrastructure but is not an ONNX
executor. The Linux static ONNX Runtime build is also not directly linkable to
the freestanding kernel: it imports pthreads, mmap, dynamic loading, libc
allocation, files, clocks, and scheduler/syscall APIs.

## F4 shell commands

The first command consumer is shell2 F4 cmd mode:

```text
tts status
tts voice=af_heart speed=1.0 "Hello from TRUEOS"
tts stop

stt status
stt file recordings/hello.wav language=en
stt record
```

`tts` returns to the shell after enqueueing. One AP1 queue worker serializes
inference and playback, preserving submission order with a depth of eight.
`tts stop` clears waiting requests and discards the active inference result if
it cannot be cancelled inside the backend. The backend returns signed i16,
stereo, 48 kHz PCM, which is chunked cooperatively into the existing HDA lane.

Kokoro's input tensor has 512 positions, with two used for boundary padding.
The exact single-pass limit is therefore 510 phoneme tokens. The voice archive
has the matching `(510, 1, 256)` style shape. This must be enforced after
language-specific grapheme-to-phoneme conversion; a UTF-8 byte or Unicode
character limit cannot represent the model window exactly. Shell2 separately
uses an 8 KiB defensive allocation limit. Requests above 510 phonemes need to
be rejected or punctuation-split by the native backend.

File STT reads a signed 16-bit mono/stereo PCM WAV from TRUEOSFS on the BSP,
downmixes and resamples it to Whisper's mono 16 kHz boundary with periodic
Embassy yields, then queues only memory-resident samples to AP2+ workers.

`stt record` is intentionally a capability report for now. The HDA driver
already exposes discovered input-stream, ADC, microphone-pin, and line-input
counts, but it does not yet configure an input BDL or route an ADC stream tag.
The command therefore reports `dma_configured=0` and does not pretend to
capture silence or output-loopback audio.
