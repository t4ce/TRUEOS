# LiquidAI LFM2.5-350M Q8_0

Self-contained, CPU-only local inference setup for Liquid AI's instruction-tuned
LFM2.5-350M model.

## Installed artifacts

- Model: `LFM2.5-350M-Q8_0.gguf`
- Upstream: `LiquidAI/LFM2.5-350M-GGUF`
- Upstream revision: `bb7ee58b243e4cede04187e323e760b04f8a0091`
- Exact size: `379217632` bytes (379 MB decimal, 361.6 MiB)
- SHA-256: `be036a757295e550098b85e13f6af2735d0fa73b41e1156a40c7d8e8e32a5766`
- Runtime: official llama.cpp `b10075`, Linux x86-64 CPU build

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

The generated `LFM2.5-350M-Q8_0.truega.bin` remains ignored beside the GGUF. This is an
offline conversion only; the command does not invoke Gowin, PCIe, JTAG, or flashing.

## Capture the layer-0 FFN golden vectors

After building the native image, capture and verify the fixed BOS-token layer-0 FFN
checkpoint with:

```sh
./crates/trueos-fpga-abi/truega/tools/capture_lfm25_ffn_golden.sh
```

The trace source is rebuilt from the exact official llama.cpp `b10075` commit. The command
publishes only the small sealed golden artifact and HDL vectors under `truega/artifacts`;
the ignored GGUF, runtime, and native weight image remain in this directory.
