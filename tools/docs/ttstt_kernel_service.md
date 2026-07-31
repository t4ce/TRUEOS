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
  pool has multiple workers. Up to eight more service-level TTS jobs may wait;
  STT work can still use the remaining workers concurrently.
- Each worker constructs `trueos_ttstt_cpu::Dispatcher` after entering its AP
  executor. Quantized Kokoro work therefore selects AVX-VNNI on the i5-14500T
  and i9-13900K, AVX2 on the presently enabled Tiger Lake XSTATE contract, and
  scalar only when YMM state or AVX2 is unavailable. The dispatcher is carried
  in `WorkerContext` so a backend uses the lane detected on that worker CPU.
- `WorkerContext::matvec_bf16` remains a separate runtime AVX2/FMA path with
  its established SSE2/scalar fallback; status logs distinguish `q8_lane` from
  `bf16_lane` rather than conflating the two instruction contracts.

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
or GGML. A factory returning a job with the wrong direction is rejected. The
service refuses speech requests with `BackendUnavailable` until this adapter
is installed, and with `BackendWarming` until warm completes; model residency
alone is never reported as decoded inference.

### TTS request and output stream

`submit_tts(TtsRequest)` admits the request and immediately returns a job ID
plus a `TtsStream`. The native backend receives the same request with a
`TtsOutput` producer. This is an owned, bounded, nonblocking boundary:

- Language-specific G2P and text splitting belong to the backend. Each model
  inference chunk contains 1 through 510 phonemes.
- Every emitted `TtsAudioChunk` identifies its zero-based model-chunk index and
  phoneme count. Multiple PCM chunks can refer to one model chunk; exactly the
  last sets `end_of_model_chunk`.
- PCM is signed i16, interleaved stereo, 48 kHz. A PCM chunk contains at most
  12,000 frames (250 ms).
- The stream buffers at most four PCM chunks, so finalized audio can run ahead
  by no more than one second. `TtsOutput::try_push` never blocks an AP2+ worker.
  On `WouldBlock`, the backend retains that exact owned chunk, returns
  `JobProgress::Pending`, and retries it in a later slice.
- The service rejects empty, malformed, oversized, out-of-order, or
  phoneme-inconsistent chunks. `finish_success` closes only a complete model
  chunk and publishes counters calculated by the service itself.
- Consumer cancellation closes the output side cooperatively. The backend can
  observe `TtsOutput::cancelled` and exit early; independently, the service
  drops the job with `tts-stream-cancelled` before its next bounded slice. A
  TTS job that terminates without closing its stream is converted into an
  explicit failure rather than leaving shell2 waiting forever. Raw `submit`
  rejects TTS jobs so callers cannot bypass this stream guard.

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

`tts` returns to the shell after enqueueing. One AP1 queue worker owns one
request while up to eight more wait, preserving submission and PCM handoff
order. As each bounded chunk arrives, that worker applies backpressure and
calls the existing `aud::pcm_lane::submit_i16_stereo_48k` entry point. The
eSynth audio service consumes that shared lane and mixes it into live HDA
output. A stalled lane fails the request after 30 seconds instead of retaining
an unbounded waveform indefinitely.

`tts stop` clears the eight-request shell queue, increments the PCM stop
generation, clears pending PCM, and cooperatively cancels the active stream at
its next poll. The PCM stop is shared with other playback clients, so this
command intentionally stops those queued overlays too.

The success line says `pcm handoff complete`, not `playback complete`.
`pcm_lane` presently exposes queue depth but no per-request playback-completion
callback, and its depth excludes the buffer currently being mixed. Status and
completion output therefore use `playback_completion=untracked` rather than
claiming that the speakers have already rendered the final frame.

Kokoro's input tensor has 512 positions, with two used for boundary padding.
The exact single-pass limit is therefore 510 phoneme tokens. The voice archive
has the matching `(510, 1, 256)` style shape. This must be enforced after
language-specific grapheme-to-phoneme conversion; a UTF-8 byte or Unicode
character limit cannot represent the model window exactly. Shell2 separately
uses an 8 KiB defensive allocation limit. Requests above 510 phonemes are
split by the native backend into ordered model chunks, preferably at
punctuation in the final fifth of the window, matching the proven host
streaming behavior.

`tts status` reports model/backend readiness, service active and queued jobs,
buffered/emitted PCM, shell request/job IDs and phase, shared PCM-lane depth,
HDA readiness, and cumulative handoff/completion/failure/cancellation counts.

## Current implementation boundary

The model residency, serialized queue, validated streaming contract, and PCM
handoff into the live kernel playback lane are implemented. The repository
does not yet install a native Kokoro `SpeechBackend`; consequently a current
boot correctly reports `native-kokoro-backend-unregistered` and does not
generate placeholder tones or claim synthesis. Audible speech begins when the
native graph executor implements this contract and registers itself.

File STT reads a signed 16-bit mono/stereo PCM WAV from TRUEOSFS on the BSP,
downmixes and resamples it to Whisper's mono 16 kHz boundary with periodic
Embassy yields, then queues only memory-resident samples to AP2+ workers.

`stt record` is intentionally a capability report for now. The HDA driver
already exposes discovered input-stream, ADC, microphone-pin, and line-input
counts, but it does not yet configure an input BDL or route an ADC stream tag.
The command therefore reports `dma_configured=0` and does not pretend to
capture silence or output-loopback audio.
