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

The host CLI's model engines are not kernel dependencies: Kokoro has both an
ONNX Runtime oracle and a pinned pure-Rust RTen reference, while Whisper uses
whisper.cpp. RTen proves the graph and audio contract locally, but its
`std`/Rayon graph runtime is not transplanted into the kernel. The kernel
service therefore does not pretend that loading ONNX/GGML bytes decodes those
formats. A native decoder implements `InferenceJob`, consumes the resident
`ModelSet`, and uses the provided CPU/GPU kernels in bounded slices. This keeps
model parsing and filesystem traffic out of the request path while avoiding a
host thread pool, C runtime, or filesystem access during inference.

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

For the packaged quantized Kokoro graph this is a substantial, explicit port.
The prepared reference graph is opset 20 with 3,615 nodes, 55 distinct
domain/operator entries, and 762 initializers. It includes integer
convolution/matmul, six bidirectional LSTMs, contrib normalization/GELU,
resize, convolution transpose, and STFT. The existing BF16 matvec dispatch is
useful infrastructure but is not an ONNX executor. The Linux static ONNX
Runtime build is also not directly linkable to the freestanding kernel: it
imports pthreads, mmap, dynamic loading, libc allocation, files, clocks, and
scheduler/syscall APIs.

### Native quantized-compute coverage

The i5-14500T CPU lane has allocation-free, dependency-free implementations
for quantized matrix multiplication and every group-one ConvInteger shape in
the pinned Kokoro graph. It dispatches between scalar, AVX2, and 256-bit
AVX-VNNI after the worker's XCR0 contract is established. Unsigned convolution
weights are shifted losslessly into the signed VNNI domain; padding is filled
with the activation zero-point, so padded taps are exactly zero after
centering. The checked host oracle reproduces real left-edge, interior, and
right-edge accumulators bit-for-bit on all three lanes.

The exact `0x4680` revision `0x0C` GPU lane contains baked C++/IGC kernels for
quantized matrix multiplication and the dominant stride-one ConvInteger
family. The convolution artifact admits 54 of 87 graph nodes and accounts for
83.92% of measured ConvInteger time; the generic CPU path is the fallback for
the remaining shapes. Its direct-RCS boundary uses bounded halo tiles, a
persistent DMA arena, ordered completion, and strict device/revision gating.
No neighboring Intel GPU is admitted by inference.

These kernels are not themselves a graph executor. The native path uses an
offline, hash-sealed Kokoro compiler: it lowers the prepared reference graph
to model-specific operations, prepacked constants, rank-four tensor
descriptors, view aliases, and liveness-planned arena slots. The no-std kernel
runtime executes that sealed program with a cooperative cursor. Duration
expansion is data-dependent, so the program has an explicit checked barrier:
encoder/duration prediction first, decoder/vocoder arena resolution second.
The observed 36 MiB host activation peak is a measurement, not an unchecked
kernel allocation limit.

The allocation-free f32 lane now implements 1,885 pinned graph nodes:
broadcast Add/Mul/Div/Sub, ReduceMean, LayerNormalization, Softmax, FastGelu,
SkipLayerNormalization, and every remaining elementwise unary math operator.
It accepts rank-four-or-smaller checked strided views and leaves outputs
unchanged on validation or math failure. A separate model-specific duration
kernel fuses the proven 50-logit Sigmoid/ReduceSum/Div/Round/Clip/Cast/CumSum
chain. It returns both the INT64 cumulative-duration vector and the checked
frame count used to size phase 1; a pinned ONNX Runtime oracle fixture covers
the exact barrier contract.

The typed layout lane covers another 811 pinned nodes: all Transpose, Gather,
Concat, two-way Split, Expand, Shape, Slice, both reflection Pad nodes,
NonZero, ScatterND, and 338
statically proven Reshape/Squeeze/Unsqueeze aliases. Copy kernels are generic across the model's
six admitted element types, normalize negative axes and indices exactly, and
validate every shape, permutation, broadcast, buffer, and alias before writing
the destination.

All six model Resize nodes also have a fixed no-std lane. Its admitted
contracts are nearest/asymmetric time upsampling by 2 or 300 and
linear/half-pixel down/up sampling by 300; batch and channel dimensions cannot
change. The scale enum, divisibility checks, and cooperative output ranges keep
shape policy out of the hot loop, and stable ONNX Runtime fixtures cover every
mode/scale family bit-for-bit.

The remaining scalar/control lane implements every pinned Cast, Range,
CumSum, comparison, boolean-And, Where, and ConstantOfShape form. That is 359
source nodes before quant-fusion overlap is removed. It uses the same rank-four
broadcast rules, checks integer ranges and cumulative overflow, and performs a
validation pass before any cast, range, or cumulative destination is changed.

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

The model residency, serialized queue, validated streaming contract, PCM
handoff into the live kernel playback lane, local Rust oracle, and primary
quantized CPU/GPU kernels are implemented. The repository does not yet install
a native Kokoro `SpeechBackend`; consequently a current boot correctly reports
`native-kokoro-backend-unregistered` and does not generate placeholder tones
or claim synthesis. The remaining critical path is the sealed AOT graph
program emission/dispatch and its recurrent, attention, layout/index,
float-convolution, resize, inverse-STFT, and vocoder operators. Audible speech
begins only when that executor registers the native backend and passes the
waveform oracle.

File STT reads a signed 16-bit mono/stereo PCM WAV from TRUEOSFS on the BSP,
downmixes and resamples it to Whisper's mono 16 kHz boundary with periodic
Embassy yields, then queues only memory-resident samples to AP2+ workers.

`stt record` is intentionally a capability report for now. The HDA driver
already exposes discovered input-stream, ADC, microphone-pin, and line-input
counts, but it does not yet configure an input BDL or route an ADC stream tag.
The command therefore reports `dma_configured=0` and does not pretend to
capture silence or output-loopback audio.
