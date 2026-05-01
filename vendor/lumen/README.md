# Lumen

> A compact Rust-first deep learning core with dynamic autograd, flexible dtype control, safetensors loading, quantization-aware inference, and CPU/CUDA Llama runtime work.

[中文说明](./README_zh-CN.md)

---

## What this project is

Lumen is a **small but complete Rust-first ML stack** that connects several layers of the system in one repository:

- a tensor core with dynamic autograd;
- reusable layers, modules, losses, and optimizers;
- a Llama-style decoder implementation;
- safetensors loading with optional streaming;
- runtime dtype control for parameters, activations, and KV cache;
- optional on-load or offline `i8` quantization;
- CPU execution paths and optional CUDA acceleration;
- benchmark tools for CPU/CUDA kernel work and end-to-end inference experiments.

This repository is best understood as:

- a **learning-oriented deep learning core** mainly written in Rust, and
- a **CPU/CUDA LLM inference playground** centered on a compact Llama runtime.

It is **not** trying to be a full training framework, a production serving stack, or a universal launcher for arbitrary checkpoints.

Lumen is also **not a pure Rust project**. The high-level runtime, tensor/autograd system, model code, loader, tokenizer wrapper, dtype policy, and CPU backend are written in Rust, while the optional CUDA backend uses CUDA C++ kernels and NVIDIA libraries through an FFI boundary.

A more accurate description is:

> Rust-first, not Rust-only.

---

## Current focus

The project now has both a mature CPU path and an actively improving CUDA path.

Notable parts of the current codebase:

- dynamic autograd and general tensor ops;
- a Llama-family decoder with RMSNorm, RoPE, GQA, SwiGLU-style MLP, and KV-cache decoding;
- support for `f32`, `f16`, `bf16`, and `i8` in storage, loading, and runtime configuration;
- optional CUDA execution behind the `cuda` feature;
- cuDNN detection that prefers explicit/system installs and can fall back to Python `nvidia.cudnn`;
- CUDA-resident tensors, KV-cache updates, forward decode, and a growing training/backward path;
- optional parameter dtype copies for faster mixed-precision execution;
- optional streamed weight loading for lower peak memory usage;
- development-only CPU/CUDA kernel, training, and end-to-end prefill/decode benchmarks.

---

## Highlights

- **Rust-first, not Rust-only** implementation
  - Rust owns the framework structure and most high-level logic.
  - CUDA C++ is used for optional GPU acceleration.
  - CPU-only builds remain available without the `cuda` feature.
- **Dynamic autograd** built around tensor graph construction
- **Module-style abstraction** for model components
- **Separated layers / ops / models** for easier experimentation
- **Flexible precision system**
  - parameter dtype
  - runtime dtype
  - activation dtype
  - KV-cache dtype
- **Quantization-aware loading**
  - load float weights normally
  - quantize on load to `i8`
  - generate offline quantized safetensors
- **CPU and CUDA execution paths** with explicit kernel/backend work
- **Hugging Face `tokenizers`** integration
- **Safetensors** support with memory-mapped and streamed loading modes
- Release profile tuned with `lto`, `panic = "abort"`, and `strip`

---

## Rust and CUDA cooperation

Lumen uses Rust and CUDA for different layers of the system.

Rust is responsible for the high-level framework structure:

- tensor representation and dynamic autograd graph construction;
- module, layer, loss, optimizer, and model abstractions;
- dtype and runtime precision configuration;
- safetensors loading, tokenizer integration, and quantization flow;
- CPU execution paths and backend dispatch logic;
- CLI tools, benchmark tools, and inference/training orchestration;
- safe-ish wrappers around lower-level CUDA calls.

CUDA is used as an optional low-level acceleration backend:

- CUDA kernels live under `src/ops/cuda/lumen_cuda.cu`;
- the Rust side exposes CUDA-aware operation wrappers and calls the native CUDA functions through FFI;
- when the `cuda` feature is enabled, `build.rs` locates CUDA/cuDNN, invokes `nvcc`, builds the CUDA source into a native library, and links it with the Rust binary;
- CUDA handles GPU memory operations, cuBLAS/cuDNN calls, custom kernels, KV-cache updates, decode-oriented kernels, and selected forward/backward operations.

The intended division of responsibilities is:

```text
Rust side
  ├─ Tensor / autograd graph
  ├─ Layers, modules, losses, optimizers
  ├─ Llama model and runtime logic
  ├─ dtype / precision policy
  ├─ safetensors / tokenizer / quantization
  ├─ CPU kernels and backend dispatch
  └─ FFI wrappers for CUDA calls

CUDA side
  ├─ device memory allocation and reuse
  ├─ custom CUDA kernels
  ├─ cuBLAS-backed matrix operations
  ├─ optional cuDNN-backed primitives
  ├─ KV-cache and decode-oriented kernels
  └─ selected training/backward kernels
```

In other words, Lumen does not try to force every performance-critical operation into Rust. Rust manages the framework logic, type-level organization, runtime policy, and safety boundary, while CUDA is used where direct GPU execution is more appropriate.

CUDA support is optional and gated behind the `cuda` feature:

```bash
cargo build --release --features cuda
```

CPU-only builds do not require CUDA:

```bash
cargo build --release
```

Development benchmarks can combine `dev-tools` and `cuda`:

```bash
cargo build --release --features "dev-tools cuda" --bin cuda_cpu_bench
cargo build --release --features "dev-tools cuda" --bin prefill_decode_bench
```

---

## Repository layout

```text
src/
├─ autograd.rs              # Tensor + dynamic autograd core
├─ module.rs                # Module trait / macros
├─ loader.rs                # Safetensors loading and streamed loading
├─ tokenizer.rs             # Tokenizer wrapper
├─ precision.rs             # DType / runtime precision configuration
├─ ops/                     # Tensor ops, CPU kernels, and optional CUDA ops
│  └─ cuda/lumen_cuda.cu    # CUDA/cuDNN/cuBLAS-backed kernels
├─ layers/                  # Neural-network layers and attention building blocks
├─ models/llama.rs          # Llama model implementation
├─ main.rs                  # Minimal local inference CLI
└─ bin/
   ├─ quantize_safetensors.rs  # Offline quantization utility
   ├─ kernel_bench.rs          # Dev-only kernel benchmark
   ├─ prefill_decode_bench.rs  # Dev-only end-to-end benchmark
   └─ cuda_cpu_bench.rs        # Dev-only CPU/CUDA ops, NN, and backward benchmark
```

---

## Build

Release build:

```bash
cargo build --release
```

For better local CPU codegen:

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

PowerShell:

```powershell
$env:RUSTFLAGS = "-C target-cpu=native"
cargo build --release
```

Default release builds produce:

- `lumen`
- `quantize_safetensors`

Development benchmarks are intentionally gated behind `dev-tools`:

```bash
cargo build --release --features dev-tools --bin kernel_bench
cargo build --release --features dev-tools --bin prefill_decode_bench
cargo build --release --features dev-tools --bin cuda_cpu_bench
```

CUDA builds are intentionally gated behind the `cuda` feature:

```bash
cargo build --release --features cuda
cargo build --release --features "dev-tools cuda" --bin prefill_decode_bench
```

The build script searches CUDA from environment variables / `nvcc`, then common platform install paths.

cuDNN probing prefers an explicit or system install, then tries the Python `nvidia.cudnn` package. On Windows, a system cuDNN install such as `C:\Program Files\NVIDIA\CUDNN\...` is copied into the target directory for local runs.

---

## Running the minimal inference CLI

```bash
cargo run --release --bin lumen -- \
  --weights path/to/model.safetensors \
  --tokenizer path/to/tokenizer.json
```

Useful flags:

- `--system TEXT`
- `--temperature FLOAT`
- `--top-p FLOAT`
- `--repetition-penalty FLOAT`
- `--recent-window N`
- `--max-gen N`
- `--parameter-dtype f32|f16|bf16|i8`
- `--runtime-dtype f32|f16|bf16`
- `--activation-dtype f32|f16|bf16|i8`
- `--kv-cache-dtype f32|f16|bf16`
- `--quantize off|i8`
- `--quant-scale FLOAT`
- `--allow-parameter-copies`
- `--stream-weights`
- `--max-seq-len N`
- `--load-only`
- `--device cpu|cuda`

Example: BF16 runtime:

```bash
cargo run --release --bin lumen -- \
  --weights path/to/model.safetensors \
  --tokenizer path/to/tokenizer.json \
  --parameter-dtype bf16 \
  --runtime-dtype bf16 \
  --activation-dtype bf16 \
  --kv-cache-dtype bf16 \
  --allow-parameter-copies
```

Example: `i8` weights with BF16 runtime:

```bash
cargo run --release --bin lumen -- \
  --weights path/to/model.safetensors \
  --tokenizer path/to/tokenizer.json \
  --parameter-dtype i8 \
  --runtime-dtype bf16 \
  --activation-dtype i8 \
  --kv-cache-dtype bf16 \
  --quantize i8 \
  --allow-parameter-copies
```

You can print backend diagnostics during startup with:

```bash
LUMEN_SHOW_BACKENDS=1 cargo run --release --bin lumen -- \
  --weights path/to/model.safetensors \
  --tokenizer path/to/tokenizer.json
```

Interactive commands:

- `/reset` — clear chat history and KV cache
- `/exit` — quit

---

## Offline quantization

Generate an `i8` safetensors checkpoint ahead of time:

```bash
cargo run --release --bin quantize_safetensors -- \
  --input path/to/model.safetensors \
  --output path/to/model.i8.safetensors \
  --dtype i8
```

Optional manual scale:

```bash
cargo run --release --bin quantize_safetensors -- \
  --input path/to/model.safetensors \
  --output path/to/model.i8.safetensors \
  --dtype i8 \
  --scale 0.02
```

---

## Benchmark tools

### Kernel benchmark

```bash
cargo run --release --features "dev-tools x86-fp-kernels x86-int8-kernels" --bin kernel_bench -- \
  --iters 400 --samples 7 --hidden 2048 --inter 5632 --vocab 32000
```

### End-to-end prefill/decode benchmark

```bash
cargo run --release --features "dev-tools cuda" --bin prefill_decode_bench -- \
  --weights path/to/model.safetensors \
  --tokenizer path/to/tokenizer.json \
  --prompt "Explain Transformer KV cache." \
  --runs 5 --warmup 1 --max-gen 128 --mode greedy \
  --device cuda \
  --parameter-dtype bf16 \
  --activation-dtype bf16 \
  --kv-cache-dtype bf16 \
  --allow-parameter-copies
```

### CPU/CUDA ops and training benchmark

```bash
cargo run --release --features "dev-tools cuda" --bin cuda_cpu_bench -- \
  --suite all --size small --dtype bf16 --runs 5 --warmup 1 --check
```

Use `--release` for performance numbers.

Debug builds are useful for correctness but are not representative for speed.

---

## Representative performance on the current baseline

### Local environment used for the snapshot

The CUDA numbers below were collected on this local machine:

- OS: Microsoft Windows 11 Home China, `10.0.26200`, 64-bit
- CPU: AMD Ryzen 9 8945HX with Radeon Graphics
- RAM: 32.00 GB installed memory reported by Windows
- GPU: NVIDIA GeForce RTX 5070 Laptop GPU, 8 GB VRAM
- NVIDIA driver: `596.36`; runtime CUDA reported by `nvidia-smi`: `13.2`
- CUDA toolkit: `CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.0`; `nvcc 13.0.48`
- cuDNN: `9.21.1`; detected from `C:\Program Files\NVIDIA\CUDNN\v9.21\lib\13.2\x64\cudnn.lib`
- Rust toolchain: `stable-x86_64-pc-windows-msvc`; `rustc 1.89.0`; `cargo 1.89.0`

These numbers are a local snapshot, not a universal benchmark claim.

---

### CUDA snapshot

Local release run on 2026-05-01 with TinyLlama weights, `--device cuda`, BF16 parameters/activations/KV cache, greedy decode:

```shell
cargo run --release --features "dev-tools cuda" --bin cuda_cpu_bench -- --suite all --size small --dtype bf16 --runs 5 --warmup 1 --check
```

Small BF16 CPU/CUDA benchmark with correctness checks:

| Case | CPU | CUDA | Speedup |
|---|---:|---:|---:|
| `matmul.forward` | 0.873 ms | 0.034 ms | 25.99x |
| `softmax.forward` | 0.314 ms | 0.015 ms | 21.07x |
| `cross_entropy.backward` | 0.366 ms | 0.090 ms | 4.05x |
| `fused_gateup.forward` | 0.324 ms | 0.077 ms | 4.21x |
| `llama.train.backward` | 1.907 ms | 2.961 ms | 0.64x |
| `llama.train.step` | 1.718 ms | 2.973 ms | 0.58x |

Takeaway: CUDA already covers real inference, CUDA-only gradients, and training checks, but small training/backward and tiny fused-QKV cases still need more batching/fusion to beat CPU reliably.

---

### CPU snapshot

The following numbers come from the current **AVX-512 baseline** that successfully enables BF16 kernels on the author's machine.

Kernel-level snapshot observed during tuning:

- `backend: float=x86-avx512 int8=x86-avx2`
- `avx512_bf16_available=true`
- `matvec_bf16io ≈ 104 us`
- `fused_qkv ≈ 90 us`

These are not universal claims for every CPU. They are a snapshot of one working baseline on one machine.

---

### End-to-end snapshot

For a run with `prompt_tokens=60`, `max_gen=128`, `runs=5`, `warmup=1`:

| Configuration | Prefill forward | Decode forward | End-to-end decode |
|---|---:|---:|---:|
| BF16 | 140.70 tok/s | 19.09 tok/s | 17.64 tok/s |
| F16 | 131.89 tok/s | 14.99 tok/s | 14.04 tok/s |
| F32 | 44.56 tok/s | 11.18 tok/s | 9.86 tok/s |
| I8 weights + BF16 runtime | **203.66 tok/s** | **25.13 tok/s** | **23.16 tok/s** |

Practical takeaway on that machine:

- **BF16** is the recommended floating-point path on both CPU and CUDA today.
- **I8 weights + BF16 runtime** is the fastest tested configuration so far.
- **F16 is currently not the main optimization target**, since it underperforms BF16 in this implementation.

---

## Design notes and limitations

`src/main.rs` intentionally uses a **hard-coded `model_config()`** and a lightweight CLI.

That keeps the example easy to inspect, but it also means:

- the architecture must match the loaded checkpoint;
- adapting to a different model may require editing dimensions, layer counts, KV-head layout, or prompt formatting;
- this is a compact local runner, not a universal inference frontend.

Similarly, the benchmark tools are intended for **development and kernel tuning**, not polished public benchmarking infrastructure.

CUDA support is real but still evolving. Single CUDA device operation is the practical target today, while future multi-GPU work still needs explicit device-index plumbing through tensors, modules, and CUDA calls.

---

## Who this project is for

Lumen is a good fit if you want to:

- learn how a Rust tensor/autograd core can be structured;
- study a small Llama runtime without a huge framework wrapped around it;
- experiment with dtype management, quantization, and CPU/CUDA inference kernels;
- benchmark and tune a compact Rust inference stack on your own machine;
- inspect how a Rust-first runtime can call into CUDA for selected acceleration paths.

It is probably **not** the right fit if you need:

- large-scale training features;
- a mature serving system;
- mature multi-GPU deployment tooling;
- plug-and-play support for arbitrary model families.

---

## License

This repository is released under the license included in [`LICENSE`](./LICENSE).
