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
