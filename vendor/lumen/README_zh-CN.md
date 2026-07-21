# Lumen

> 一个以 Rust 为主体的轻量级深度学习核心，包含动态自动微分、灵活 dtype 控制、safetensors 加载、量化推理，以及 CPU/CUDA Llama runtime 实验。

[English README](./README.md)

---

## 项目定位

Lumen 是一个**以 Rust 为主体的轻量级机器学习运行时 / 深度学习核心**。它把下面几层内容放在了同一个仓库中：

- Tensor 核心与动态自动微分；
- 可复用的 Layer、Module、Loss、Optimizer；
- Llama 风格的 decoder 模型实现；
- safetensors 权重加载与可选流式加载；
- 参数、激活、KV cache 的运行时 dtype 控制；
- 可选的 on-load 或 offline `i8` 量化；
- CPU 执行路径与可选 CUDA 加速；
- 用于 CPU/CUDA kernel 调优和端到端推理测试的 benchmark 工具。

这个项目更适合被理解为：

- 一个**以学习和实验为导向的 Rust-first 深度学习核心**；
- 一个围绕紧凑 Llama runtime 构建的 **CPU/CUDA LLM 推理实验平台**。

它不是完整训练框架，也不是生产级推理服务系统，更不是可以直接适配任意 checkpoint 的通用启动器。

需要特别说明的是：**Lumen 并不是纯 Rust 项目**。高层 runtime、tensor/autograd 系统、模型代码、loader、tokenizer wrapper、dtype 策略和 CPU backend 主要由 Rust 编写；而可选 CUDA backend 使用 CUDA C++ kernel 与 NVIDIA 库，并通过 FFI 边界与 Rust 侧配合。

更准确的描述是：

> Rust-first, not Rust-only.

也就是：**以 Rust 为主体，但不是只使用 Rust**。

---

## 当前重点

目前项目已经有较完整的 CPU 路径，同时 CUDA 路径也在持续完善。

当前代码库中比较重要的部分包括：

- 动态自动微分和通用 Tensor ops；
- Llama-family decoder，包括 RMSNorm、RoPE、GQA、SwiGLU-style MLP 和 KV-cache decode；
- 对 `f32`、`f16`、`bf16`、`i8` 的存储、加载与运行时配置支持；
- 通过 `cuda` feature 开启的可选 CUDA 执行路径；
- cuDNN 检测逻辑：优先使用显式或系统安装，也可以回退到 Python `nvidia.cudnn`；
- CUDA-resident tensor、KV-cache 更新、forward decode，以及逐步扩展中的训练 / backward 路径；
- 可选的参数 dtype copy，用于加速混合精度执行；
- 可选流式权重加载，用于降低峰值内存占用；
- 开发用 CPU/CUDA kernel、训练和端到端 prefill/decode benchmark。

---

## 主要特点

- **Rust-first, not Rust-only**
  - Rust 负责框架结构和大部分高层逻辑；
  - CUDA C++ 负责可选 GPU 加速；
  - 不开启 `cuda` feature 时，仍然可以使用 CPU-only 构建。
- **动态自动微分**
  - 通过 Tensor 计算过程构建 autograd graph。
- **Module 风格抽象**
  - 用于组织 Layer、Model、Loss、Optimizer 等结构。
- **清晰拆分的 layers / ops / models**
  - 便于做实验和替换实现。
- **灵活精度系统**
  - parameter dtype；
  - runtime dtype；
  - activation dtype；
  - KV-cache dtype。
- **量化感知加载**
  - 正常加载 float 权重；
  - 加载时量化为 `i8`；
  - 生成 offline quantized safetensors。
- **CPU 与 CUDA 执行路径**
  - 同时保留 CPU kernel 调优和 CUDA kernel 调优空间。
- **Hugging Face `tokenizers` 集成**
- **Safetensors 支持**
  - 支持 mmap 加载和流式加载。
- Release profile 使用 `lto`、`panic = "abort"`、`strip` 等优化选项。

---

## Rust 与 CUDA 如何配合

Lumen 中 Rust 和 CUDA 分别负责系统的不同层次。

Rust 负责高层框架结构：

- Tensor 表示与动态自动微分图构建；
- Module、Layer、Loss、Optimizer、Model 抽象；
- dtype 和运行时精度配置；
- safetensors 加载、tokenizer 集成与量化流程；
- CPU 执行路径和 backend dispatch 逻辑；
- CLI、benchmark 工具、推理 / 训练流程组织；
- 对底层 CUDA 调用的封装。

CUDA 作为可选低层加速后端：

- CUDA kernel 位于 `src/ops/cuda/lumen_cuda.cu`；
- Rust 侧通过 CUDA-aware operation wrapper 调用 native CUDA 函数；
- 开启 `cuda` feature 后，`build.rs` 会查找 CUDA/cuDNN，调用 `nvcc` 编译 CUDA 源文件，并将生成的 native library 链接进 Rust binary；
- CUDA 侧负责 GPU memory 操作、cuBLAS/cuDNN 调用、自定义 kernel、KV-cache 更新、decode-oriented kernel，以及部分 forward/backward 操作。

整体分工可以理解为：

```text
Rust 侧
  ├─ Tensor / autograd graph
  ├─ Layers、modules、losses、optimizers
  ├─ Llama model 与 runtime 逻辑
  ├─ dtype / precision 策略
  ├─ safetensors / tokenizer / quantization
  ├─ CPU kernels 与 backend dispatch
  └─ CUDA FFI wrappers

CUDA 侧
  ├─ device memory 分配与复用
  ├─ 自定义 CUDA kernels
  ├─ cuBLAS 矩阵运算
  ├─ 可选 cuDNN primitives
  ├─ KV-cache 与 decode-oriented kernels
  └─ 部分训练 / backward kernels
```

换句话说，Lumen 并不是强行把所有性能关键代码都写成 Rust。Rust 负责框架逻辑、类型组织、运行时策略和安全边界；CUDA 则负责那些更适合直接在 GPU 上执行的计算路径。

CUDA 支持是可选的，并且由 `cuda` feature 控制：

```bash
cargo build --release --features cuda
```

CPU-only 构建不需要 CUDA：

```bash
cargo build --release
```

开发 benchmark 可以同时开启 `dev-tools` 和 `cuda`：

```bash
cargo build --release --features "dev-tools cuda" --bin cuda_cpu_bench
cargo build --release --features "dev-tools cuda" --bin prefill_decode_bench
```

---

## 仓库结构

```text
src/
├─ autograd.rs              # Tensor + 动态自动微分核心
├─ module.rs                # Module trait / macros
├─ loader.rs                # Safetensors 加载与流式加载
├─ tokenizer.rs             # Tokenizer wrapper
├─ precision.rs             # DType / runtime precision 配置
├─ ops/                     # Tensor ops、CPU kernels、可选 CUDA ops
│  └─ cuda/lumen_cuda.cu    # CUDA/cuDNN/cuBLAS-backed kernels
├─ layers/                  # 神经网络 layers 与 attention 组件
├─ models/llama.rs          # Llama 模型实现
├─ main.rs                  # 最小本地推理 CLI
└─ bin/
   ├─ quantize_safetensors.rs  # Offline quantization 工具
   ├─ kernel_bench.rs          # 开发用 kernel benchmark
   ├─ prefill_decode_bench.rs  # 开发用端到端 benchmark
   └─ cuda_cpu_bench.rs        # 开发用 CPU/CUDA ops、NN、backward benchmark
```

---

## 构建

Release 构建：

```bash
cargo build --release
```

如果希望启用更适合本机 CPU 的 codegen：

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

PowerShell：

```powershell
$env:RUSTFLAGS = "-C target-cpu=native"
cargo build --release
```

默认 release 构建会生成：

- `lumen`
- `quantize_safetensors`

开发 benchmark 默认通过 `dev-tools` feature 隔离：

```bash
cargo build --release --features dev-tools --bin kernel_bench
cargo build --release --features dev-tools --bin prefill_decode_bench
cargo build --release --features dev-tools --bin cuda_cpu_bench
```

CUDA 构建通过 `cuda` feature 开启：

```bash
cargo build --release --features cuda
cargo build --release --features "dev-tools cuda" --bin prefill_decode_bench
```

构建脚本会从环境变量、`nvcc` 和常见系统安装路径中查找 CUDA。

cuDNN 探测逻辑会优先使用显式或系统安装，然后尝试 Python `nvidia.cudnn` package。在 Windows 上，如果检测到类似 `C:\Program Files\NVIDIA\CUDNN\...` 的系统 cuDNN 安装，会把相关文件复制到 target 目录，方便本地运行。

---

## 运行最小推理 CLI

```bash
cargo run --release --bin lumen -- \
  --weights path/to/model.safetensors \
  --tokenizer path/to/tokenizer.json
```

常用参数：

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

示例：BF16 runtime：

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

示例：`i8` weights + BF16 runtime：

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

启动时可以通过下面的环境变量打印 backend 诊断信息：

```bash
LUMEN_SHOW_BACKENDS=1 cargo run --release --bin lumen -- \
  --weights path/to/model.safetensors \
  --tokenizer path/to/tokenizer.json
```

交互命令：

- `/reset`：清空 chat history 和 KV cache；
- `/exit`：退出程序。

---

## Offline quantization

提前生成 `i8` safetensors checkpoint：

```bash
cargo run --release --bin quantize_safetensors -- \
  --input path/to/model.safetensors \
  --output path/to/model.i8.safetensors \
  --dtype i8
```

手动指定 scale：

```bash
cargo run --release --bin quantize_safetensors -- \
  --input path/to/model.safetensors \
  --output path/to/model.i8.safetensors \
  --dtype i8 \
  --scale 0.02
```

---

## Benchmark 工具

### Kernel benchmark

```bash
cargo run --release --features "dev-tools x86-fp-kernels x86-int8-kernels" --bin kernel_bench -- \
  --iters 400 --samples 7 --hidden 2048 --inter 5632 --vocab 32000
```

### 端到端 prefill/decode benchmark

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

### CPU/CUDA ops 与训练 benchmark

```bash
cargo run --release --features "dev-tools cuda" --bin cuda_cpu_bench -- \
  --suite all --size small --dtype bf16 --runs 5 --warmup 1 --check
```

性能测试请使用 `--release`。

Debug build 适合检查正确性，但不能代表真实性能。

---

## 当前基线性能快照

### 本地测试环境

下面的 CUDA 数字来自作者本地机器：

- OS：Microsoft Windows 11 Home China，`10.0.26200`，64-bit
- CPU：AMD Ryzen 9 8945HX with Radeon Graphics
- RAM：32.00 GB installed memory
- GPU：NVIDIA GeForce RTX 5070 Laptop GPU，8 GB VRAM
- NVIDIA driver：`596.36`；`nvidia-smi` 报告 runtime CUDA：`13.2`
- CUDA toolkit：`CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.0`；`nvcc 13.0.48`
- cuDNN：`9.21.1`；检测路径为 `C:\Program Files\NVIDIA\CUDNN\v9.21\lib\13.2\x64\cudnn.lib`
- Rust toolchain：`stable-x86_64-pc-windows-msvc`；`rustc 1.89.0`；`cargo 1.89.0`

这些数字只是本地快照，不是通用 benchmark 结论。

---

### CUDA 快照

2026-05-01 本地 release 运行，使用 TinyLlama 权重，`--device cuda`，BF16 parameters / activations / KV cache，greedy decode：

```shell
cargo run --release --features "dev-tools cuda" --bin cuda_cpu_bench -- --suite all --size small --dtype bf16 --runs 5 --warmup 1 --check
```

Small BF16 CPU/CUDA benchmark，带 correctness check：

| Case | CPU | CUDA | Speedup |
|---|---:|---:|---:|
| `matmul.forward` | 0.873 ms | 0.034 ms | 25.99x |
| `softmax.forward` | 0.314 ms | 0.015 ms | 21.07x |
| `cross_entropy.backward` | 0.366 ms | 0.090 ms | 4.05x |
| `fused_gateup.forward` | 0.324 ms | 0.077 ms | 4.21x |
| `llama.train.backward` | 1.907 ms | 2.961 ms | 0.64x |
| `llama.train.step` | 1.718 ms | 2.973 ms | 0.58x |

结论：CUDA 路径已经覆盖了真实推理、CUDA-only gradients 和训练检查；但小规模 training/backward 和很小的 fused-QKV case 仍然需要更多 batching / fusion，才能稳定超过 CPU。

---

### CPU 快照

下面数据来自当前已经成功启用 BF16 kernels 的 **AVX-512 baseline**。

kernel-level 调优中观察到：

- `backend: float=x86-avx512 int8=x86-avx2`
- `avx512_bf16_available=true`
- `matvec_bf16io ≈ 104 us`
- `fused_qkv ≈ 90 us`

这些不是所有 CPU 上都成立的通用结论，而是某台机器上的可工作基线快照。

---

### 端到端快照

某次运行配置：`prompt_tokens=60`，`max_gen=128`，`runs=5`，`warmup=1`。

| Configuration | Prefill forward | Decode forward | End-to-end decode |
|---|---:|---:|---:|
| BF16 | 140.70 tok/s | 19.09 tok/s | 17.64 tok/s |
| F16 | 131.89 tok/s | 14.99 tok/s | 14.04 tok/s |
| F32 | 44.56 tok/s | 11.18 tok/s | 9.86 tok/s |
| I8 weights + BF16 runtime | **203.66 tok/s** | **25.13 tok/s** | **23.16 tok/s** |

在该机器上的实践结论：

- **BF16** 是当前 CPU 和 CUDA 路径中更推荐的 floating-point 配置；
- **I8 weights + BF16 runtime** 是目前测试过的最快配置；
- **F16 目前不是主要优化目标**，因为在当前实现中它落后于 BF16。

---

## 设计说明与限制

`src/main.rs` 目前有一个**硬编码的 `model_config()`**，并使用较轻量的 CLI。

这样做的好处是示例更容易检查，但也意味着：

- 模型结构必须和加载的 checkpoint 匹配；
- 如果要适配不同模型，可能需要修改 hidden size、layer 数、KV-head layout 或 prompt formatting；
- 这更像是一个紧凑的本地 runner，而不是通用推理前端。

同样，benchmark 工具主要服务于**开发和 kernel 调优**，不是精心包装的公开 benchmark 基础设施。

CUDA 支持是真实存在的，但仍在持续演进。当前实际目标是单 CUDA device；未来如果要支持多 GPU，还需要把 device index 明确传递到 tensor、module 和 CUDA call 中。

---

## 适合谁使用

Lumen 比较适合你在以下场景中使用：

- 学习 Rust tensor / autograd core 如何组织；
- 研究一个没有大型框架包裹的轻量 Llama runtime；
- 实验 dtype 管理、量化、CPU/CUDA inference kernels；
- 在自己的机器上 benchmark 和调优一个紧凑 Rust inference stack；
- 观察 Rust-first runtime 如何通过 CUDA 实现特定路径加速。

它可能不适合以下需求：

- 大规模训练；
- 成熟的生产级 serving；
- 完整多 GPU 部署工具；
- 任意模型家族的 plug-and-play 支持。

---

## License

本仓库使用 [`LICENSE`](./LICENSE) 中包含的许可证发布。
