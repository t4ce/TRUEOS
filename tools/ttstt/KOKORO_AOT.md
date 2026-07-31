# Kokoro AOT compiler stepstone

`compile_kokoro_aot.py` is a model-specific, fail-closed host tool for the
prepared Kokoro graph. It does not attempt to be a general ONNX runtime.

The checked-in tool currently provides three deliberately separate results:

1. A complete audit/lowering inventory for the real 124 MB prepared graph.
2. A complete canonical lowering inventory: every one of the 3,615 source
   nodes is owned exactly once by a typed scalar/f32/layout record, one of 235
   native quantized fusions, or the native duration resolver.
3. A byte-exact v1 emitter exercised by a 1,651-byte synthetic program using
   the native `DynamicQuantizedGemm`, `ResolveDecoderShape`, and
   `DynamicQuantizedConv1d` opcodes.

The large ONNX graph, voice archive, and a large emitted program are not
checked in. This step does not assign arena slots, tensor capacities, a
fixed maximum frame count, or runtime logical shapes. Consequently it does
not claim that the real analysis is already an executable Kokoro artifact.
The complete lowering stream is a sealed input to that next planning step;
real emission remains gated (`executable_graph_emitted=false`).

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
contiguous view aliases (141 Reshape, 192 Unsqueeze, 5 Squeeze), computes
half-open liveness intervals, extends direct owners across view lifetimes, and
classifies 2,126 phase-0-only, 2,036 phase-1-only, and 582 shared tensors. The
raw-plan peak is 770 simultaneously live storage owners. Of those view
operators, 287 have initializer control tensors and 51 Reshape target tensors
come from runtime `Concat` nodes. All 338 have a statically proven rank and
alias root, but the latter 51 are not falsely described as compile-time-shaped.
After quant fusion, 258 view records survive in the complete lowered stream;
the other 80 Reshapes are owned by quantized Conv bias fusions. No full graph
is emitted in this stepstone, so no artifact instantiates those runtime-
resolved descriptors yet.
The canonical tensor/alias/liveness plan hashes to
`8bad04023c1aa2d2810646ea4558942e23e8aab1bd901c6cb8d292845db2c653`;
the 235-entry native quantized lowering plan hashes to
`a949c04bfce049d0c26be6b8ad322d3b1a108b714c479ffe41fe1c094cdefd13`.

## CPU attribute ABI and admitted records

Every non-empty operation-attribute record begins with this little-endian,
four-byte-aligned header:

```text
u16 abi_version = 1
u16 kind = operation opcode
u32 total_record_bytes (including this header)
```

Each kind then has a fixed-size body. Reserved bytes must be zero, the outer op
record length must equal `total_record_bytes`, and the kind must equal the
outer opcode. The binary bodies carry input/output ranks and checked ONNX
multidirectional-broadcast semantics; reduction/normalization records carry
axis, exact f32 epsilon bits, and layout contracts; `LeakyRelu` carries exact
f32 alpha bits. Parameterless unary operations use the eight-byte header as
their complete record. View bodies carry dtype, ranks, alias/static-control
flags, `allowzero`, and up to four exact shape/axis integers. A dynamic Reshape
body carries no invented dimensions and retains its controller tensor binding.

The v1 typed bodies now also cover Bool/I64 comparisons, the I64 Add variant,
Cast, ConstantOfShape, CumSum, DequantizeLinear, Where, MatMul, Pow, Range,
Resize, `BiLstm256`, float Conv/ConvTranspose, `FixedStft20`, both native
quantized kernels, and `ResolveDecoderShape`. Native records bind original
float activations plus constant weight/scale/zero/bias roles; their profile
IDs seal the exact model-specific ranks, dtypes and attributes.

The earlier raw pass still validates 2,696 potential records (1,885 f32 and
811 layout/view). Native-chain ownership removes overlaps and yields exactly
2,227 non-overlapping lowered operations:

| Lane | Lowered ops | Source nodes owned |
| --- | ---: | ---: |
| Surviving original scalar/f32/layout records | 1,845 | 1,845 |
| Direct typed residual profiles | 146 | 146 |
| DynamicQuantizedGemm/Conv1d | 235 | 1,615 |
| ResolveDecoderShape | 1 | 9 |
| **Total** | **2,227** | **3,615** |

The 146 direct residual profiles comprise one I64 Add, And, 17 Cast, six
ConstantOfShape, one CumSum, four DequantizeLinear, the comparison/Where
control operations, 27 MatMul, 50 Pow, two Range, six Resize, six BiLSTM,
one float Conv, six float ConvTranspose, and one fixed STFT. There is no
generic fallback: an unsupported profile, unowned source node, or duplicate
owner rejects the model.

The 811-record layout lane resolves rank gaps through the compiler's propagated
descriptors. Its records preserve exact permutations and axes, both Split
lengths, Shape start/end presence, Expand-controller provenance, all four Slice
control slots and their static/dynamic provenance, and reflect-pad vectors.
The admitted Slice forms have one selected axis and a default or explicit step
of positive one; a negative, dynamic, or non-unit step is rejected. Pad admits
only `mode="reflect"` with non-negative in-range pads. Every materialized
layout result must match the input/output dtype and propagated rank contract,
and ranks above four are rejected.

The terminal iSTFT `NonZero` is restricted to rank-one Bool input and emits
row-major INT64 coordinates shaped `[1,count]`. The compiler propagates static
shape-vector lengths through the adjacent `Shape`, `Slice`, and `Concat` nodes;
that proves the otherwise-undeclared updates `Reshape` is rank two, not rank
three. `ScatterND` is then admitted only as the exact rank-2 data, rank-3
indices, rank-2 updates, two-element index-tuple tail with `reduction="none"`.
Both index-construction branches must trace back to the same `NonZero` node.
The record also requires strictly increasing, unique destinations; the runtime
kernel checks that precondition on every invocation. The canonical 16-phoneme
trace produces destinations `[1..19]`.

The `KKLOWER2` canonical stream includes anchor source index, an ordered list
of source nodes owned by the record, phase, opcode, tensor-ID bindings, and
typed attribute bytes. Its complete plan SHA-256 is
`7ea9436430722d1ce9ddfad00f579883f6628e9f4518f66f464eb5d3e6b5c463`.
The source-to-lowered-op ownership map hashes to
`248cbd89b62638bd1fc2ba16649e46d35204cc5fe7abfc1e799620ea2a166ac4`;
all 3,615 source nodes have count one. The pre-fusion/raw plan independently
hashes to
`93fcb54122768eab1d40abb0e84fe69a46433688bd4a61243dba69dc57b03b51`.

## Checked phase boundary

Phase 0 is source nodes `[0,1747)` and phase 1 is `[1747,3615)`. After fusion,
the recomputed lowered ranges are `[0,1079)` and `[1079,2227)`; source index
`1747` is not written blindly into a lowered program.

Node 1746, `/encoder/Gather_1`, produces the scalar INT64 tensor
`/encoder/Cast_1_output_0`. Its audited chain is:

```text
Sigmoid -> ReduceSum(axis=-1) -> Div(speed) -> Round -> Clip(min=1)
        -> Cast(INT64) -> Gather(batch=0) -> CumSum(axis=0) -> Gather(last)
```

Those nine nodes are one `ResolveDecoderShape` record. It binds the biased
duration logits and `speed`, preserves `/encoder/CumSum_output_0` as an INT64
rank-one output for later consumers, and exposes
`/encoder/Cast_1_output_0` explicitly as the returned INT64 frame scalar.
That scalar's sole consumer is node 1749,
`Range(0, frame_count, 1)`, the first frame-count-sized tensor. Of 1,868 phase-1
nodes, 1,862 descend from that scalar. The remaining six nodes (1747, 1748,
1750, 1752, 1755, and 1759) form companion alignment operands which join a
frame-dependent operation. No phase-1 value feeds phase 0 and no quant fusion
crosses the cut.

There is intentionally no second allocation barrier after source node 2067.
That point only finishes F0/N tensors shaped `[1,F]`, reveals no new size, and
would carry 120 live values after decoder work has already begun.

## Deferred frame-capacity planning

`F_max` is intentionally not part of this compiler result. Exact pinned-model
ORT measurements on the i9 for a repeated legal 14-token phrase currently
give these sizing points:

| Tokens | Speed | Audio samples | Decoder frames | Audio seconds |
| ---: | ---: | ---: | ---: | ---: |
| 252 | 1.0 | 372,600 | 1,242 | 15.525 |
| 448 | 1.0 | 625,800 | 2,086 | 26.075 |
| 252 | 0.5 | 744,600 | 2,482 | 31.025 |

The intended native text policy targets 175–250 tokens, may grow to 450 at a
sentence boundary, and uses 510 only as the graph hard fail-safe. `F_max=8192`
is therefore a candidate, not a sealed choice. The next step must run the
post-fusion allocator against that bound, prove the resulting memory budget,
and pair fixed capacities with runtime logical tensor shapes before real AOT
artifact emission can be enabled.

## Sealed fixture and tests

Generate and inspect the tiny artifact without installing ONNX:

```sh
python3 tools/ttstt/compile_kokoro_aot.py fixture /tmp/kokoro-fixture.kkaot
python3 tools/ttstt/compile_kokoro_aot.py inspect /tmp/kokoro-fixture.kkaot
python3 tools/ttstt/compile_kokoro_aot.py attribute-fixture \
  /tmp/kokoro-attributes.kkaot
python3 tools/ttstt/compile_kokoro_aot.py inspect \
  /tmp/kokoro-attributes.kkaot
python3 -m unittest tools.ttstt.test_compile_kokoro_aot -v
```

With the local model and ONNX tooling installed, the same test module also runs
the full pinned audit. The fixture covers the 352-byte header, independent
model/voice hashes, a whole-artifact SHA-256 seal (with only the seal field
zeroed while hashing), six canonical aligned sections,
constants in DATA, a static view, fixed and affine frame-count slots, two phase
records, bindings, and native quantized opcodes. Payload, provenance, directory,
and reserved-header tampering are negative tests.

The separate 27,500-byte attribute fixture contains 56 op records, including
all newly admitted scalar/control and native profiles plus distinct f32/I64
Add and three Cast dtype variants. Its whole-file SHA-256 is
`76b3b6f833b7cdbf4933b0985ad081ebe7b543b21d6d7c68ca9ff3ad19b603e9` and
its canonical artifact seal is
`52b9d668b82cf015259ae8ecbf3001d6719048ffdce15b04f642b4f009480a0d`.
Python validates each body fail-closed; the existing Rust reader parses the
same artifact as opaque attribute slices, ready for a later typed decoder.

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
