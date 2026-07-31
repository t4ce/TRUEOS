# Kokoro AOT compiler stepstone

`compile_kokoro_aot.py` is a model-specific, fail-closed host tool for the
prepared Kokoro graph. It does not attempt to be a general ONNX runtime.

The checked-in tool currently provides three deliberately separate results:

1. A complete audit/lowering inventory for the real 124 MB prepared graph.
2. Canonical, versioned attribute/lowering records for the CPU math kernels,
   materialized layout kernels, and no-copy views which have concrete Rust
   implementations.
3. A byte-exact v1 emitter exercised by a 1,651-byte synthetic program using
   the native `DynamicQuantizedGemm`, `ResolveDecoderShape`, and
   `DynamicQuantizedConv1d` opcodes.

The large ONNX graph, voice archive, and a large emitted program are not
checked in. The current v1 runtime treats operation attributes as opaque and
does not yet have a math dispatcher for all 3,615 source nodes. Consequently
this tool does not claim that the real analysis is already an executable
Kokoro artifact. The new records are a sealed input to that later wiring, not
a substitute for it. Real emission stays gated until every emitted opcode has
a runtime contract.

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
No full graph is emitted in this stepstone, so no artifact instantiates those
runtime-resolved descriptors yet.
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

Operations outside this admitted set still use the outer format's canonical
empty form (`attribute_offset=0`, `attribute_len=0`).

Each kind then has a fixed-size body. Reserved bytes must be zero, the outer op
record length must equal `total_record_bytes`, and the kind must equal the
outer opcode. The binary bodies carry input/output ranks and checked ONNX
multidirectional-broadcast semantics; reduction/normalization records carry
axis, exact f32 epsilon bits, and layout contracts; `LeakyRelu` carries exact
f32 alpha bits. Parameterless unary operations use the eight-byte header as
their complete record. View bodies carry dtype, ranks, alias/static-control
flags, `allowzero`, and up to four exact shape/axis integers. A dynamic Reshape
body carries no invented dimensions and retains its controller tensor binding.

The pinned graph admits exactly 2,696 records:

| Lane | Records | Detail |
| --- | ---: | --- |
| Existing f32 core | 1,629 | 1,444 Add/Mul/Div/Sub, 130 ReduceMean, 19 LayerNormalization, 12 Softmax, 12 FastGelu, 12 SkipLayerNormalization |
| f32 unary extension | 256 | 90 Sqrt, 81 Floor, 51 Sin, 28 LeakyRelu, and six singleton unary nodes |
| Materialized layout | 473 | 88 Transpose, 135 Gather, 72 Concat, 74 two-output Split, 5 Expand, 73 Shape, 22 Slice, 2 reflect Pad, 1 NonZero, 1 ScatterND |
| View aliases | 338 | 141 Reshape, 192 Unsqueeze, 5 Squeeze |

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

The complete canonical stream includes source node index, phase, opcode,
tensor-ID bindings, and attribute bytes. It hashes to
`fd91172b0b96ae5f22a353664d381b58d4e708a6effaca2b183332e63d0b9920`.
The compiler explicitly leaves 80 INT32 and one INT64 Add outside the f32
lane. Any admitted arity, domain, dtype, rank, broadcast, axis, epsilon,
alpha, parameter-width, view-control, or alias-root change is rejected.

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

The separate 14,260-byte attribute fixture contains 32 op records: one for
each admitted opcode kind across binary, unary, normalization, contrib, and
layout/view lanes. Its whole-file SHA-256 is
`c28964ad9f347ec5df9ba3bd2d583d14aa9da9124d2e828a983bf1474c3a0084` and
its canonical artifact seal is
`eb620cfaade0098dc6f63f5053f08094c1c9a3a935371f5edddbd24dea8261a4`.
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
