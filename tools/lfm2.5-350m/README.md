# LiquidAI LFM2.5-350M Q8_0

Self-contained local inference setup for Liquid AI's instruction-tuned
LFM2.5-350M model, with a fixed CPU lane and an Intel NEO/IGC iGPU lane.

## Installed artifacts

- Model: `LFM2.5-350M-Q8_0.gguf`
- TRUEOS tokenizer: `LFM2.5-350M-Q8_0.tokenizer.bin` (generated)
- TRUEGA native weights: `LFM2.5-350M-Q8_0.native.bin` (generated)
- Hybrid CPU F32 sidecar: `LFM2.5-350M-Q8_0.cpu-f32.bin` (generated)
- Upstream: `LiquidAI/LFM2.5-350M-GGUF`
- Upstream revision: `bb7ee58b243e4cede04187e323e760b04f8a0091`
- Exact size: `379217632` bytes (379 MB decimal, 361.6 MiB)
- SHA-256: `be036a757295e550098b85e13f6af2735d0fa73b41e1156a40c7d8e8e32a5766`
- Runtime: official llama.cpp `b10075`, Linux x86-64 CPU build
- Headers/source ABI: llama.cpp commit `76f46ad29d61fd8c1401e8221842934bf62a6064`

The GGUF contains the tokenizer, special tokens, and chat template. No separate
tokenizer/config download is required. `LICENSE.LFM-1.0` is the model's upstream
custom license; it is not a standard OSI license, so review it before
redistributing or using the model commercially.

## Run

Start an interactive chat:

```sh
./tools/lfm2.5-350m/chat.sh
```

Run one prompt:

```sh
./tools/lfm2.5-350m/prompt.sh "Explain why the sky is blue." -n 256
```

Start a local OpenAI-compatible endpoint on `127.0.0.1:8080`:

```sh
./tools/lfm2.5-350m/serve.sh
```

Build and run the fixed userspace C++ lane:

```sh
make lfm25-cpp
./tools/lfm2.5-350m/cpp_prompt.sh --native "hi ai"
make lfm25-cpp-verify
make lfm25-igpu-verify
```

`lfm25-fixed` is intentionally model-specific rather than a general inference
frontend. It admits only the pinned 379,217,632-byte GGUF with SHA-256
`be036a757295e550098b85e13f6af2735d0fa73b41e1156a40c7d8e8e32a5766`,
uses the fixed TRUEOS chat token envelope, and performs greedy one-token decode
steps with a 32-token reply limit. `--native` loads only the vocabulary through
the pinned llama.cpp b10075 ABI; all model math runs in the fixed TRUEOS C++
implementation over the memory-mapped TRUEGA Q8 image and F32 sidecar. Omitting
`--native` keeps the pinned llama.cpp graph as a comparison oracle. The native
lane is deliberately fixed to this model, greedy decoding, a 256-token state
budget, and at most 32 reply tokens; it is not a generic GGUF runtime.

`make lfm25-cpp-verify` is the no-boot parity gate. It verifies the independent
Intel AVX2/FMA/F16C gate, up, and down projections exactly against captured
b10075 values, runs the complete 16-layer native path across all ten sealed
`hi` prompt decisions, and requires the native greedy `hi ai` reply to equal
the b10075 oracle: `Hello! How can I help you today?`. The native path includes
Q8 embeddings and projections, RMS norms, short convolution, RoPE attention
with F16 KV state, SwiGLU, residuals, and the tied vocabulary projection.

`make lfm25-igpu-verify` is the Ubuntu hardware gate. It feeds the published
62,288-byte SPIR-V artifact to Intel NEO with `clCreateProgramWithIL`, requires
IGC to return an executable device binary, and runs every fixed Q8 projection
on the selected Intel GPU. The remaining norms, state updates, attention
reductions, nonlinearities, tokenization, and control flow stay on the CPU.
The gate traces the process and fails unless it observes `libigdrcl`, the IGC
compiler libraries, an Intel DRM render node, and successful
`DRM_IOCTL_I915_GEM_EXECBUFFER2` submissions. It then requires all ten sealed
`hi` decisions and the complete `hi ai` reply to match the pinned b10075 oracle
byte for byte. The runner reports the selected device, projection count,
OpenCL event-profiled kernel time, NEO driver version, and the size and SHA-256
of the host-specific executable returned after the IGC build.

The packed endgame projection lane is verified independently before it enters
the TRUEOS product image:

```sh
make intel-gpu-bake-lfm25-q8-packed-cpp
make lfm25-packed-isa-verify
make lfm25-cpp-verify
make lfm25-igpu-packed-verify
```

It repacks all 93 Q8 matrices into pairs of 32-value blocks across sixteen
rows. Each pair is exactly 1,088 bytes, preserving the 376,701,952-byte image
and every sealed tensor offset while aligning every 64-byte SIMD16 weight
vector. The deterministic packed image SHA-256 is
`90876f02e0cc224fe23e01c8739dcbb94d7bcc8fbfa3d36204c6267a440f5fd8`.
The no-device C++ gate preserves all 25,994 binary16 subnormal scales
byte-for-byte, requires exact layer-0 gate/up/down results, and runs the packed
reference through all 930 projections in the sealed ten-token `hi` trace. The
ISA gate requires eight real SIMD16 DP4A instructions, no scratch, no SLM, and
at most one remaining scale-byte gather. The Ubuntu hardware gate feeds only
the packed SPIR-V to NEO, reports effective model-weight GB/s, and requires
sealed `hi` plus `hi ai` parity. After that gate passes, TRUEOS repacks the
sealed model once in place, verifies the packed hash, and selects the checked
packed ADL-S artifact through the existing hybrid backend. The Shell2 `lum`
command is unchanged, the legacy projection artifact remains published, and
the product build requires both artifacts while rerunning the packed ISA
check.

The checked-in `lfm25_q8_project.bin` is the separate ADL-S Zebin admitted by
the TRUEOS artifact contract. Ubuntu does not submit that ADL-S binary to the
Raptor Lake UHD 770; it uses the identical published SPIR-V and lets the host
IGC produce the appropriate Raptor Lake executable. This keeps the hardware
test honest about the target while exercising the same kernel source and ABI.

The launchers use Liquid AI's recommended generation defaults: temperature
`0.1`, top-k `50`, repetition penalty `1.05`, and a 32,768-token context.
Extra command-line arguments are passed through to llama.cpp and can override
these defaults. `prompt.sh` adds llama.cpp's `--single-turn` option so a closed
stdin cannot be mistaken for an empty stream of interactive prompts.

The model is compact and useful for extraction, structured output, tool use,
and modest local reasoning. Liquid AI does not recommend this 350M model for
knowledge-intensive work or programming.

## Restore the ignored binaries

The large model and runtime are intentionally ignored by Git. To fetch and
verify the same pinned artifacts again:

```sh
./tools/lfm2.5-350m/download.sh
```

## Build the FPGA-native image

From the repository root, seal the pinned GGUF into the deterministic TRUEGA model image:

```sh
./crates/trueos-fpga-abi/truega/tools/build_lfm25_image.sh
```

Generate the software-only F32 sidecar separately:

```sh
./tools/lfm2.5-350m/build_cpu_f32_sidecar.sh
```

Install `LFM2.5-350M-Q8_0.native.bin`,
`LFM2.5-350M-Q8_0.tokenizer.bin`, and
`LFM2.5-350M-Q8_0.cpu-f32.bin` under
`trueosfs:/models/lfm2.5/`. The sidecar contains the original little-endian
F32 bits for the 55 generated normalization and short-convolution tensors and
is sealed to the pinned GGUF, native image, and generated tensor table. These
are offline conversions only; neither command invokes Gowin, PCIe, JTAG, or
flashing.

## Capture the layer-0 FFN golden vectors

After building the native image, capture and verify the fixed BOS-token layer-0 FFN
checkpoint with:

```sh
./crates/trueos-fpga-abi/truega/tools/capture_lfm25_ffn_golden.sh
```

The trace source is rebuilt from the exact official llama.cpp `b10075` commit. The command
publishes only the small sealed golden artifact and HDL vectors under `truega/artifacts`;
the ignored GGUF, runtime, and native weight image remain in this directory.
