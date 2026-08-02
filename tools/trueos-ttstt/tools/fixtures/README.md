# Kokoro ConvInteger oracle fixture

`kokoro_convinteger_oracle.json` is a compact, byte-exact oracle for the
Kokoro generator's profiled ConvInteger hotspot. It contains real quantized
activation patches and weights from this inference input:

- IPA: `həlˈoʊ fɹʌm ɹʌst`
- tokens including BOS/EOS: `[0, 50, 83, 54, 156, 57, 135, 16, 48, 123,
  138, 55, 16, 123, 138, 61, 62, 0]`
- voice/style: `af_heart`, style index 16
- speed: 1.0

The complete activation and accumulator are represented only by their shape,
range and SHA-256. Their multi-megabyte payloads are deliberately omitted.
The `compact_fixture` object contains all bytes required for a standalone
kernel test.

## Operator contract

The selected node is
`/decoder/decoder/generator/noise_res.1/convs1.2/Conv_quant`, graph index 2570.

| Tensor | Type | Layout | Shape |
| --- | --- | --- | --- |
| activation `X` | `u8` | NCW | `[1, 128, 8281]` |
| weight `W` | `u8` | MCK | `[128, 128, 11]` |
| accumulator `Y` | `i32` | NMW | `[1, 128, 8281]` |

The attributes are stride 1, dilation 5, left/right padding 25, group 1,
kernel width 11 and `auto_pad=NOTSET`. Both zero points are rank-zero scalar
`u8` tensors: `X_zero_point=96` and `W_zero_point=61`. The weight zero point is
not per-channel.

For output position `p`, input channel `c`, output channel `m`, and kernel tap
`k`, the input coordinate is:

```text
x_position = p * stride - pad_left + k * dilation
Y[0,m,p] = sum(c=0..127, k=0..10)
               (X[0,c,x_position] - 96) * (W[m,c,k] - 61)
```

An out-of-bounds input coordinate is the quantized real zero, `96`. Its
centered contribution is therefore zero. ConvInteger has no bias; bias and
scaling occur in later graph nodes.

## Compact byte layout

All base64 fields contain uncompressed, contiguous row-major bytes. Each field
also supplies its decoded byte length and SHA-256.

`compact_fixture.patches` has shape `[3, 128, 11]` and layout
`position,input_channel,kernel_index`. For indices `q`, `c`, and `k`, its byte
offset is:

```text
((q * 128 + c) * 11 + k)
```

The three `q` entries correspond, in order, to output positions `[0, 4140,
8280]`. Position 0 is the left-padding case, position 4140 is fully interior,
and position 8280 is the right-padding case. Their valid kernel taps are
respectively `[5..10]`, `[0..10]`, and `[0..5]`; all other stored patch bytes
are 96.

`compact_fixture.weights` has shape `[2, 128, 11]` and layout
`selected_output_channel,input_channel,kernel_index`. Its byte offset is:

```text
((r * 128 + c) * 11 + k)
```

The two `r` entries select original output channels `[0, 127]`.

`compact_fixture.expected_i32` has shape `[3, 2]` and layout
`position,selected_output_channel`. It is encoded as signed little-endian
32-bit integers at byte offset `(q * 2 + r) * 4`. Its values are:

```text
[[-10249,  4913],
 [-37067,  9573],
 [ -4291,  3863]]
```

These accumulator results require absolute and relative tolerance zero. The
kernel result must be bit-exact.

## Verification and regeneration

Self-verification uses only the Python standard library. It validates every
encoded hash and recomputes the six integer dot products:

```sh
cd tools/trueos-ttstt
python3 tools/generate_kokoro_convinteger_oracle.py \
  --verify-fixture tools/fixtures/kokoro_convinteger_oracle.json
```

The committed fixture SHA-256 is
`35ea56b83c6af31d0b0b1c9d955f8d780c63895d65a2d87bcf02ef9dfc2f9e61`.
Its component hashes are:

- patches: `6b40a229128570604bfeb07a3deeb1b72ebf5cacb6f649db58acf85c5ecd57f1`
- selected weights: `59b08989dc4914876ec128977306c8fe8952e81902109e94b2953b5245d02543`
- expected `i32`: `9b488798fe4a984f386297d3d556a9aee675e5c23c5f946e4a1ab0f5a88ddb02`

Byte-identical regeneration additionally requires NumPy 2.5.1, ONNX 1.22.0,
ONNX Runtime 1.28.0, and the source model and voice archive named in the JSON:

```sh
/tmp/ttstt-conv-oracle-venv/bin/python \
  tools/generate_kokoro_convinteger_oracle.py \
  --check tools/fixtures/kokoro_convinteger_oracle.json
```
