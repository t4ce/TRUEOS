# LFM2.5 layer-0 FFN golden format

This host-only tool captures one deterministic layer-0 FFN evaluation from official
llama.cpp `b10075`, verifies it against the sealed TRUEGA native image, and emits the
checked-in golden binary plus HDL vectors. It is not part of TRUEOS and has no PCIe, JTAG,
flash, or FPGA runtime path.

The 64,000-byte `TGAGFFN1` artifact is little-endian:

| Offset | Bytes | Meaning |
| ---: | ---: | --- |
| `0x000` | 256 | fixed header and SHA-256 seals |
| `0x100` | 240 | five 48-byte vector descriptors |
| `0x1f0` | 16 | zero alignment padding |
| `0x200` | 63,488 | five contiguous F32 payloads |

The seal is SHA-256 over the complete artifact with its own 32-byte field zeroed. The
payload has a separate SHA-256, and `verify` checks both without needing the model files:

```sh
cargo run --manifest-path Cargo.toml --release -- \
  verify ../../artifacts/lfm25_layer0_ffn.golden.bin
```

`llama-b10075-ffn-trace.patch` is deliberately kept outside the ignored llama.cpp checkout.
The capture wrapper verifies the checkout commit and accepts either a clean source tree or
that exact already-applied patch.

The adjacent `.vectors` file contains row-0 native blocks for gate, up, and down, exact
signed integer dots, exact Q30 terms, captured-F32 row references, and the frozen Q30 error
bound. It is consumed directly by `truega_q8_0_gemv_tb.sv`.

## Sealed single-block runtime golden

`artifacts/lfm25_q8_block.golden.bin` is the canonical gate row-0/block-0 call used by
the runtime milestone. Generate it without rerunning llama.cpp or rebuilding the native
model image:

```sh
cargo run --manifest-path Cargo.toml --release -- \
  block ../../artifacts/lfm25_layer0_ffn.golden.bin \
  ../../artifacts/lfm25_layer0_ffn.golden.bin.vectors \
  ../../../../../tools/lfm2.5-350m/LFM2.5-350M-Q8_0.truega.bin \
  ../../artifacts/lfm25_q8_block.golden.bin

cargo run --manifest-path Cargo.toml --release -- \
  verify-block ../../artifacts/lfm25_q8_block.golden.bin \
  ../../artifacts/lfm25_layer0_ffn.golden.bin \
  ../../artifacts/lfm25_layer0_ffn.golden.bin.vectors
```

Generation verifies the complete native image against its pinned SHA-256 and extracts the
weight block at the generated model-contract offset. It independently requantizes the first
32 normalized-input values from the sealed FFN golden, checks both blocks against the
simulation vector, and recomputes the integer dot and Q30 term. Offline verification needs
only the three checked-in artifacts: it checks the FFN seal, the exact vectors-file hash,
the block payload hash and self-seal, then repeats the activation, dot, and Q30 derivation.
The generated `.sha256` file seals the complete 336-byte artifact for ordinary file checks.

The block artifact is little-endian:

| Offset | Bytes | Meaning |
| ---: | ---: | --- |
| `0x000` | 8 | `TGAQ8B01` magic |
| `0x008` | 8 | version, header/input/output sizes (`1`, `256`, `68`, `12`) |
| `0x010` | 16 | model generation and tensor/layer/role/row/block coordinates |
| `0x020` | 32 | complete sealed FFN-golden SHA-256 |
| `0x040` | 32 | pinned native-image SHA-256 |
| `0x060` | 32 | generated model-contract SHA-256 |
| `0x080` | 32 | exact simulation-vectors SHA-256 |
| `0x0a0` | 32 | SHA-256 of the 80-byte input/output payload |
| `0x0c0` | 32 | self-seal over the complete file with this field zeroed |
| `0x0e0` | 16 | native block offset, payload offsets, source-vector index, flags |
| `0x0f0` | 16 | zero reserved bytes |
| `0x100` | 68 | unchanged activation Q8_0 block followed by weight Q8_0 block |
| `0x144` | 12 | signed `i32` dot followed by signed `i64` Q30 term |

The checked-in call has dot `-14901`, Q30 term `-9429888`, payload SHA-256
`2faaf8b87bc3d121642d60b3c95019f61fc88b1bc17c7b264933d06fa3e8f1d1`, and complete-file
SHA-256 `d05cd8cd89f23dcdae758c7b8fe2a27a55d6ad8de60a33ade60c089da558eed2`.
