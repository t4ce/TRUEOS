# LFM2.5-350M packed DP4A design

## Boundary

This is the first model-specific step toward the resident GPU graph. The local
Ubuntu lane consumes SPIR-V through Intel NEO/IGC; after its parity and
performance gates pass, the same fixed ABI is selected by the TRUEOS GuC/RCS
runtime. Shell2 `lum` itself is unchanged.

The model contract is fixed:

- 16 layers: ten short-convolution and six attention
- hidden width 1,024; FFN width 4,608; vocabulary 65,536
- 93 Q8 projections and 376,569,856 Q8 weight bytes per evaluated token
- 93 Q8 tensors and 55 non-Q8 tensors
- context 256 for the first resident graph

No generic tensor planner, framework, FPGA path, or alternate model layout is
admitted.

## Weight ABI

Let:

- `B = columns / 32` Q8 blocks per row
- `P = B / 2` block pairs per row
- `T = row / 16` and `L = row % 16`
- `p = block / 2` and `q = block % 2`
- `g = 0..7`, selecting four signed-int8 values

Each `(T, p)` unit is exactly 1,088 bytes:

```text
offset   bytes   contents
0          32   block 0 binary16 scales for lanes 0..15
32         32   block 1 binary16 scales for lanes 0..15
64        512   block 0 qwords: [g=0..7][lane=0..15]
576       512   block 1 qwords: [g=0..7][lane=0..15]
```

The byte addresses are:

```text
pair  = tensor_offset + (T * P + p) * 1088
scale = pair + q * 32 + L * 2
qword = pair + 64 + q * 512 + g * 64 + L * 4
```

The unit is seventeen 64-byte cache lines. Every SIMD16 qword vector begins on
a cache-line boundary. Its payload is exactly two native
`16 * (2 + 32)`-byte tiles, so tensor offsets, tensor extents, and total image
size do not change.

The packer copies every binary16 scale bit-for-bit. The sealed model contains
25,994 subnormal scales, so decoding and re-encoding scales is forbidden. The
fixed model contains no `-128` Q8 value.

The deterministic packed image is:

```text
bytes:   376701952
sha256:  90876f02e0cc224fe23e01c8739dcbb94d7bcc8fbfa3d36204c6267a440f5fd8
```

## Activation ABI

For `B` blocks, the activation is:

```text
uint scale_slot[B]
uint qword[B][8]
```

Only the low 16 bits of a scale slot are populated. Total storage remains
36 bytes per block: 1,152 bytes at K=1,024 and 5,184 bytes at K=4,608. The
qword region begins on a 64-byte boundary for both fixed widths. The future GPU
quantizer must produce this layout directly.

## Arithmetic invariant

Each output lane retains eight independent float accumulators. For every Q8
block, IGC emits eight signed SIMD16 DP4A instructions, one for each group of
four values. Each integer result is converted to float and accumulated with:

```text
sum[g] = fma(weight_scale * activation_scale, float(dot4[g]), sum[g])
```

The final reduction remains:

```text
low0 = sum0 + sum4
low1 = sum1 + sum5
low2 = sum2 + sum6
low3 = sum3 + sum7
out  = (low0 + low2) + (low1 + low3)
```

The existing oracle saturates each signed pair to int16 before adding two
pairs. Host activations and the sealed weights are both restricted to
`[-127, 127]`; therefore pair saturation cannot trigger and DP4A is
integer-equivalent. Collapsing the eight integer or float accumulators is not
permitted because it changes rounding.

## Local execution lane

`lfm25-fixed --igpu-packed` performs:

1. sealed native-image and contract admission;
2. deterministic model packing and hash verification;
3. SPIR-V ingestion through `clCreateProgramWithIL`;
4. host-specific executable extraction and hashing;
5. one NEO-owned packed model buffer;
6. packed activation upload, SIMD16 dispatch, result readback and event timing.

The runner reports logical weight bytes and effective GB/s in addition to
kernel time. This is intentionally still a projection-only lane; norms,
shortconv, attention, state and nonlinearities remain on the CPU until packed
projection parity and bandwidth are measured.

## Gates and TRUEOS port

```sh
make intel-gpu-bake-lfm25-q8-packed-cpp
make intel-gpu-verify-cpp-artifacts
make lfm25-packed-isa-verify
make lfm25-cpp-verify
make lfm25-igpu-packed-verify
```

Required evidence:

- reproducible SPIR-V and ADL-S Zebin;
- SIMD16, 128 GRF, zero scratch and zero SLM;
- exactly eight static SIMD16 DP4A instructions and at most one scale gather;
- all 93 Q8 tensors admitted with the fixed image hash;
- layer-0 gate/up/down `max_abs=0`;
- the CPU packed contract executor reproduces all ten sealed `hi` decisions
  across 930 projections;
- sealed `hi` token trace with 930 GPU projections;
- `hi ai` byte-identical to the pinned b10075 oracle;
- NEO, IGC and successful i915 `EXECBUFFER2` evidence;
- event-profiled model-weight GB/s and scalar-versus-packed speedup.

The port keeps the native and packed artifacts as separate admitted variants
behind one layout-tagged RCS runtime. This prevents a resident model mapping
from being dispatched with the wrong instruction image. The packed artifact
uses GPU VA `0x0D820000`; its page-rounded mapping is compile-time checked
against the legacy artifact and the fixed model VA window.

At backend open, TRUEOS:

1. loads and verifies the sealed native image;
2. repacks all Q8 tensors in place using one 78,336-byte scratch tile;
3. verifies the deterministic packed-image SHA-256;
4. binds the packed model and artifact as one layout-tagged mapping;
5. repacks each 1,024- or 4,608-wide activation into the split ABI before
   submission;
6. reads the tied token embedding directly from the packed tensor layout.

Tensor offsets, projection batching, the 99-operation CPU control plane, and
Shell2 `lum` stay unchanged. The legacy artifact and native binder remain
available as a rollback/debug lane. Linked and packaged product gates require
the packed binary, and the normal compiler-free artifact gate reruns the
eight-DP4A ISA proof.
