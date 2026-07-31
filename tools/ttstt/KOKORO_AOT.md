# Kokoro AOT compiler stepstone

`compile_kokoro_aot.py` is a model-specific, fail-closed host tool for the
prepared Kokoro graph. It does not attempt to be a general ONNX runtime.

The checked-in tool currently provides two deliberately separate results:

1. A complete audit/lowering inventory for the real 124 MB prepared graph.
2. A byte-exact v1 emitter exercised by a 1,651-byte synthetic program using
   the native `DynamicQuantizedGemm`, `ResolveDecoderShape`, and
   `DynamicQuantizedConv1d` opcodes.

The large ONNX graph, voice archive, and a large emitted program are not
checked in. The current v1 runtime validates operation records but does not yet
define canonical attributes or a math dispatcher for all 3,615 source nodes.
Consequently this tool does not claim that the real analysis is already an
executable Kokoro artifact. Real emission must stay gated until every admitted
opcode has that contract; using generic or invented ONNX attribute records
would weaken the fail-closed boundary.

## Reproduce the audit

Install ONNX only in the host tooling environment:

```sh
python3 -m venv .venv-kokoro-aot
.venv-kokoro-aot/bin/pip install 'onnx>=1.16,<2'
```

Then run:

```sh
.venv-kokoro-aot/bin/python tools/ttstt/compile_kokoro_aot.py analyze \
  crates/ttstt/.ttstt/models/kokoro/kokoro-rten.onnx \
  --voices crates/ttstt/.ttstt/models/kokoro/voices-v1.0.bin
```

The committed canonical result is
`tools/ttstt/kokoro_aot_analysis.json`. To update it intentionally, pass
`--report PATH --force`; review any change as a model-contract change.

The pinned inputs are:

| Input | Bytes | SHA-256 |
| --- | ---: | --- |
| `kokoro-rten.onnx` | 124,604,222 | `239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29` |
| `voices-v1.0.bin` | 28,214,398 | `bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d` |

Both hashes are independent fields in the sealed header. A matching graph
with a different voice archive is rejected because the style tensor's meaning
depends on that archive.

## Real-model result

The 2026-07-31 audit accepts exactly 3,615 nodes, 762 initializers, 3 inputs,
1 output, and 4,744 assigned tensors. Actual node domains are restricted to
3,591 standard ONNX nodes and 24 admitted Microsoft contrib nodes. Tensor
dtypes are restricted to F32, I32, I64, U8, I8, and Bool. Declared plus
operator-proven ranks cover every tensor and the maximum rank is four; 968
ranks required the custom propagation pass after ONNX shape information became
incomplete at STFT.

Every integer kernel has a recognized lowering:

| Chain | Count | Required structure |
| --- | ---: | --- |
| DynamicQuantizeLinear | 139 | per-tensor U8 activation, scale, zero point |
| MatMulInteger | 148 | DQL tuple + constant quantized weight/ZP + Cast + scale Mul |
| MatMul optional F32 bias | 136 | single constant-bias Add after dequantization |
| ConvInteger | 87 | DQL tuple + constant quantized weight/ZP + Cast + scale Mul |
| ConvInteger I32 bias | 80 | Div -> Floor -> Cast -> Reshape -> Add before F32 Cast/Mul |
| ConvInteger direct | 7 | no integer bias Add |

Any changed producer, fan-out, dtype, constant role, or epilogue rejects the
graph instead of falling back to generic math.

The analyzer assigns stable tensor IDs in source order, recognizes 338 safe
contiguous view candidates (141 Reshape, 192 Unsqueeze, 5 Squeeze), computes
half-open liveness intervals, extends direct owners across view lifetimes, and
classifies 2,126 phase-0-only, 2,036 phase-1-only, and 582 shared tensors. The
raw-plan peak is 770 simultaneously live storage owners. Dynamic views are not
emitted in v1; they must be materialized because v1 permits only static views.
The canonical tensor/alias/liveness plan hashes to
`cf1edfd4de99fea4a424f86a3e9cb89eb0c7e94140078bc7c2033fa5f48e6a81`;
the 235-entry native quantized lowering plan hashes to
`a949c04bfce049d0c26be6b8ad322d3b1a108b714c479ffe41fe1c094cdefd13`.

## Checked phase boundary

Phase 0 is source nodes `[0,1747)` and phase 1 is `[1747,3615)`. Lowering must
recompute the exclusive operation offset after fusion; `1747` is not written
blindly into a lowered program.

Node 1746, `/encoder/Gather_1`, produces the scalar INT64 tensor
`/encoder/Cast_1_output_0`. Its audited chain is:

```text
Sigmoid -> ReduceSum(axis=-1) -> Div(speed) -> Round -> Clip(min=1)
        -> Cast(INT64) -> Gather(batch=0) -> CumSum(axis=0) -> Gather(last)
```

This yields the total decoder frame count. Its sole consumer is node 1749,
`Range(0, frame_count, 1)`, the first frame-count-sized tensor. Of 1,868 phase-1
nodes, 1,862 descend from that scalar. The remaining six nodes (1747, 1748,
1750, 1752, 1755, and 1759) form companion alignment operands which join a
frame-dependent operation. No phase-1 value feeds phase 0 and no quant fusion
crosses the cut.

There is intentionally no second allocation barrier after source node 2067.
That point only finishes F0/N tensors shaped `[1,F]`, reveals no new size, and
would carry 120 live values after decoder work has already begun.

## Sealed fixture and tests

Generate and inspect the tiny artifact without installing ONNX:

```sh
python3 tools/ttstt/compile_kokoro_aot.py fixture /tmp/kokoro-fixture.kkaot
python3 tools/ttstt/compile_kokoro_aot.py inspect /tmp/kokoro-fixture.kkaot
python3 -m unittest tools.ttstt.test_compile_kokoro_aot -v
```

With the local model and ONNX tooling installed, the same test module also runs
the full pinned audit. The fixture covers the 352-byte header, independent
model/voice hashes, a whole-artifact SHA-256 seal (with only the seal field
zeroed while hashing), six canonical aligned sections,
constants in DATA, a static view, fixed and affine frame-count slots, two phase
records, bindings, and native quantized opcodes. Payload, provenance, directory,
and reserved-header tampering are negative tests.

The matching no-std reader lives in `crates/trueos-kokoro-aot`. The unittest
invokes its inspector automatically when Cargo is available. To run that half
manually, start Cargo outside the checkout so the kernel workspace's custom
target/build-std configuration does not apply:

```sh
trueos_root="$PWD"
(cd /tmp && cargo run --manifest-path \
  "$trueos_root/crates/trueos-kokoro-aot/Cargo.toml" \
  --example inspect -- /tmp/kokoro-fixture.kkaot 16)
```
