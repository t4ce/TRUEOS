#!/usr/bin/env python3
"""Compile or audit the pinned prepared Kokoro ONNX graph.

This is deliberately a narrow compiler, not a general ONNX importer.  The
``analyze`` command admits only the prepared TRUEOS Kokoro graph and
fails closed when its provenance, operator inventory, tensor contract,
quantized lowering patterns, or frame-count phase boundary changes.

The binary writer at the bottom of this file implements the matching
``trueos-kokoro-aot`` v1 wire contract.  Importing the module does not require
ONNX, which keeps the binary-format unit tests usable in the normal kernel
tooling environment.  Graph commands require the explicitly installed
offline tooling dependency ``onnx``.
"""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from dataclasses import dataclass, field
import hashlib
import json
from pathlib import Path
import struct
import sys
from typing import Any, Iterable, Mapping, Sequence


TOOL_VERSION = 1
ANALYSIS_SCHEMA = "trueos.kokoro-aot-analysis.v1"

# Operation attributes have their own ABI because the outer artifact format is
# intentionally opaque to individual kernels. Every non-empty record begins
# with ``<u16 version, u16 kind/opcode, u32 total_bytes>``. Version one records
# are four-byte aligned, fixed-size per kind, and require every reserved byte
# to remain zero.
ATTRIBUTE_ABI_VERSION = 1
ATTRIBUTE_LAYOUT_CHECKED = 1
ATTRIBUTE_BINARY_MULTIDIRECTIONAL = 1 << 0
ATTRIBUTE_BINARY_RUNTIME_SHAPE_CHECK = 1 << 1
ATTRIBUTE_VIEW_ALIAS = 1 << 0
ATTRIBUTE_VIEW_STATIC_CONTROL = 1 << 1
ATTRIBUTE_CONTROL_ABSENT = 0
ATTRIBUTE_CONTROL_INITIALIZER = 1
ATTRIBUTE_CONTROL_DYNAMIC = 2
ATTRIBUTE_PAD_REFLECT = 1
ATTRIBUTE_SCATTER_REDUCTION_NONE = 0
ATTRIBUTE_SCATTER_ORDERED_UNIQUE = 1

PINNED_MODEL_FILE = "kokoro-rten.onnx"
PINNED_MODEL_BYTES = 124_604_222
PINNED_MODEL_SHA256 = "239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29"
PINNED_VOICES_BYTES = 28_214_398
PINNED_VOICES_SHA256 = "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d"

PINNED_IR_VERSION = 9
PINNED_PRODUCER = ("onnx.quantize", "0.1.0")
PINNED_GRAPH_NAME = "main_graph"
PINNED_METADATA = {"trueos.ttstt.rten_bridge": "1"}
PINNED_OPSETS = (
    ("", 20),
    ("com.microsoft.experimental", 1),
    ("ai.onnx.ml", 5),
    ("ai.onnx.training", 1),
    ("com.microsoft", 1),
    ("ai.onnx.preview.training", 1),
    ("com.microsoft.nchwc", 1),
    ("org.pytorch.aten", 1),
)

PINNED_NODE_COUNTS = {
    "Add": 546,
    "And": 1,
    "Atan": 1,
    "Cast": 333,
    "Clip": 1,
    "Concat": 72,
    "ConstantOfShape": 6,
    "Conv": 1,
    "ConvInteger": 87,
    "ConvTranspose": 6,
    "Cos": 1,
    "CumSum": 2,
    "DequantizeLinear": 4,
    "Div": 174,
    "DynamicQuantizeLinear": 139,
    "Equal": 4,
    "Exp": 1,
    "Expand": 5,
    "Floor": 81,
    "Gather": 135,
    "Greater": 3,
    "GreaterOrEqual": 1,
    "LSTM": 6,
    "LayerNormalization": 19,
    "LeakyRelu": 28,
    "Less": 2,
    "MatMul": 27,
    "MatMulInteger": 148,
    "Mul": 737,
    "NonZero": 1,
    "Pad": 2,
    "Pow": 50,
    "Range": 2,
    "ReduceMean": 130,
    "ReduceSum": 1,
    "Reshape": 141,
    "Resize": 6,
    "Round": 1,
    "STFT": 1,
    "ScatterND": 1,
    "Shape": 73,
    "Sigmoid": 1,
    "Sin": 51,
    "SkipLayerNormalization": 12,
    "Slice": 22,
    "Softmax": 12,
    "Split": 74,
    "Sqrt": 90,
    "Squeeze": 5,
    "Sub": 68,
    "Tanh": 1,
    "Transpose": 88,
    "Unsqueeze": 192,
    "Where": 7,
    "FastGelu": 12,
}

# The two Microsoft operators left after the RTen bridge are intentionally
# represented by their domain-qualified keys.  The unqualified inventory
# above is retained because it is easier to compare with common ONNX tooling.
PINNED_DOMAIN_COUNTS = {"ai.onnx": 3_591, "com.microsoft": 24}

FRAME_COUNT_NODE_INDEX = 1_746
FRAME_COUNT_NODE_NAME = "/encoder/Gather_1"
FRAME_COUNT_TENSOR = "/encoder/Cast_1_output_0"
PHASE_ONE_RAW_START = 1_747
FRAME_RANGE_NODE_INDEX = 1_749
FRAME_RANGE_NODE_NAME = "/encoder/Range"

DTYPE_NAMES = {
    1: "FLOAT",
    2: "UINT8",
    3: "INT8",
    6: "INT32",
    7: "INT64",
    9: "BOOL",
}
SUPPORTED_DTYPES = frozenset(DTYPE_NAMES)

VIEW_OPS = frozenset({"Reshape", "Squeeze", "Unsqueeze"})


class CompileError(ValueError):
    """A fail-closed model or artifact-contract rejection."""


def reject(condition: bool, message: str) -> None:
    if condition:
        raise CompileError(message)


def sha256_file(path: Path) -> tuple[int, str]:
    digest = hashlib.sha256()
    size = 0
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            size += len(chunk)
            digest.update(chunk)
    return size, digest.hexdigest()


def require_onnx() -> Any:
    try:
        import onnx  # type: ignore
    except ImportError as error:
        raise CompileError(
            "graph commands require ONNX; install the offline dependency with "
            "`python3 -m pip install 'onnx>=1.16,<2'`"
        ) from error
    return onnx


def canonical_json(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


# Sealed-program v1.  Keep these literal constants adjacent to the writer so
# format drift is caught by the cross-language fixture test.
AOT_MAGIC = b"KKAOTV1\0"
AOT_VERSION = 1
AOT_ENDIAN_TAG = 0x4C45
AOT_HEADER_BYTES = 352
AOT_SECTION_COUNT = 6
AOT_PHASE_COUNT = 2
AOT_ARENA_ALIGNMENT = 64
AOT_NO_ID = 0xFFFF_FFFF
AOT_STATIC_DIM = 0xFF

AOT_SECTION_SPECS = (
    (1, 16, 128, "tensors"),
    (2, 16, 64, "slots"),
    (3, 8, 40, "ops"),
    (4, 8, 4, "bindings"),
    (5, 8, 48, "phases"),
    (6, 16, 1, "data"),
)

AOT_OPCODES = {
    "ResolveDecoderShape": 0x0001,
    "Add": 0x0100,
    "And": 0x0101,
    "Atan": 0x0102,
    "Cast": 0x0103,
    "Clip": 0x0104,
    "Concat": 0x0105,
    "ConstantOfShape": 0x0106,
    "Conv": 0x0107,
    "ConvInteger": 0x0108,
    "ConvTranspose": 0x0109,
    "Cos": 0x010A,
    "CumSum": 0x010B,
    "DequantizeLinear": 0x010C,
    "Div": 0x010D,
    "DynamicQuantizeLinear": 0x010E,
    "Equal": 0x010F,
    "Exp": 0x0110,
    "Expand": 0x0111,
    "Floor": 0x0112,
    "Gather": 0x0113,
    "Greater": 0x0114,
    "GreaterOrEqual": 0x0115,
    "LSTM": 0x0116,
    "LayerNormalization": 0x0117,
    "LeakyRelu": 0x0118,
    "Less": 0x0119,
    "MatMul": 0x011A,
    "MatMulInteger": 0x011B,
    "Mul": 0x011C,
    "NonZero": 0x011D,
    "Pad": 0x011E,
    "Pow": 0x011F,
    "Range": 0x0120,
    "ReduceMean": 0x0121,
    "ReduceSum": 0x0122,
    "Reshape": 0x0123,
    "Resize": 0x0124,
    "Round": 0x0125,
    "STFT": 0x0126,
    "ScatterND": 0x0127,
    "Shape": 0x0128,
    "Sigmoid": 0x0129,
    "Sin": 0x012A,
    "Slice": 0x012B,
    "Softmax": 0x012C,
    "Split": 0x012D,
    "Sqrt": 0x012E,
    "Squeeze": 0x012F,
    "Sub": 0x0130,
    "Tanh": 0x0131,
    "Transpose": 0x0132,
    "Unsqueeze": 0x0133,
    "Where": 0x0134,
    "FastGelu": 0x0200,
    "SkipLayerNormalization": 0x0201,
    "DynamicQuantizedGemm": 0x0300,
    "DynamicQuantizedConv1d": 0x0301,
    "AddSoftmax": 0x0302,
    "BiLstm256": 0x0303,
    "AlbertAttention": 0x0304,
    "FloatConv1d": 0x0305,
    "FloatConvTranspose1d": 0x0306,
    "FixedStft20": 0x0307,
    "ElementwiseFusion": 0x0308,
}

AOT_DTYPE_BYTES = {1: 4, 2: 4, 3: 8, 4: 1, 5: 1, 6: 1}

LOWERED_F32_BINARY_OPS = frozenset({"Add", "Mul", "Div", "Sub"})
LOWERED_PARAMETERLESS_UNARY_OPS = frozenset(
    {"Atan", "Cos", "Exp", "Floor", "Round", "Sigmoid", "Sin", "Sqrt", "Tanh"}
)
LOWERED_UNARY_OPS = LOWERED_PARAMETERLESS_UNARY_OPS | {"LeakyRelu"}
LOWERED_F32_OPS = frozenset(
    {
        *LOWERED_F32_BINARY_OPS,
        *LOWERED_UNARY_OPS,
        "ReduceMean",
        "LayerNormalization",
        "Softmax",
        "FastGelu",
        "SkipLayerNormalization",
    }
)
LOWERED_VIEW_OPS = frozenset({"Reshape", "Squeeze", "Unsqueeze"})
LOWERED_MATERIAL_LAYOUT_OPS = frozenset(
    {
        "Transpose",
        "Gather",
        "Concat",
        "Split",
        "Expand",
        "Shape",
        "Slice",
        "Pad",
        "NonZero",
        "ScatterND",
    }
)
LOWERED_LAYOUT_OPS = LOWERED_VIEW_OPS | LOWERED_MATERIAL_LAYOUT_OPS
LOWERED_OPS = LOWERED_F32_OPS | LOWERED_LAYOUT_OPS

PINNED_LOWERING_COUNTS = {
    "Add": 465,
    "Atan": 1,
    "Cos": 1,
    "Div": 174,
    "Exp": 1,
    "FastGelu": 12,
    "Floor": 81,
    "Gather": 135,
    "LayerNormalization": 19,
    "LeakyRelu": 28,
    "Mul": 737,
    "NonZero": 1,
    "Pad": 2,
    "ReduceMean": 130,
    "Reshape": 141,
    "Round": 1,
    "ScatterND": 1,
    "Sigmoid": 1,
    "SkipLayerNormalization": 12,
    "Sin": 51,
    "Shape": 73,
    "Slice": 22,
    "Softmax": 12,
    "Split": 74,
    "Sqrt": 90,
    "Squeeze": 5,
    "Sub": 68,
    "Tanh": 1,
    "Transpose": 88,
    "Unsqueeze": 192,
    "Concat": 72,
    "Expand": 5,
}

# Filled from the canonical lowering stream after every record has passed the
# operation-specific validator. This is deliberately separate from the model
# hash: it seals the compiler's interpretation of the accepted graph.
PINNED_LOWERING_SHA256 = "fd91172b0b96ae5f22a353664d381b58d4e708a6effaca2b183332e63d0b9920"

LAYER_NORM_EPSILON_BITS = frozenset({0x2B8CBCCC, 0x3727C5AC})
SKIP_LAYER_NORM_EPSILON_BITS = 0x2B8CBCCC
LEAKY_RELU_ALPHA_BITS = frozenset({0x3C23D70A, 0x3DCCCCCD, 0x3E4CCCCD})


def align_up(value: int, alignment: int) -> int:
    reject(alignment <= 0 or alignment & (alignment - 1) != 0, "invalid alignment")
    return (value + alignment - 1) & -alignment


def contiguous_strides(dtype: int, dims: Sequence[int]) -> tuple[int, int, int, int]:
    reject(dtype not in AOT_DTYPE_BYTES, f"unknown AOT dtype {dtype}")
    reject(len(dims) > 4, "AOT tensor rank exceeds four")
    strides = [0, 0, 0, 0]
    stride = AOT_DTYPE_BYTES[dtype]
    for index in range(len(dims) - 1, -1, -1):
        strides[index] = stride
        stride *= dims[index]
    return tuple(strides)  # type: ignore[return-value]


def logical_bytes(dtype: int, dims: Sequence[int]) -> int:
    size = AOT_DTYPE_BYTES[dtype]
    for dim in dims:
        reject(dim < 0 or dim > 0xFFFF_FFFF, f"invalid tensor dimension {dim}")
        size *= dim
        reject(size > 0xFFFF_FFFF_FFFF_FFFF, "tensor byte capacity overflows u64")
    return size


def f32_bits(value: float) -> int:
    """Return the exact IEEE-754 binary32 payload used by ONNX attributes."""

    return struct.unpack("<I", struct.pack("<f", float(value)))[0]


def _attribute_record(opcode: int, body: bytes) -> bytes:
    total_bytes = 8 + len(body)
    reject(opcode not in AOT_OPCODES.values(), f"attribute kind 0x{opcode:04x} rejected")
    reject(total_bytes % 4 != 0, "attribute record is not four-byte aligned")
    return struct.pack(
        "<HHI", ATTRIBUTE_ABI_VERSION, opcode, total_bytes
    ) + body


def binary_attribute(
    op_type: str, lhs_rank: int, rhs_rank: int, output_rank: int
) -> bytes:
    reject(op_type not in LOWERED_F32_BINARY_OPS, f"binary attribute kind {op_type!r}")
    return _attribute_record(
        AOT_OPCODES[op_type],
        struct.pack(
            "<BBBBBB2x",
            lhs_rank,
            rhs_rank,
            output_rank,
            ATTRIBUTE_LAYOUT_CHECKED,
            ATTRIBUTE_LAYOUT_CHECKED,
            ATTRIBUTE_BINARY_MULTIDIRECTIONAL
            | ATTRIBUTE_BINARY_RUNTIME_SHAPE_CHECK,
        ),
    )


def parameterless_unary_attribute(op_type: str) -> bytes:
    reject(
        op_type not in LOWERED_PARAMETERLESS_UNARY_OPS,
        f"parameterless unary attribute kind {op_type!r}",
    )
    return _attribute_record(AOT_OPCODES[op_type], b"")


def leaky_relu_attribute(alpha_bits: int, input_rank: int, output_rank: int) -> bytes:
    return _attribute_record(
        AOT_OPCODES["LeakyRelu"],
        struct.pack(
            "<IBBBx",
            alpha_bits,
            input_rank,
            output_rank,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def reduce_mean_attribute(
    axis: int, keepdims: int, noop_with_empty_axes: int, input_rank: int, output_rank: int
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["ReduceMean"],
        struct.pack(
            "<iBBBBB3x",
            axis,
            keepdims,
            noop_with_empty_axes,
            input_rank,
            output_rank,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def layer_norm_attribute(
    axis: int,
    epsilon_bits: int,
    stash_type: int,
    input_rank: int,
    output_rank: int,
    parameter_rank: int,
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["LayerNormalization"],
        struct.pack(
            "<iIIBBBB",
            axis,
            epsilon_bits,
            stash_type,
            input_rank,
            output_rank,
            parameter_rank,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def softmax_attribute(axis: int, input_rank: int, output_rank: int) -> bytes:
    return _attribute_record(
        AOT_OPCODES["Softmax"],
        struct.pack(
            "<iBBBx",
            axis,
            input_rank,
            output_rank,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def fast_gelu_attribute(
    has_bias: int, input_rank: int, output_rank: int, bias_rank: int
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["FastGelu"],
        struct.pack(
            "<BBBBB3x",
            has_bias,
            input_rank,
            output_rank,
            bias_rank,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def skip_layer_norm_attribute(
    epsilon_bits: int, input_rank: int, output_rank: int, parameter_rank: int
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["SkipLayerNormalization"],
        struct.pack(
            "<IBBBB",
            epsilon_bits,
            input_rank,
            output_rank,
            parameter_rank,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def view_attribute(
    op_type: str,
    input_rank: int,
    output_rank: int,
    dtype: int,
    *,
    static_control: bool,
    allowzero: int = 0,
    parameters: Sequence[int] = (),
) -> bytes:
    reject(op_type not in LOWERED_VIEW_OPS, f"view attribute kind {op_type!r}")
    reject(len(parameters) > 4, f"{op_type} has too many view parameters")
    padded = tuple(int(value) for value in parameters) + (0,) * (4 - len(parameters))
    flags = ATTRIBUTE_VIEW_ALIAS
    if static_control:
        flags |= ATTRIBUTE_VIEW_STATIC_CONTROL
    return _attribute_record(
        AOT_OPCODES[op_type],
        struct.pack(
            "<BBBBBBBB4i",
            input_rank,
            output_rank,
            dtype,
            flags,
            allowzero,
            len(parameters),
            ATTRIBUTE_LAYOUT_CHECKED,
            0,
            *padded,
        ),
    )


def transpose_attribute(permutation: Sequence[int], rank: int, dtype: int) -> bytes:
    reject(len(permutation) > 4, "Transpose permutation exceeds rank four")
    padded = tuple(int(axis) for axis in permutation) + (0,) * (4 - len(permutation))
    return _attribute_record(
        AOT_OPCODES["Transpose"],
        struct.pack(
            "<BBBBB3x4i",
            rank,
            rank,
            dtype,
            ATTRIBUTE_LAYOUT_CHECKED,
            len(permutation),
            *padded,
        ),
    )


def gather_attribute(
    axis: int,
    data_rank: int,
    indices_rank: int,
    output_rank: int,
    dtype: int,
    control_mode: int,
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["Gather"],
        struct.pack(
            "<iBBBBBB2x",
            axis,
            data_rank,
            indices_rank,
            output_rank,
            dtype,
            control_mode,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def concat_attribute(
    axis: int, rank: int, output_rank: int, dtype: int, input_count: int
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["Concat"],
        struct.pack(
            "<iBBBBB3x",
            axis,
            rank,
            output_rank,
            dtype,
            input_count,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def split_attribute(
    axis: int,
    first_axis_len: int,
    second_axis_len: int,
    input_rank: int,
    output_rank: int,
    dtype: int,
    output_count: int,
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["Split"],
        struct.pack(
            "<iIIBBBBB3x",
            axis,
            first_axis_len,
            second_axis_len,
            input_rank,
            output_rank,
            dtype,
            output_count,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def expand_attribute(
    input_rank: int,
    output_rank: int,
    dtype: int,
    control_mode: int,
    control_rank: int,
    producer_opcode: int,
    target_dims: Sequence[int] = (),
) -> bytes:
    reject(len(target_dims) > 4, "Expand target rank exceeds four")
    padded = tuple(int(dim) for dim in target_dims) + (0,) * (4 - len(target_dims))
    return _attribute_record(
        AOT_OPCODES["Expand"],
        struct.pack(
            "<BBBBBBH4i",
            input_rank,
            output_rank,
            dtype,
            control_mode,
            control_rank,
            ATTRIBUTE_LAYOUT_CHECKED,
            producer_opcode,
            *padded,
        ),
    )


def shape_attribute(
    start: int,
    end: int,
    has_end: int,
    input_rank: int,
    output_rank: int,
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["Shape"],
        struct.pack(
            "<iiBBBB4x",
            start,
            end,
            input_rank,
            output_rank,
            has_end,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def slice_attribute(
    input_rank: int,
    output_rank: int,
    dtype: int,
    control_count: int,
    axes_present: bool,
    steps_present: bool,
    control_modes: Sequence[int],
    control_values: Sequence[int],
    producer_opcodes: Sequence[int],
) -> bytes:
    reject(
        len(control_modes) != 4
        or len(control_values) != 4
        or len(producer_opcodes) != 4,
        "Slice requires four canonical control slots",
    )
    flags = (1 if axes_present else 0) | (2 if steps_present else 0)
    return _attribute_record(
        AOT_OPCODES["Slice"],
        struct.pack(
            "<BBBBBBH4B4x4q4H",
            input_rank,
            output_rank,
            dtype,
            control_count,
            flags,
            ATTRIBUTE_LAYOUT_CHECKED,
            0,
            *control_modes,
            *control_values,
            *producer_opcodes,
        ),
    )


def pad_attribute(
    input_rank: int, output_rank: int, dtype: int, pads: Sequence[int]
) -> bytes:
    reject(len(pads) > 8, "Pad vector exceeds rank-four ABI")
    padded = tuple(int(value) for value in pads) + (0,) * (8 - len(pads))
    return _attribute_record(
        AOT_OPCODES["Pad"],
        struct.pack(
            "<BBBBBBH8I",
            input_rank,
            output_rank,
            dtype,
            ATTRIBUTE_PAD_REFLECT,
            len(pads),
            ATTRIBUTE_LAYOUT_CHECKED,
            0,
            *padded,
        ),
    )


def nonzero_attribute(input_rank: int, output_rank: int) -> bytes:
    return _attribute_record(
        AOT_OPCODES["NonZero"],
        struct.pack(
            "<BBBBBB2x",
            input_rank,
            output_rank,
            9,
            7,
            ATTRIBUTE_LAYOUT_CHECKED,
            1,
        ),
    )


def scatter_nd_attribute(
    data_rank: int,
    indices_rank: int,
    updates_rank: int,
    output_rank: int,
    dtype: int,
    tuple_len: int,
) -> bytes:
    return _attribute_record(
        AOT_OPCODES["ScatterND"],
        struct.pack(
            "<BBBBBBBBB3x",
            data_rank,
            indices_rank,
            updates_rank,
            output_rank,
            dtype,
            tuple_len,
            ATTRIBUTE_SCATTER_REDUCTION_NONE,
            ATTRIBUTE_SCATTER_ORDERED_UNIQUE,
            ATTRIBUTE_LAYOUT_CHECKED,
        ),
    )


def attribute_alignment(record: bytes) -> int:
    reject(len(record) < 8, "attribute length rejected")
    kind = struct.unpack_from("<H", record, 2)[0]
    return 8 if kind == AOT_OPCODES["Slice"] else 4


ATTRIBUTE_RECORD_BYTES = {
    AOT_OPCODES["Add"]: 16,
    AOT_OPCODES["Mul"]: 16,
    AOT_OPCODES["Div"]: 16,
    AOT_OPCODES["Sub"]: 16,
    **{AOT_OPCODES[name]: 8 for name in LOWERED_PARAMETERLESS_UNARY_OPS},
    AOT_OPCODES["LeakyRelu"]: 16,
    AOT_OPCODES["ReduceMean"]: 20,
    AOT_OPCODES["LayerNormalization"]: 24,
    AOT_OPCODES["Softmax"]: 16,
    AOT_OPCODES["FastGelu"]: 16,
    AOT_OPCODES["SkipLayerNormalization"]: 16,
    AOT_OPCODES["Reshape"]: 32,
    AOT_OPCODES["Squeeze"]: 32,
    AOT_OPCODES["Unsqueeze"]: 32,
    AOT_OPCODES["Transpose"]: 32,
    AOT_OPCODES["Gather"]: 20,
    AOT_OPCODES["Concat"]: 20,
    AOT_OPCODES["Split"]: 28,
    AOT_OPCODES["Expand"]: 32,
    AOT_OPCODES["Shape"]: 24,
    AOT_OPCODES["Slice"]: 64,
    AOT_OPCODES["Pad"]: 48,
    AOT_OPCODES["NonZero"]: 16,
    AOT_OPCODES["ScatterND"]: 20,
}


def inspect_attribute_record(
    record: bytes, expected_opcode: int | None = None
) -> dict[str, int | tuple[int, ...]]:
    """Validate one canonical v1 attribute record and return its fields."""

    reject(len(record) < 8 or len(record) % 4 != 0, "attribute length rejected")
    version, kind, total_bytes = struct.unpack_from("<HHI", record)
    reject(version != ATTRIBUTE_ABI_VERSION, "attribute ABI version rejected")
    reject(expected_opcode is not None and kind != expected_opcode, "attribute kind rejected")
    reject(total_bytes != len(record), "attribute byte count rejected")
    reject(kind not in ATTRIBUTE_RECORD_BYTES, f"attribute kind 0x{kind:04x} unsupported")
    reject(len(record) != ATTRIBUTE_RECORD_BYTES[kind], "attribute fixed size rejected")

    result: dict[str, int | tuple[int, ...]] = {
        "version": version,
        "kind": kind,
        "bytes": total_bytes,
    }
    if kind in {AOT_OPCODES[name] for name in LOWERED_F32_BINARY_OPS}:
        lhs_rank, rhs_rank, output_rank, input_layout, output_layout, flags = (
            struct.unpack_from("<BBBBBB", record, 8)
        )
        reject(any(record[14:16]), "binary attribute reserved bytes rejected")
        reject(max(lhs_rank, rhs_rank, output_rank) > 4, "binary attribute rank rejected")
        reject(output_rank != max(lhs_rank, rhs_rank), "binary output rank rejected")
        reject(
            input_layout != ATTRIBUTE_LAYOUT_CHECKED
            or output_layout != ATTRIBUTE_LAYOUT_CHECKED,
            "binary layout contract rejected",
        )
        reject(
            flags
            != ATTRIBUTE_BINARY_MULTIDIRECTIONAL
            | ATTRIBUTE_BINARY_RUNTIME_SHAPE_CHECK,
            "binary flags rejected",
        )
        result.update(
            lhs_rank=lhs_rank,
            rhs_rank=rhs_rank,
            output_rank=output_rank,
            flags=flags,
        )
    elif kind in {
        AOT_OPCODES[name] for name in LOWERED_PARAMETERLESS_UNARY_OPS
    }:
        # The opcode in the common header is the complete parameterless
        # contract. Rank/layout facts are validated by the lowering record.
        pass
    elif kind == AOT_OPCODES["LeakyRelu"]:
        alpha_bits, input_rank, output_rank, layout = struct.unpack_from(
            "<IBBB", record, 8
        )
        reject(record[15] != 0, "LeakyRelu reserved byte rejected")
        alpha = struct.unpack("<f", struct.pack("<I", alpha_bits))[0]
        reject(
            not (alpha > 0.0 and alpha < float("inf"))
            or input_rank > 4
            or output_rank != input_rank
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "LeakyRelu contract rejected",
        )
        result.update(
            alpha_bits=alpha_bits,
            input_rank=input_rank,
            output_rank=output_rank,
        )
    elif kind == AOT_OPCODES["ReduceMean"]:
        axis, keepdims, noop, input_rank, output_rank, layout = struct.unpack_from(
            "<iBBBBB", record, 8
        )
        reject(any(record[17:20]), "ReduceMean reserved bytes rejected")
        reject(
            input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or axis < -input_rank
            or axis >= input_rank,
            "ReduceMean rank/axis rejected",
        )
        reject(
            (keepdims, noop, layout) != (1, 0, ATTRIBUTE_LAYOUT_CHECKED),
            "ReduceMean flags rejected",
        )
        result.update(
            axis=axis,
            keepdims=keepdims,
            noop_with_empty_axes=noop,
            input_rank=input_rank,
            output_rank=output_rank,
        )
    elif kind == AOT_OPCODES["LayerNormalization"]:
        axis, epsilon_bits, stash_type, input_rank, output_rank, parameter_rank, layout = (
            struct.unpack_from("<iIIBBBB", record, 8)
        )
        reject(
            input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or axis < -input_rank
            or axis >= input_rank,
            "LayerNormalization rank/axis rejected",
        )
        reject(
            stash_type != 1
            or parameter_rank != 1
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "LayerNormalization contract rejected",
        )
        epsilon = struct.unpack("<f", struct.pack("<I", epsilon_bits))[0]
        reject(not (epsilon > 0.0 and epsilon < float("inf")), "epsilon rejected")
        result.update(
            axis=axis,
            epsilon_bits=epsilon_bits,
            stash_type=stash_type,
            input_rank=input_rank,
            output_rank=output_rank,
            parameter_rank=parameter_rank,
        )
    elif kind == AOT_OPCODES["Softmax"]:
        axis, input_rank, output_rank, layout = struct.unpack_from("<iBBB", record, 8)
        reject(record[15] != 0, "Softmax reserved byte rejected")
        reject(
            input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or axis < -input_rank
            or axis >= input_rank
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "Softmax contract rejected",
        )
        result.update(axis=axis, input_rank=input_rank, output_rank=output_rank)
    elif kind == AOT_OPCODES["FastGelu"]:
        has_bias, input_rank, output_rank, bias_rank, layout = struct.unpack_from(
            "<BBBBB", record, 8
        )
        reject(any(record[13:16]), "FastGelu reserved bytes rejected")
        reject(
            has_bias != 1
            or input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or bias_rank != 1
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "FastGelu contract rejected",
        )
        result.update(
            has_bias=has_bias,
            input_rank=input_rank,
            output_rank=output_rank,
            parameter_rank=bias_rank,
        )
    elif kind == AOT_OPCODES["SkipLayerNormalization"]:
        epsilon_bits, input_rank, output_rank, parameter_rank, layout = struct.unpack_from(
            "<IBBBB", record, 8
        )
        epsilon = struct.unpack("<f", struct.pack("<I", epsilon_bits))[0]
        reject(
            not (epsilon > 0.0 and epsilon < float("inf"))
            or input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or parameter_rank != 1
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "SkipLayerNormalization contract rejected",
        )
        result.update(
            epsilon_bits=epsilon_bits,
            input_rank=input_rank,
            output_rank=output_rank,
            parameter_rank=parameter_rank,
        )
    elif kind == AOT_OPCODES["Transpose"]:
        input_rank, output_rank, dtype, layout, count = struct.unpack_from(
            "<BBBBB", record, 8
        )
        permutation = struct.unpack_from("<4i", record, 16)
        reject(any(record[13:16]), "Transpose reserved bytes rejected")
        reject(
            input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or dtype not in {1, 7}
            or layout != ATTRIBUTE_LAYOUT_CHECKED
            or count != input_rank
            or any(permutation[count:])
            or sorted(permutation[:count]) != list(range(input_rank)),
            "Transpose contract rejected",
        )
        result.update(
            input_rank=input_rank,
            output_rank=output_rank,
            dtype=dtype,
            parameters=permutation[:count],
        )
    elif kind == AOT_OPCODES["Gather"]:
        axis, data_rank, indices_rank, output_rank, dtype, control_mode, layout = (
            struct.unpack_from("<iBBBBBB", record, 8)
        )
        reject(any(record[18:20]), "Gather reserved bytes rejected")
        reject(
            data_rank == 0
            or data_rank > 4
            or indices_rank > 4
            or output_rank != data_rank - 1 + indices_rank
            or output_rank > 4
            or axis < -data_rank
            or axis >= data_rank
            or dtype not in {1, 3, 7}
            or control_mode
            not in {ATTRIBUTE_CONTROL_INITIALIZER, ATTRIBUTE_CONTROL_DYNAMIC}
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "Gather contract rejected",
        )
        result.update(
            axis=axis,
            input_rank=data_rank,
            indices_rank=indices_rank,
            output_rank=output_rank,
            dtype=dtype,
            control_mode=control_mode,
        )
    elif kind == AOT_OPCODES["Concat"]:
        axis, input_rank, output_rank, dtype, input_count, layout = struct.unpack_from(
            "<iBBBBB", record, 8
        )
        reject(any(record[17:20]), "Concat reserved bytes rejected")
        reject(
            input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or axis < -input_rank
            or axis >= input_rank
            or dtype not in {1, 7}
            or input_count < 2
            or input_count > 4
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "Concat contract rejected",
        )
        result.update(
            axis=axis,
            input_rank=input_rank,
            output_rank=output_rank,
            dtype=dtype,
            input_count=input_count,
        )
    elif kind == AOT_OPCODES["Split"]:
        (
            axis,
            first_axis_len,
            second_axis_len,
            input_rank,
            output_rank,
            dtype,
            output_count,
            layout,
        ) = struct.unpack_from("<iIIBBBBB", record, 8)
        reject(any(record[25:28]), "Split reserved bytes rejected")
        reject(
            input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or axis < -input_rank
            or axis >= input_rank
            or first_axis_len == 0
            or second_axis_len == 0
            or dtype != 1
            or output_count != 2
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "Split contract rejected",
        )
        result.update(
            axis=axis,
            input_rank=input_rank,
            output_rank=output_rank,
            dtype=dtype,
            output_count=output_count,
            parameters=(first_axis_len, second_axis_len),
        )
    elif kind == AOT_OPCODES["Expand"]:
        (
            input_rank,
            output_rank,
            dtype,
            control_mode,
            control_rank,
            layout,
            producer_opcode,
        ) = struct.unpack_from("<BBBBBBH", record, 8)
        target_dims = struct.unpack_from("<4i", record, 16)
        reject(
            input_rank > 4
            or output_rank < input_rank
            or output_rank > 4
            or dtype not in {1, 7}
            or control_rank != 1
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "Expand rank/layout rejected",
        )
        if control_mode == ATTRIBUTE_CONTROL_INITIALIZER:
            reject(
                producer_opcode != 0
                or any(dim <= 0 for dim in target_dims[:output_rank])
                or any(target_dims[output_rank:]),
                "Expand static target rejected",
            )
        elif control_mode == ATTRIBUTE_CONTROL_DYNAMIC:
            reject(
                producer_opcode not in AOT_OPCODES.values() or any(target_dims),
                "Expand dynamic target rejected",
            )
        else:
            raise CompileError("Expand control mode rejected")
        result.update(
            input_rank=input_rank,
            output_rank=output_rank,
            dtype=dtype,
            control_mode=control_mode,
            producer_opcode=producer_opcode,
            parameters=target_dims[:output_rank]
            if control_mode == ATTRIBUTE_CONTROL_INITIALIZER
            else (),
        )
    elif kind == AOT_OPCODES["Shape"]:
        start, end, input_rank, output_rank, has_end, layout = struct.unpack_from(
            "<iiBBBB", record, 8
        )
        reject(any(record[20:24]), "Shape reserved bytes rejected")
        reject(
            start != 0
            or end != 0
            or has_end != 0
            or input_rank == 0
            or input_rank > 4
            or output_rank != 1
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "Shape contract rejected",
        )
        result.update(
            start=start,
            end=end,
            has_end=has_end,
            input_rank=input_rank,
            output_rank=output_rank,
        )
    elif kind == AOT_OPCODES["Slice"]:
        input_rank, output_rank, dtype, control_count, flags, layout, reserved = (
            struct.unpack_from("<BBBBBBH", record, 8)
        )
        control_modes = struct.unpack_from("<4B", record, 16)
        control_values = struct.unpack_from("<4q", record, 24)
        producer_opcodes = struct.unpack_from("<4H", record, 56)
        reject(reserved != 0 or any(record[20:24]), "Slice reserved bytes rejected")
        axes_present = bool(flags & 1)
        steps_present = bool(flags & 2)
        reject(
            input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or dtype not in {1, 7}
            or control_count != 2 + int(axes_present) + int(steps_present)
            or flags & ~3
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "Slice rank/control contract rejected",
        )
        expected_presence = (True, True, axes_present, steps_present)
        for slot, present in enumerate(expected_presence):
            mode = control_modes[slot]
            value = control_values[slot]
            producer_opcode = producer_opcodes[slot]
            if not present:
                reject(
                    mode != ATTRIBUTE_CONTROL_ABSENT
                    or value != 0
                    or producer_opcode != 0,
                    "Slice absent control rejected",
                )
            elif mode == ATTRIBUTE_CONTROL_INITIALIZER:
                reject(producer_opcode != 0, "Slice initializer provenance rejected")
            elif mode == ATTRIBUTE_CONTROL_DYNAMIC:
                reject(
                    value != 0 or producer_opcode not in AOT_OPCODES.values(),
                    "Slice dynamic provenance rejected",
                )
            else:
                raise CompileError("Slice control mode rejected")
        reject(
            control_modes[0] != ATTRIBUTE_CONTROL_INITIALIZER,
            "Slice starts must be an initializer",
        )
        reject(
            axes_present
            and (
                control_modes[2] != ATTRIBUTE_CONTROL_INITIALIZER
                or control_values[2] < -input_rank
                or control_values[2] >= input_rank
            ),
            "Slice axes rejected",
        )
        reject(
            steps_present
            and (
                control_modes[3] != ATTRIBUTE_CONTROL_INITIALIZER
                or control_values[3] != 1
            ),
            "Slice step rejected",
        )
        result.update(
            input_rank=input_rank,
            output_rank=output_rank,
            dtype=dtype,
            control_count=control_count,
            flags=flags,
            control_modes=control_modes,
            control_values=control_values,
            producer_opcodes=producer_opcodes,
        )
    elif kind == AOT_OPCODES["Pad"]:
        input_rank, output_rank, dtype, mode, count, layout, reserved = struct.unpack_from(
            "<BBBBBBH", record, 8
        )
        pads = struct.unpack_from("<8I", record, 16)
        reject(
            reserved != 0
            or input_rank == 0
            or input_rank > 4
            or output_rank != input_rank
            or dtype != 1
            or mode != ATTRIBUTE_PAD_REFLECT
            or count != input_rank * 2
            or layout != ATTRIBUTE_LAYOUT_CHECKED
            or any(pads[count:]),
            "Pad contract rejected",
        )
        result.update(
            input_rank=input_rank,
            output_rank=output_rank,
            dtype=dtype,
            mode=mode,
            parameters=pads[:count],
        )
    elif kind == AOT_OPCODES["NonZero"]:
        input_rank, output_rank, input_dtype, output_dtype, layout, row_major = (
            struct.unpack_from("<BBBBBB", record, 8)
        )
        reject(any(record[14:16]), "NonZero reserved bytes rejected")
        reject(
            input_rank != 1
            or output_rank != 2
            or input_dtype != 9
            or output_dtype != 7
            or layout != ATTRIBUTE_LAYOUT_CHECKED
            or row_major != 1,
            "NonZero contract rejected",
        )
        result.update(
            input_rank=input_rank,
            output_rank=output_rank,
            input_dtype=input_dtype,
            output_dtype=output_dtype,
            row_major=row_major,
        )
    elif kind == AOT_OPCODES["ScatterND"]:
        (
            data_rank,
            indices_rank,
            updates_rank,
            output_rank,
            dtype,
            tuple_len,
            reduction,
            ordering,
            layout,
        ) = struct.unpack_from("<BBBBBBBBB", record, 8)
        reject(any(record[17:20]), "ScatterND reserved bytes rejected")
        reject(
            data_rank != 2
            or indices_rank != 3
            or updates_rank != 2
            or output_rank != 2
            or dtype != 1
            or tuple_len != 2
            or reduction != ATTRIBUTE_SCATTER_REDUCTION_NONE
            or ordering != ATTRIBUTE_SCATTER_ORDERED_UNIQUE
            or layout != ATTRIBUTE_LAYOUT_CHECKED,
            "ScatterND contract rejected",
        )
        result.update(
            input_rank=data_rank,
            indices_rank=indices_rank,
            updates_rank=updates_rank,
            output_rank=output_rank,
            dtype=dtype,
            tuple_len=tuple_len,
            reduction=reduction,
            ordering=ordering,
        )
    else:
        input_rank, output_rank, dtype, flags, allowzero, count, layout, reserved = (
            struct.unpack_from("<BBBBBBBB", record, 8)
        )
        parameters = struct.unpack_from("<4i", record, 16)
        reject(reserved != 0, "view reserved byte rejected")
        reject(
            input_rank > 4
            or output_rank > 4
            or dtype not in {1, 6, 7}
            or flags & ~(ATTRIBUTE_VIEW_ALIAS | ATTRIBUTE_VIEW_STATIC_CONTROL)
            or not flags & ATTRIBUTE_VIEW_ALIAS
            or allowzero != 0
            or count > 4
            or layout != ATTRIBUTE_LAYOUT_CHECKED
            or any(parameters[count:]),
            "view attribute contract rejected",
        )
        is_static = bool(flags & ATTRIBUTE_VIEW_STATIC_CONTROL)
        if kind == AOT_OPCODES["Reshape"]:
            reject(
                output_rank == 0
                or (is_static and count != output_rank)
                or (not is_static and count != 0),
                "Reshape control contract rejected",
            )
            reject(
                is_static
                and (
                    sum(value == -1 for value in parameters[:count]) > 1
                    or any(value < -1 for value in parameters[:count])
                    or any(
                        value == 0 and axis >= input_rank
                        for axis, value in enumerate(parameters[:count])
                    )
                ),
                "Reshape parameters rejected",
            )
        elif kind == AOT_OPCODES["Unsqueeze"]:
            reject(
                not is_static or count == 0 or output_rank != input_rank + count,
                "Unsqueeze control contract rejected",
            )
            normalized = tuple(
                axis + output_rank if axis < 0 else axis
                for axis in parameters[:count]
            )
            reject(
                any(axis < 0 or axis >= output_rank for axis in normalized)
                or len(set(normalized)) != len(normalized),
                "Unsqueeze axes rejected",
            )
        else:
            reject(
                not is_static or count == 0 or output_rank + count != input_rank,
                "Squeeze control contract rejected",
            )
            normalized = tuple(
                axis + input_rank if axis < 0 else axis
                for axis in parameters[:count]
            )
            reject(
                any(axis < 0 or axis >= input_rank for axis in normalized)
                or len(set(normalized)) != len(normalized),
                "Squeeze axes rejected",
            )
        result.update(
            input_rank=input_rank,
            output_rank=output_rank,
            dtype=dtype,
            flags=flags,
            allowzero=allowzero,
            parameter_count=count,
            parameters=parameters[:count],
        )
    return result


@dataclass(frozen=True)
class LoweringRecord:
    source_index: int
    op_type: str
    opcode: int
    phase: int
    inputs: tuple[int, ...]
    outputs: tuple[int, ...]
    attributes: bytes
    variant: str

    def canonical_bytes(self) -> bytes:
        reject(self.source_index < 0 or self.source_index > 0xFFFF_FFFF, "source index")
        reject(self.opcode != AOT_OPCODES.get(self.op_type), "lowering opcode mismatch")
        reject(self.phase not in {0, 1}, "lowering phase rejected")
        reject(not self.outputs, "lowering has no outputs")
        reject(
            len(self.inputs) > 0xFFFF or len(self.outputs) > 0xFFFF,
            "lowering binding count rejected",
        )
        reject(
            any(tensor_id < 0 or tensor_id > 0xFFFF_FFFF for tensor_id in self.inputs + self.outputs),
            "lowering tensor ID rejected",
        )
        inspect_attribute_record(self.attributes, self.opcode)
        header = struct.pack(
            "<IHBBHHI",
            self.source_index,
            self.opcode,
            self.phase,
            0,
            len(self.inputs),
            len(self.outputs),
            len(self.attributes),
        )
        bindings = b"".join(
            struct.pack("<I", tensor_id) for tensor_id in self.inputs + self.outputs
        )
        return header + bindings + self.attributes


def lowering_plan_bytes(records: Sequence[LoweringRecord]) -> bytes:
    reject(len(records) > 0xFFFF_FFFF, "lowering record count rejected")
    return b"KKLOWER1" + struct.pack("<I", len(records)) + b"".join(
        record.canonical_bytes() for record in records
    )


def lowering_plan_sha256(records: Sequence[LoweringRecord]) -> str:
    return hashlib.sha256(lowering_plan_bytes(records)).hexdigest()


@dataclass(frozen=True)
class AotTensor:
    dtype: int
    dims: tuple[int, ...]
    storage: int
    phase: int
    flags: int = 0
    slot_id: int = AOT_NO_ID
    view_of: int = AOT_NO_ID
    storage_offset: int = 0
    strides: tuple[int, int, int, int] | None = None
    symbolic_dim: int = AOT_STATIC_DIM
    frame_multiplier: int = 0
    frame_addend: int = 0
    alignment: int = 1

    def encode(self) -> bytes:
        reject(self.dtype not in AOT_DTYPE_BYTES, "fixture tensor dtype rejected")
        reject(len(self.dims) > 4, "fixture tensor rank rejected")
        reject(self.storage not in {1, 2, 3, 4}, "fixture tensor storage rejected")
        reject(self.phase not in {0, 1, 2}, "fixture tensor phase rejected")
        reject(self.flags & ~7 != 0, "fixture tensor flags rejected")
        reject(
            self.alignment <= 0
            or self.alignment > AOT_ARENA_ALIGNMENT
            or self.alignment & (self.alignment - 1) != 0,
            "fixture tensor alignment rejected",
        )
        max_dims = tuple(self.dims) + (1,) * (4 - len(self.dims))
        strides = self.strides or contiguous_strides(self.dtype, self.dims)
        reject(len(strides) != 4, "fixture tensor strides rejected")
        capacity = logical_bytes(self.dtype, self.dims)
        record = bytearray(128)
        struct.pack_into(
            "<BBBBIIIQQ4I4Q",
            record,
            0,
            self.dtype,
            len(self.dims),
            self.storage,
            self.phase,
            self.flags,
            self.slot_id,
            self.view_of,
            self.storage_offset,
            capacity,
            *max_dims,
            *strides,
        )
        struct.pack_into("<B", record, 80, self.symbolic_dim)
        struct.pack_into(
            "<IqI",
            record,
            84,
            self.frame_multiplier,
            self.frame_addend,
            self.alignment,
        )
        return bytes(record)


@dataclass(frozen=True)
class AotSlot:
    kind: int
    phase: int
    alignment: int
    fixed_offset: int
    bytes_multiplier: int
    bytes_addend: int
    live_start: int
    live_end: int

    def encode(self) -> bytes:
        reject(self.kind not in {1, 2}, "fixture slot kind rejected")
        reject(self.phase not in {0, 1, 2}, "fixture slot phase rejected")
        record = bytearray(64)
        struct.pack_into(
            "<BBHIQQqII",
            record,
            0,
            self.kind,
            self.phase,
            0,
            self.alignment,
            self.fixed_offset,
            self.bytes_multiplier,
            self.bytes_addend,
            self.live_start,
            self.live_end,
        )
        return bytes(record)


@dataclass(frozen=True)
class AotOp:
    opcode: int
    phase: int
    inputs: tuple[int, ...]
    outputs: tuple[int, ...]
    work_units: int = 1
    flags: int = 0
    attributes: bytes = b""


@dataclass(frozen=True)
class AotPhase:
    phase_id: int
    flags: int
    op_start: int
    op_end: int
    arena_min_bytes: int
    arena_max_bytes: int
    frame_min: int
    frame_max: int

    def encode(self) -> bytes:
        record = bytearray(48)
        struct.pack_into(
            "<BBHIIIQQIIII",
            record,
            0,
            self.phase_id,
            self.flags,
            0,
            self.op_start,
            self.op_end,
            0,
            self.arena_min_bytes,
            self.arena_max_bytes,
            AOT_ARENA_ALIGNMENT,
            self.frame_min,
            self.frame_max,
            0,
        )
        return bytes(record)


@dataclass(frozen=True)
class AotProgram:
    tensors: tuple[AotTensor, ...]
    slots: tuple[AotSlot, ...]
    ops: tuple[AotOp, ...]
    phases: tuple[AotPhase, AotPhase]
    data_prefix: bytes = b""


def emit_aot(program: AotProgram, model_sha256: bytes, voices_sha256: bytes) -> bytes:
    """Emit the canonical little-endian v1 artifact."""

    reject(len(model_sha256) != 32 or not any(model_sha256), "model SHA-256 rejected")
    reject(len(voices_sha256) != 32 or not any(voices_sha256), "voices SHA-256 rejected")
    reject(len(program.phases) != 2, "v1 requires exactly two phases")

    data = bytearray(program.data_prefix)
    bindings: list[int] = []
    op_records = bytearray()
    for op_index, op in enumerate(program.ops):
        reject(op.opcode not in AOT_OPCODES.values(), f"op {op_index}: unknown opcode")
        reject(op.phase not in {0, 1}, f"op {op_index}: invalid phase")
        reject(not op.outputs, f"op {op_index}: no outputs")
        reject(op.flags & ~1 != 0, f"op {op_index}: invalid flags")
        reject(op.work_units <= 0, f"op {op_index}: invalid work units")
        binding_start = len(bindings)
        bindings.extend(op.inputs)
        bindings.extend(op.outputs)
        if op.attributes:
            inspect_attribute_record(op.attributes, op.opcode)
            attribute_offset = align_up(len(data), attribute_alignment(op.attributes))
            data.extend(b"\0" * (attribute_offset - len(data)))
            data.extend(op.attributes)
            attribute_len = len(op.attributes)
        else:
            attribute_offset = 0
            attribute_len = 0
        op_records.extend(
            struct.pack(
                "<HHB3xIHHQII8x",
                op.opcode,
                op.flags,
                op.phase,
                binding_start,
                len(op.inputs),
                len(op.outputs),
                attribute_offset,
                attribute_len,
                op.work_units,
            )
        )

    reject(
        any(binding >= len(program.tensors) for binding in bindings),
        "binding tensor ID rejected",
    )
    sections = {
        "tensors": b"".join(tensor.encode() for tensor in program.tensors),
        "slots": b"".join(slot.encode() for slot in program.slots),
        "ops": bytes(op_records),
        "bindings": b"".join(struct.pack("<I", binding) for binding in bindings),
        "phases": b"".join(phase.encode() for phase in program.phases),
        "data": bytes(data),
    }

    artifact = bytearray(AOT_HEADER_BYTES)
    entries: list[tuple[int, int, int, int, int]] = []
    cursor = AOT_HEADER_BYTES
    for kind, alignment, stride, name in AOT_SECTION_SPECS:
        offset = align_up(cursor, alignment)
        artifact.extend(b"\0" * (offset - len(artifact)))
        payload = sections[name]
        artifact.extend(payload)
        count = len(payload) if name == "data" else len(payload) // stride
        reject(name != "data" and len(payload) != count * stride, f"{name} section stride")
        entries.append((kind, alignment, offset, count, stride))
        cursor = offset + len(payload)

    struct.pack_into(
        "<8sHHIQ",
        artifact,
        0,
        AOT_MAGIC,
        AOT_VERSION,
        AOT_ENDIAN_TAG,
        AOT_HEADER_BYTES,
        len(artifact),
    )
    struct.pack_into(
        "<HHII",
        artifact,
        24,
        AOT_SECTION_COUNT,
        AOT_PHASE_COUNT,
        0,
        AOT_ARENA_ALIGNMENT,
    )
    struct.pack_into("<5H", artifact, 36, 128, 64, 40, 48, 4)
    artifact[96:128] = model_sha256
    artifact[128:160] = voices_sha256
    for index, (kind, alignment, offset, count, stride) in enumerate(entries):
        struct.pack_into(
            "<HHIQQII",
            artifact,
            160 + index * 32,
            kind,
            0,
            alignment,
            offset,
            count,
            stride,
            0,
        )
    # Bind the complete header (including both provenance hashes and the
    # directory) as well as every payload byte. The seal field itself is
    # canonicalized to zero while hashing.
    artifact[64:96] = hashlib.sha256(
        artifact[:64] + bytes(32) + artifact[96:]
    ).digest()
    return bytes(artifact)


def inspect_aot(artifact: bytes) -> dict[str, Any]:
    """Strict structural reader used by the host-side round-trip tests."""

    reject(len(artifact) < AOT_HEADER_BYTES, "artifact is shorter than header")
    reject(artifact[:8] != AOT_MAGIC, "artifact magic rejected")
    version, endian, header_bytes = struct.unpack_from("<HHI", artifact, 8)
    reject(
        (version, endian, header_bytes)
        != (AOT_VERSION, AOT_ENDIAN_TAG, AOT_HEADER_BYTES),
        "artifact version rejected",
    )
    artifact_bytes = struct.unpack_from("<Q", artifact, 16)[0]
    reject(artifact_bytes != len(artifact), "artifact length rejected")
    section_count, phase_count, flags, arena_alignment = struct.unpack_from("<HHII", artifact, 24)
    reject(
        (section_count, phase_count, flags, arena_alignment) != (6, 2, 0, 64),
        "artifact fixed header rejected",
    )
    reject(
        struct.unpack_from("<5H", artifact, 36) != (128, 64, 40, 48, 4),
        "artifact record sizes rejected",
    )
    reject(any(artifact[46:64]), "artifact header reserved bytes rejected")
    reject(
        not any(artifact[96:128]) or not any(artifact[128:160]),
        "artifact provenance hash rejected",
    )
    observed_seal = hashlib.sha256(
        artifact[:64] + bytes(32) + artifact[96:]
    ).digest()
    reject(observed_seal != artifact[64:96], "artifact seal rejected")

    cursor = AOT_HEADER_BYTES
    sections: dict[str, dict[str, int]] = {}
    for index, (kind, alignment, stride, name) in enumerate(AOT_SECTION_SPECS):
        entry = struct.unpack_from("<HHIQQII", artifact, 160 + index * 32)
        actual_kind, entry_flags, actual_alignment, offset, count, actual_stride, reserved = entry
        reject(
            (actual_kind, entry_flags, actual_alignment, actual_stride, reserved)
            != (kind, 0, alignment, stride, 0),
            f"{name} directory entry rejected",
        )
        expected_offset = align_up(cursor, alignment)
        reject(offset != expected_offset, f"{name} section offset rejected")
        reject(any(artifact[cursor:offset]), f"{name} section padding rejected")
        length = count if name == "data" else count * stride
        end = offset + length
        reject(end > len(artifact), f"{name} section bounds rejected")
        sections[name] = {"offset": offset, "count": count, "stride": stride, "end": end}
        cursor = end
    reject(cursor != len(artifact), "artifact has trailing bytes")

    op_section = sections["ops"]
    binding_section = sections["bindings"]
    data_section = sections["data"]
    attribute_kinds: Counter[int] = Counter()
    previous_attribute_end: int | None = None
    for index in range(op_section["count"]):
        offset = op_section["offset"] + index * 40
        (
            opcode,
            op_flags,
            phase,
            binding_start,
            input_count,
            output_count,
            attribute_offset,
            attribute_len,
            work_units,
        ) = struct.unpack_from("<HHB3xIHHQII", artifact, offset)
        reject(opcode not in AOT_OPCODES.values(), f"op {index}: opcode rejected")
        reject(op_flags & ~1 != 0 or phase not in {0, 1}, f"op {index}: flags rejected")
        reject(output_count == 0 or work_units == 0, f"op {index}: arity/work rejected")
        reject(
            binding_start + input_count + output_count > binding_section["count"],
            f"op {index}: binding range rejected",
        )
        for binding_index in range(
            binding_start, binding_start + input_count + output_count
        ):
            binding_offset = binding_section["offset"] + binding_index * 4
            tensor_id = struct.unpack_from("<I", artifact, binding_offset)[0]
            reject(
                tensor_id >= sections["tensors"]["count"],
                f"op {index}: tensor binding rejected",
            )
        reject(
            any(artifact[offset + 5 : offset + 8])
            or any(artifact[offset + 32 : offset + 40]),
            f"op {index}: reserved bytes rejected",
        )
        if attribute_len == 0:
            reject(attribute_offset != 0, f"op {index}: empty attribute is non-canonical")
        else:
            attribute_end = attribute_offset + attribute_len
            reject(
                attribute_end > data_section["count"],
                f"op {index}: attribute range rejected",
            )
            start = data_section["offset"] + attribute_offset
            attribute_record = artifact[start : start + attribute_len]
            alignment = attribute_alignment(attribute_record)
            reject(
                attribute_offset % alignment != 0,
                f"op {index}: attribute alignment rejected",
            )
            if previous_attribute_end is not None:
                expected_offset = align_up(previous_attribute_end, alignment)
                reject(
                    attribute_offset != expected_offset,
                    f"op {index}: attribute order/gap rejected",
                )
                padding_start = data_section["offset"] + previous_attribute_end
                padding_end = data_section["offset"] + expected_offset
                reject(
                    any(artifact[padding_start:padding_end]),
                    f"op {index}: attribute padding rejected",
                )
            decoded = inspect_attribute_record(
                attribute_record, opcode
            )
            attribute_kinds[int(decoded["kind"])] += 1
            previous_attribute_end = attribute_end
    phase_section = sections["phases"]
    phase_ids = tuple(artifact[phase_section["offset"] + index * 48] for index in range(2))
    reject(phase_ids != (0, 1), "phase IDs rejected")
    return {
        "artifact_bytes": len(artifact),
        "artifact_sha256": artifact[64:96].hex(),
        "model_sha256": artifact[96:128].hex(),
        "voices_sha256": artifact[128:160].hex(),
        "sections": {name: value["count"] for name, value in sections.items()},
        "attribute_abi": {
            "version": ATTRIBUTE_ABI_VERSION,
            "records": sum(attribute_kinds.values()),
            "kind_counts": {
                f"0x{kind:04x}": count
                for kind, count in sorted(attribute_kinds.items())
            },
        },
    }


def synthetic_fixture_artifact() -> bytes:
    """Return a tiny deterministic native-fusion and dynamic-slot fixture."""

    data = bytearray(16 + 3)
    data[0:12] = bytes(range(12))
    data[16:19] = bytes((0x7F, 0x80, 0x01))
    tensors = (
        # External graph input [1,4].
        AotTensor(1, (1, 4), 4, 0, flags=2, alignment=4),
        # Shared constant GEMM weight [4,3].
        AotTensor(5, (4, 3), 3, 2, flags=1, storage_offset=0, alignment=16),
        # Fixed phase-0 GEMM result [1,3].
        AotTensor(1, (1, 3), 1, 0, slot_id=0, alignment=64),
        # Static rank-changing view consumed by the resolver.
        AotTensor(1, (3,), 2, 0, view_of=2, alignment=64),
        # Runtime-owned scalar crossing the phase boundary.
        AotTensor(3, (), 4, 2, alignment=8),
        # Shared constant Conv1d weight [1,1,3].
        AotTensor(5, (1, 1, 3), 3, 2, flags=1, storage_offset=16, alignment=16),
        # Phase-1 waveform capacity [1,1,2*F], F in [1,32].
        AotTensor(
            1,
            (1, 1, 64),
            1,
            1,
            flags=4,
            slot_id=1,
            symbolic_dim=2,
            frame_multiplier=2,
            alignment=64,
        ),
    )
    slots = (
        AotSlot(1, 0, 64, 0, 0, 12, 0, 2),
        AotSlot(2, 1, 64, 0, 8, 0, 2, 4),
    )
    ops = (
        AotOp(AOT_OPCODES["DynamicQuantizedGemm"], 0, (0, 1), (2,), work_units=12),
        AotOp(AOT_OPCODES["ResolveDecoderShape"], 0, (3,), (4,)),
        AotOp(AOT_OPCODES["DynamicQuantizedConv1d"], 1, (4, 5), (6,), work_units=64),
    )
    phases = (
        AotPhase(0, 0, 0, 2, 64, 64, 0, 0),
        AotPhase(1, 1, 2, 3, 64, 256, 1, 32),
    )
    return emit_aot(
        AotProgram(tensors, slots, ops, phases, bytes(data)),
        bytes.fromhex(PINNED_MODEL_SHA256),
        bytes.fromhex(PINNED_VOICES_SHA256),
    )


def synthetic_attribute_fixture_artifact() -> bytes:
    """Emit one parseable v1 attribute record for every admitted CPU kind."""

    tensors: list[AotTensor] = []
    ops: list[AotOp] = []

    def external(dtype: int, dims: tuple[int, ...]) -> int:
        tensor_id = len(tensors)
        alignment = 8 if dtype == 3 else 4
        tensors.append(AotTensor(dtype, dims, 4, 0, alignment=alignment))
        return tensor_id

    def operation(
        op_type: str,
        input_specs: Sequence[tuple[int, tuple[int, ...]]],
        output_specs: Sequence[tuple[int, tuple[int, ...]]],
        attrs: bytes,
    ) -> None:
        inputs = tuple(external(dtype, dims) for dtype, dims in input_specs)
        outputs = tuple(external(dtype, dims) for dtype, dims in output_specs)
        ops.append(AotOp(AOT_OPCODES[op_type], 0, inputs, outputs, attributes=attrs))

    operation(
        "Add",
        ((1, (1, 4)), (1, (4,))),
        ((1, (1, 4)),),
        binary_attribute("Add", 2, 1, 2),
    )
    operation(
        "Mul",
        ((1, ()), (1, (1, 2, 3))),
        ((1, (1, 2, 3)),),
        binary_attribute("Mul", 0, 3, 3),
    )
    operation(
        "Div",
        ((1, (4,)), (1, ())),
        ((1, (4,)),),
        binary_attribute("Div", 1, 0, 1),
    )
    operation(
        "Sub",
        ((1, (1, 2, 3)), (1, (1, 2, 3))),
        ((1, (1, 2, 3)),),
        binary_attribute("Sub", 3, 3, 3),
    )
    for op_type in sorted(LOWERED_PARAMETERLESS_UNARY_OPS):
        operation(
            op_type,
            ((1, (1, 2, 3)),),
            ((1, (1, 2, 3)),),
            parameterless_unary_attribute(op_type),
        )
    operation(
        "LeakyRelu",
        ((1, (1, 2, 3)),),
        ((1, (1, 2, 3)),),
        leaky_relu_attribute(0x3E4CCCCD, 3, 3),
    )
    operation(
        "ReduceMean",
        ((1, (1, 2, 4)), (3, (1,))),
        ((1, (1, 2, 1)),),
        reduce_mean_attribute(2, 1, 0, 3, 3),
    )
    operation(
        "LayerNormalization",
        ((1, (1, 2, 4)), (1, (4,)), (1, (4,))),
        ((1, (1, 2, 4)),),
        layer_norm_attribute(-1, 0x3727C5AC, 1, 3, 3, 1),
    )
    operation(
        "Softmax",
        ((1, (1, 2, 3, 4)),),
        ((1, (1, 2, 3, 4)),),
        softmax_attribute(-1, 4, 4),
    )
    operation(
        "FastGelu",
        ((1, (1, 2, 4)), (1, (4,))),
        ((1, (1, 2, 4)),),
        fast_gelu_attribute(1, 3, 3, 1),
    )
    operation(
        "SkipLayerNormalization",
        ((1, (1, 2, 4)), (1, (1, 2, 4)), (1, (4,)), (1, (4,))),
        ((1, (1, 2, 4)),),
        skip_layer_norm_attribute(0x2B8CBCCC, 3, 3, 1),
    )
    operation(
        "Transpose",
        ((1, (1, 2, 3)),),
        ((1, (1, 3, 2)),),
        transpose_attribute((0, 2, 1), 3, 1),
    )
    operation(
        "Gather",
        ((1, (2, 3)), (3, ())),
        ((1, (3,)),),
        gather_attribute(0, 2, 0, 1, 1, ATTRIBUTE_CONTROL_INITIALIZER),
    )
    operation(
        "Concat",
        ((1, (2, 2)), (1, (2, 3))),
        ((1, (2, 5)),),
        concat_attribute(1, 2, 2, 1, 2),
    )
    operation(
        "Split",
        ((1, (2, 5)), (3, (2,))),
        ((1, (2, 2)), (1, (2, 3))),
        split_attribute(1, 2, 3, 2, 2, 1, 2),
    )
    operation(
        "Expand",
        ((3, (2, 1)), (3, (3,))),
        ((3, (3, 2, 2)),),
        expand_attribute(
            2,
            3,
            7,
            ATTRIBUTE_CONTROL_DYNAMIC,
            1,
            AOT_OPCODES["Where"],
        ),
    )
    operation(
        "Shape",
        ((1, (2, 3, 4)),),
        ((3, (3,)),),
        shape_attribute(0, 0, 0, 3, 1),
    )
    operation(
        "Slice",
        ((1, (2, 3, 4)), (3, (1,)), (3, (1,)), (3, (1,)), (3, (1,))),
        ((1, (2, 2, 4)),),
        slice_attribute(
            3,
            3,
            1,
            4,
            True,
            True,
            (
                ATTRIBUTE_CONTROL_INITIALIZER,
                ATTRIBUTE_CONTROL_INITIALIZER,
                ATTRIBUTE_CONTROL_INITIALIZER,
                ATTRIBUTE_CONTROL_INITIALIZER,
            ),
            (0, 2, 1, 1),
            (0, 0, 0, 0),
        ),
    )
    operation(
        "Pad",
        ((1, (1, 1, 3)), (3, (6,))),
        ((1, (1, 1, 6)),),
        pad_attribute(3, 3, 1, (0, 0, 2, 0, 0, 1)),
    )
    operation(
        "NonZero",
        ((6, (4,)),),
        ((3, (1, 2)),),
        nonzero_attribute(1, 2),
    )
    operation(
        "ScatterND",
        ((1, (2, 3)), (3, (1, 2, 2)), (1, (1, 2))),
        ((1, (2, 3)),),
        scatter_nd_attribute(2, 3, 2, 2, 1, 2),
    )

    def view_operation(
        op_type: str,
        dtype: int,
        input_dims: tuple[int, ...],
        control_dims: tuple[int, ...],
        output_dims: tuple[int, ...],
        attrs: bytes,
    ) -> None:
        owner = external(dtype, input_dims)
        control = external(3, control_dims)
        output = len(tensors)
        tensors.append(
            AotTensor(
                dtype,
                output_dims,
                2,
                0,
                view_of=owner,
                alignment=8 if dtype == 3 else 4,
            )
        )
        ops.append(
            AotOp(
                AOT_OPCODES[op_type],
                0,
                (owner, control),
                (output,),
                flags=1,
                attributes=attrs,
            )
        )

    view_operation(
        "Reshape",
        2,
        (4,),
        (3,),
        (1, 4, 1),
        view_attribute(
            "Reshape", 1, 3, 6, static_control=True, parameters=(1, -1, 1)
        ),
    )
    view_operation(
        "Unsqueeze",
        3,
        (),
        (1,),
        (1,),
        view_attribute("Unsqueeze", 0, 1, 7, static_control=True, parameters=(0,)),
    )
    view_operation(
        "Squeeze",
        1,
        (1, 4, 1),
        (1,),
        (1, 4),
        view_attribute("Squeeze", 3, 2, 1, static_control=True, parameters=(1,)),
    )

    phases = (
        AotPhase(0, 0, 0, len(ops), 0, 0, 0, 0),
        AotPhase(1, 1, len(ops), len(ops), 0, 0, 1, 1),
    )
    return emit_aot(
        AotProgram(tuple(tensors), (), tuple(ops), phases),
        bytes.fromhex(PINNED_MODEL_SHA256),
        bytes.fromhex(PINNED_VOICES_SHA256),
    )


def node_domain(node: Any) -> str:
    return node.domain or "ai.onnx"


def node_key(node: Any) -> str:
    if node.domain:
        return f"{node.domain}::{node.op_type}"
    return node.op_type


def attributes(onnx: Any, node: Any) -> dict[str, Any]:
    return {
        attribute.name: onnx.helper.get_attribute_value(attribute)
        for attribute in node.attribute
    }


def tensor_shape(value_info: Any) -> tuple[int | str | None, ...] | None:
    tensor_type = value_info.type.tensor_type
    if not tensor_type.HasField("shape"):
        return None
    dims: list[int | str | None] = []
    for dim in tensor_type.shape.dim:
        if dim.HasField("dim_value"):
            dims.append(int(dim.dim_value))
        elif dim.HasField("dim_param"):
            dims.append(dim.dim_param)
        else:
            dims.append(None)
    return tuple(dims)


@dataclass(frozen=True)
class TensorFact:
    tensor_id: int
    name: str
    dtype: int
    rank: int
    declared_shape: tuple[int | str | None, ...] | None
    producer: int | None
    initializer: bool
    graph_input: bool
    graph_output: bool
    alias_of: int | None
    live_start: int
    live_end: int
    phase: int


@dataclass(frozen=True)
class QuantFusion:
    kind: str
    kernel_index: int
    kernel_name: str
    dynamic_quant_index: int
    scale_index: int
    cast_index: int
    dequant_mul_index: int
    int32_bias: bool
    float_bias: bool
    result_tensor: str


@dataclass
class GraphAnalysis:
    model_path: Path
    model_bytes: int
    model_sha256: str
    voices_path: Path | None
    voices_bytes: int
    voices_sha256: str
    model: Any
    onnx: Any
    producers: dict[str, int]
    consumers: dict[str, list[int]]
    initializers: dict[str, Any]
    value_infos: dict[str, Any]
    dtypes: dict[str, int]
    ranks: dict[str, int]
    tensors: list[TensorFact]
    quant_fusions: list[QuantFusion]
    phase_cut: int
    lowerings: list[LoweringRecord] = field(default_factory=list)
    report: dict[str, Any] = field(default_factory=dict)


def initializer_values(onnx: Any, tensor: Any) -> tuple[int | float, ...]:
    array = onnx.numpy_helper.to_array(tensor)
    return tuple(item.item() for item in array.reshape(-1))


def axes_from_input(
    onnx: Any, node: Any, initializers: Mapping[str, Any], input_index: int
) -> tuple[int, ...] | None:
    if len(node.input) <= input_index or not node.input[input_index]:
        return None
    tensor = initializers.get(node.input[input_index])
    if tensor is None:
        return None
    return tuple(int(value) for value in initializer_values(onnx, tensor))


def _declared_shape(
    analysis: GraphAnalysis, tensor_name: str
) -> tuple[int | str | None, ...] | None:
    tensor = analysis.initializers.get(tensor_name)
    if tensor is not None:
        return tuple(int(dim) for dim in tensor.dims)
    value_info = analysis.value_infos.get(tensor_name)
    return None if value_info is None else tensor_shape(value_info)


def _validate_declared_rank(analysis: GraphAnalysis, tensor_name: str) -> None:
    shape = _declared_shape(analysis, tensor_name)
    if shape is None:
        return
    reject(
        len(shape) != analysis.ranks[tensor_name],
        f"tensor {tensor_name!r}: declared/inferred rank mismatch",
    )
    reject(
        any(isinstance(dim, int) and dim <= 0 for dim in shape),
        f"tensor {tensor_name!r}: non-positive dimension rejected",
    )


def _validate_same_numeric_shape(
    analysis: GraphAnalysis, names: Sequence[str], context: str
) -> None:
    shapes = [_declared_shape(analysis, name) for name in names]
    known = [shape for shape in shapes if shape is not None]
    if not known:
        return
    rank = len(known[0])
    reject(any(len(shape) != rank for shape in known), f"{context}: rank mismatch")
    for axis in range(rank):
        numeric = {
            int(shape[axis])
            for shape in known
            if isinstance(shape[axis], int)
        }
        reject(len(numeric) > 1, f"{context}: dimension {axis} mismatch")


def _validate_binary_broadcast(
    analysis: GraphAnalysis, lhs: str, rhs: str, output: str, context: str
) -> None:
    lhs_rank = analysis.ranks[lhs]
    rhs_rank = analysis.ranks[rhs]
    output_rank = analysis.ranks[output]
    reject(output_rank != max(lhs_rank, rhs_rank), f"{context}: output rank mismatch")
    shapes = (
        _declared_shape(analysis, lhs),
        _declared_shape(analysis, rhs),
        _declared_shape(analysis, output),
    )
    if any(shape is None for shape in shapes):
        return
    lhs_shape, rhs_shape, output_shape = shapes
    assert lhs_shape is not None and rhs_shape is not None and output_shape is not None
    lhs_aligned = (1,) * (output_rank - lhs_rank) + lhs_shape
    rhs_aligned = (1,) * (output_rank - rhs_rank) + rhs_shape
    for axis, (lhs_dim, rhs_dim, output_dim) in enumerate(
        zip(lhs_aligned, rhs_aligned, output_shape)
    ):
        if isinstance(lhs_dim, int) and isinstance(rhs_dim, int):
            reject(
                lhs_dim != rhs_dim and lhs_dim != 1 and rhs_dim != 1,
                f"{context}: broadcast mismatch at axis {axis}",
            )
            if isinstance(output_dim, int):
                reject(
                    output_dim != max(lhs_dim, rhs_dim),
                    f"{context}: broadcast output mismatch at axis {axis}",
                )
        elif isinstance(output_dim, int):
            for input_dim in (lhs_dim, rhs_dim):
                reject(
                    isinstance(input_dim, int) and input_dim not in {1, output_dim},
                    f"{context}: runtime broadcast mismatch at axis {axis}",
                )


def _small_int_initializer(
    analysis: GraphAnalysis,
    tensor_name: str,
    context: str,
    *,
    max_count: int = 4,
) -> tuple[int, ...]:
    tensor = analysis.initializers.get(tensor_name)
    reject(tensor is None, f"{context}: controller is not an initializer")
    assert tensor is not None
    reject(
        int(tensor.data_type) != 7 or len(tensor.dims) != 1,
        f"{context}: controller must be rank-one INT64",
    )
    count = int(tensor.dims[0])
    reject(count < 1 or count > max_count, f"{context}: controller length rejected")
    values = initializer_values(analysis.onnx, tensor)
    reject(len(values) != count, f"{context}: controller payload length changed")
    reject(
        any(not isinstance(value, int) for value in values),
        f"{context}: controller payload is not integral",
    )
    return tuple(int(value) for value in values)


def _normalize_axes(axes: Sequence[int], rank: int, context: str) -> tuple[int, ...]:
    normalized = tuple(axis + rank if axis < 0 else axis for axis in axes)
    reject(
        any(axis < 0 or axis >= rank for axis in normalized)
        or len(set(normalized)) != len(normalized),
        f"{context}: axes rejected",
    )
    return normalized


def _validate_last_dimension_parameter(
    analysis: GraphAnalysis, data: str, parameter: str, context: str
) -> int | None:
    reject(
        parameter not in analysis.initializers,
        f"{context}: parameter {parameter!r} is not constant",
    )
    parameter_shape = _declared_shape(analysis, parameter)
    reject(
        parameter_shape is None
        or len(parameter_shape) != 1
        or not isinstance(parameter_shape[0], int)
        or parameter_shape[0] <= 0,
        f"{context}: parameter must be a non-empty rank-one tensor",
    )
    data_shape = _declared_shape(analysis, data)
    if data_shape is not None and isinstance(data_shape[-1], int):
        reject(
            data_shape[-1] != parameter_shape[0],
            f"{context}: parameter/last-dimension mismatch",
        )
    return int(parameter_shape[0])


def _validate_reshape_static_shape(
    analysis: GraphAnalysis,
    data: str,
    output: str,
    target: Sequence[int],
    context: str,
) -> None:
    input_rank = analysis.ranks[data]
    reject(
        sum(value == -1 for value in target) > 1
        or any(value < -1 for value in target),
        f"{context}: Reshape target values rejected",
    )
    for axis, value in enumerate(target):
        reject(value == 0 and axis >= input_rank, f"{context}: Reshape zero index rejected")

    source_shape = _declared_shape(analysis, data)
    output_shape = _declared_shape(analysis, output)
    resolved: list[int | str | None] = list(target)
    if source_shape is not None:
        for axis, value in enumerate(resolved):
            if value == 0:
                resolved[axis] = source_shape[axis]
    source_numeric = source_shape is not None and all(
        isinstance(dim, int) for dim in source_shape
    )
    known_target = [dim for dim in resolved if isinstance(dim, int) and dim > 0]
    if source_numeric:
        source_elements = 1
        for dim in source_shape:
            assert isinstance(dim, int)
            source_elements *= dim
        target_elements = 1
        for dim in known_target:
            target_elements *= dim
        infer_positions = [index for index, dim in enumerate(resolved) if dim == -1]
        if infer_positions:
            reject(
                target_elements == 0 or source_elements % target_elements != 0,
                f"{context}: Reshape inferred dimension is not integral",
            )
            resolved[infer_positions[0]] = source_elements // target_elements
        else:
            reject(
                target_elements != source_elements,
                f"{context}: Reshape element count mismatch",
            )
    if output_shape is not None:
        reject(len(output_shape) != len(resolved), f"{context}: Reshape output rank mismatch")
        for axis, (expected, actual) in enumerate(zip(resolved, output_shape)):
            reject(
                isinstance(expected, int)
                and expected > 0
                and isinstance(actual, int)
                and expected != actual,
                f"{context}: Reshape output dimension {axis} mismatch",
            )


def build_supported_lowerings(analysis: GraphAnalysis) -> list[LoweringRecord]:
    """Validate and lower only CPU kernels/views already implemented in Rust."""

    graph = analysis.model.graph
    tensor_ids = {tensor.name: tensor.tensor_id for tensor in analysis.tensors}
    tensor_facts = {tensor.name: tensor for tensor in analysis.tensors}
    records: list[LoweringRecord] = []

    def require_arity(node: Any, index: int, inputs: int, outputs: int) -> None:
        reject(
            len(node.input) != inputs
            or any(not name for name in node.input)
            or len(node.output) != outputs
            or any(not name for name in node.output),
            f"node {index} {node.name!r}: {node.op_type} arity rejected",
        )

    def require_f32(names: Sequence[str], context: str) -> None:
        reject(
            any(analysis.dtypes.get(name) != 1 for name in names),
            f"{context}: expected FLOAT tensors",
        )
        for name in names:
            _validate_declared_rank(analysis, name)
            reject(analysis.ranks[name] > 4, f"{context}: rank exceeds four")

    def append(index: int, node: Any, attrs: bytes, variant: str) -> None:
        inspect_attribute_record(attrs, AOT_OPCODES[node.op_type])
        records.append(
            LoweringRecord(
                source_index=index,
                op_type=node.op_type,
                opcode=AOT_OPCODES[node.op_type],
                phase=0 if index < analysis.phase_cut else 1,
                inputs=tuple(tensor_ids[name] for name in node.input),
                outputs=tuple(tensor_ids[name] for name in node.output),
                attributes=attrs,
                variant=variant,
            )
        )

    for index, node in enumerate(graph.node):
        op_type = node.op_type
        if op_type not in LOWERED_OPS:
            continue
        context = f"node {index} {node.name!r}"
        node_attrs = attributes(analysis.onnx, node)

        if op_type in LOWERED_F32_BINARY_OPS:
            require_arity(node, index, 2, 1)
            reject(node.domain != "" or node_attrs, f"{context}: binary contract changed")
            names = tuple(node.input) + tuple(node.output)
            dtypes = {analysis.dtypes[name] for name in names}
            if dtypes != {1}:
                # The prepared graph also contains 80 INT32 and one INT64 Add
                # used outside this f32 lane. They remain explicit, unlowered
                # graph operations and cannot be mistaken for a CPU f32 record.
                reject(
                    op_type != "Add" or len(dtypes) != 1 or next(iter(dtypes)) not in {6, 7},
                    f"{context}: mixed/unsupported binary dtype",
                )
                continue
            require_f32(names, context)
            lhs_rank, rhs_rank = (analysis.ranks[name] for name in node.input)
            output_rank = analysis.ranks[node.output[0]]
            _validate_binary_broadcast(
                analysis, node.input[0], node.input[1], node.output[0], context
            )
            reject(
                tensor_facts[node.output[0]].alias_of is not None,
                f"{context}: compute output cannot be a view",
            )
            append(
                index,
                node,
                binary_attribute(op_type, lhs_rank, rhs_rank, output_rank),
                f"r{lhs_rank},r{rhs_rank}->r{output_rank}:checked-broadcast",
            )
            continue

        if op_type in LOWERED_UNARY_OPS:
            require_arity(node, index, 1, 1)
            reject(node.domain != "", f"{context}: unary domain changed")
            require_f32(tuple(node.input) + tuple(node.output), context)
            input_rank = analysis.ranks[node.input[0]]
            output_rank = analysis.ranks[node.output[0]]
            reject(input_rank != output_rank, f"{context}: unary rank mismatch")
            _validate_same_numeric_shape(
                analysis, (node.input[0], node.output[0]), context
            )
            reject(
                tensor_facts[node.output[0]].alias_of is not None,
                f"{context}: compute output cannot be a view",
            )
            if op_type == "LeakyRelu":
                reject(set(node_attrs) != {"alpha"}, f"{context}: LeakyRelu attrs changed")
                alpha_bits = f32_bits(float(node_attrs["alpha"]))
                reject(
                    alpha_bits not in LEAKY_RELU_ALPHA_BITS,
                    f"{context}: LeakyRelu alpha rejected",
                )
                encoded = leaky_relu_attribute(alpha_bits, input_rank, output_rank)
                variant = f"r{input_rank}:alpha=0x{alpha_bits:08x}:checked-strided"
            else:
                reject(node_attrs, f"{context}: parameterless unary attrs changed")
                encoded = parameterless_unary_attribute(op_type)
                variant = f"r{input_rank}:checked-strided"
            append(index, node, encoded, variant)
            continue

        if op_type == "ReduceMean":
            require_arity(node, index, 2, 1)
            reject(
                node.domain != ""
                or node_attrs != {"keepdims": 1, "noop_with_empty_axes": 0},
                f"{context}: ReduceMean attributes changed",
            )
            require_f32((node.input[0], node.output[0]), context)
            reject(
                analysis.dtypes[node.input[1]] != 7
                or analysis.ranks[node.input[1]] != 1,
                f"{context}: ReduceMean axes tensor rejected",
            )
            axes = _small_int_initializer(analysis, node.input[1], context)
            reject(len(axes) != 1, f"{context}: ReduceMean requires exactly one axis")
            input_rank = analysis.ranks[node.input[0]]
            output_rank = analysis.ranks[node.output[0]]
            axis = axes[0]
            normalized_axis = _normalize_axes(axes, input_rank, context)[0]
            reject(output_rank != input_rank, f"{context}: ReduceMean output rank changed")
            input_shape = _declared_shape(analysis, node.input[0])
            output_shape = _declared_shape(analysis, node.output[0])
            if input_shape is not None and output_shape is not None:
                for dimension, (source, target) in enumerate(zip(input_shape, output_shape)):
                    if dimension == normalized_axis:
                        reject(
                            isinstance(target, int) and target != 1,
                            f"{context}: reduced dimension is not one",
                        )
                    else:
                        reject(
                            isinstance(source, int)
                            and isinstance(target, int)
                            and source != target,
                            f"{context}: non-reduced dimension changed",
                        )
            append(
                index,
                node,
                reduce_mean_attribute(axis, 1, 0, input_rank, output_rank),
                f"r{input_rank}:axis={axis}:keepdims=1:contiguous",
            )
            continue

        if op_type == "LayerNormalization":
            require_arity(node, index, 3, 1)
            reject(
                node.domain != ""
                or set(node_attrs) != {"axis", "epsilon", "stash_type"}
                or int(node_attrs["axis"]) != -1
                or int(node_attrs["stash_type"]) != 1,
                f"{context}: LayerNormalization attributes changed",
            )
            epsilon_bits = f32_bits(float(node_attrs["epsilon"]))
            reject(
                epsilon_bits not in LAYER_NORM_EPSILON_BITS,
                f"{context}: LayerNormalization epsilon rejected",
            )
            require_f32(tuple(node.input) + tuple(node.output), context)
            input_rank = analysis.ranks[node.input[0]]
            output_rank = analysis.ranks[node.output[0]]
            reject(
                input_rank == 0 or output_rank != input_rank,
                f"{context}: LayerNormalization rank rejected",
            )
            scale_dim = _validate_last_dimension_parameter(
                analysis, node.input[0], node.input[1], context
            )
            bias_dim = _validate_last_dimension_parameter(
                analysis, node.input[0], node.input[2], context
            )
            reject(scale_dim != bias_dim, f"{context}: scale/bias dimensions differ")
            _validate_same_numeric_shape(
                analysis, (node.input[0], node.output[0]), context
            )
            append(
                index,
                node,
                layer_norm_attribute(-1, epsilon_bits, 1, input_rank, output_rank, 1),
                f"r{input_rank}:axis=-1:epsilon=0x{epsilon_bits:08x}:width={scale_dim}",
            )
            continue

        if op_type == "Softmax":
            require_arity(node, index, 1, 1)
            reject(
                node.domain != "" or node_attrs != {"axis": -1},
                f"{context}: Softmax attributes changed",
            )
            require_f32(tuple(node.input) + tuple(node.output), context)
            input_rank = analysis.ranks[node.input[0]]
            output_rank = analysis.ranks[node.output[0]]
            reject(input_rank == 0 or input_rank != output_rank, f"{context}: Softmax rank")
            _validate_same_numeric_shape(
                analysis, (node.input[0], node.output[0]), context
            )
            append(
                index,
                node,
                softmax_attribute(-1, input_rank, output_rank),
                f"r{input_rank}:axis=-1:contiguous",
            )
            continue

        if op_type == "FastGelu":
            require_arity(node, index, 2, 1)
            reject(
                node.domain != "com.microsoft" or node_attrs,
                f"{context}: FastGelu contract changed",
            )
            require_f32(tuple(node.input) + tuple(node.output), context)
            input_rank = analysis.ranks[node.input[0]]
            output_rank = analysis.ranks[node.output[0]]
            reject(input_rank == 0 or input_rank != output_rank, f"{context}: FastGelu rank")
            bias_dim = _validate_last_dimension_parameter(
                analysis, node.input[0], node.input[1], context
            )
            _validate_same_numeric_shape(
                analysis, (node.input[0], node.output[0]), context
            )
            append(
                index,
                node,
                fast_gelu_attribute(1, input_rank, output_rank, 1),
                f"r{input_rank}:bias=1:width={bias_dim}:contiguous",
            )
            continue

        if op_type == "SkipLayerNormalization":
            require_arity(node, index, 4, 1)
            reject(
                node.domain != "com.microsoft" or set(node_attrs) != {"epsilon"},
                f"{context}: SkipLayerNormalization contract changed",
            )
            epsilon_bits = f32_bits(float(node_attrs["epsilon"]))
            reject(
                epsilon_bits != SKIP_LAYER_NORM_EPSILON_BITS,
                f"{context}: SkipLayerNormalization epsilon rejected",
            )
            require_f32(tuple(node.input) + tuple(node.output), context)
            input_rank = analysis.ranks[node.input[0]]
            output_rank = analysis.ranks[node.output[0]]
            reject(
                input_rank == 0
                or analysis.ranks[node.input[1]] != input_rank
                or output_rank != input_rank,
                f"{context}: SkipLayerNormalization rank rejected",
            )
            scale_dim = _validate_last_dimension_parameter(
                analysis, node.input[0], node.input[2], context
            )
            bias_dim = _validate_last_dimension_parameter(
                analysis, node.input[0], node.input[3], context
            )
            reject(scale_dim != bias_dim, f"{context}: scale/bias dimensions differ")
            _validate_same_numeric_shape(
                analysis, (node.input[0], node.input[1], node.output[0]), context
            )
            append(
                index,
                node,
                skip_layer_norm_attribute(epsilon_bits, input_rank, output_rank, 1),
                f"r{input_rank}:epsilon=0x{epsilon_bits:08x}:width={scale_dim}",
            )
            continue

        if op_type == "Transpose":
            require_arity(node, index, 1, 1)
            reject(
                node.domain != "" or set(node_attrs) != {"perm"},
                f"{context}: Transpose attributes changed",
            )
            permutation = tuple(int(axis) for axis in node_attrs["perm"])
            data, output = node.input[0], node.output[0]
            dtype = analysis.dtypes[data]
            input_rank = analysis.ranks[data]
            output_rank = analysis.ranks[output]
            reject(
                dtype not in {1, 7}
                or analysis.dtypes[output] != dtype
                or input_rank == 0
                or output_rank != input_rank
                or len(permutation) != input_rank
                or sorted(permutation) != list(range(input_rank)),
                f"{context}: Transpose dtype/rank/permutation rejected",
            )
            input_shape = _declared_shape(analysis, data)
            output_shape = _declared_shape(analysis, output)
            if input_shape is not None and output_shape is not None:
                for output_axis, input_axis in enumerate(permutation):
                    source = input_shape[input_axis]
                    target = output_shape[output_axis]
                    reject(
                        isinstance(source, int)
                        and isinstance(target, int)
                        and source != target,
                        f"{context}: Transpose output dimension mismatch",
                    )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: Transpose output cannot be a view",
            )
            append(
                index,
                node,
                transpose_attribute(permutation, input_rank, dtype),
                f"{DTYPE_NAMES[dtype]}:r{input_rank}:perm="
                + ",".join(map(str, permutation)),
            )
            continue

        if op_type == "Gather":
            require_arity(node, index, 2, 1)
            reject(
                node.domain != "" or set(node_attrs) != {"axis"},
                f"{context}: Gather attributes changed",
            )
            data, indices = node.input
            output = node.output[0]
            dtype = analysis.dtypes[data]
            data_rank = analysis.ranks[data]
            indices_rank = analysis.ranks[indices]
            output_rank = analysis.ranks[output]
            axis = int(node_attrs["axis"])
            normalized_axis = _normalize_axes((axis,), data_rank, context)[0]
            reject(
                dtype not in {1, 3, 7}
                or analysis.dtypes[indices] != 7
                or analysis.dtypes[output] != dtype
                or output_rank != data_rank - 1 + indices_rank
                or output_rank > 4,
                f"{context}: Gather dtype/rank rejected",
            )
            for name in (data, indices, output):
                _validate_declared_rank(analysis, name)
            data_shape = _declared_shape(analysis, data)
            indices_shape = _declared_shape(analysis, indices)
            output_shape = _declared_shape(analysis, output)
            if data_shape is not None and indices_shape is not None and output_shape is not None:
                expected = (
                    data_shape[:normalized_axis]
                    + indices_shape
                    + data_shape[normalized_axis + 1 :]
                )
                reject(len(expected) != len(output_shape), f"{context}: Gather output rank")
                for source, target in zip(expected, output_shape):
                    reject(
                        isinstance(source, int)
                        and isinstance(target, int)
                        and source != target,
                        f"{context}: Gather output dimension mismatch",
                    )
            static_indices = indices in analysis.initializers
            if static_indices:
                index_tensor = analysis.initializers[indices]
                element_count = 1
                for dim in index_tensor.dims:
                    element_count *= int(dim)
                reject(element_count > 4, f"{context}: Gather index fixture widened")
                index_values = tuple(
                    int(value) for value in initializer_values(analysis.onnx, index_tensor)
                )
                if (
                    data_shape is not None
                    and isinstance(data_shape[normalized_axis], int)
                ):
                    axis_len = int(data_shape[normalized_axis])
                    reject(
                        any(value < -axis_len or value >= axis_len for value in index_values),
                        f"{context}: Gather initializer index rejected",
                    )
            control_mode = (
                ATTRIBUTE_CONTROL_INITIALIZER
                if static_indices
                else ATTRIBUTE_CONTROL_DYNAMIC
            )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: Gather output cannot be a view",
            )
            append(
                index,
                node,
                gather_attribute(
                    axis, data_rank, indices_rank, output_rank, dtype, control_mode
                ),
                f"{DTYPE_NAMES[dtype]}:r{data_rank},indices-r{indices_rank}"
                f"->r{output_rank}:axis={axis}:"
                + ("static" if static_indices else "dynamic"),
            )
            continue

        if op_type == "Concat":
            reject(
                len(node.input) < 2
                or len(node.input) > 4
                or any(not name for name in node.input)
                or len(node.output) != 1
                or not node.output[0],
                f"{context}: Concat arity rejected",
            )
            reject(
                node.domain != "" or set(node_attrs) != {"axis"},
                f"{context}: Concat attributes changed",
            )
            output = node.output[0]
            dtype = analysis.dtypes[node.input[0]]
            rank = analysis.ranks[node.input[0]]
            output_rank = analysis.ranks[output]
            axis = int(node_attrs["axis"])
            normalized_axis = _normalize_axes((axis,), rank, context)[0]
            reject(
                dtype not in {1, 7}
                or any(analysis.dtypes[name] != dtype for name in node.input)
                or analysis.dtypes[output] != dtype
                or any(analysis.ranks[name] != rank for name in node.input)
                or output_rank != rank,
                f"{context}: Concat dtype/rank rejected",
            )
            input_shapes = [_declared_shape(analysis, name) for name in node.input]
            output_shape = _declared_shape(analysis, output)
            if output_shape is not None:
                for dimension in range(rank):
                    known_inputs = [
                        int(shape[dimension])
                        for shape in input_shapes
                        if shape is not None and isinstance(shape[dimension], int)
                    ]
                    target = output_shape[dimension]
                    if dimension == normalized_axis:
                        if len(known_inputs) == len(input_shapes) and isinstance(target, int):
                            reject(
                                sum(known_inputs) != target,
                                f"{context}: Concat axis dimension mismatch",
                            )
                    else:
                        reject(
                            len(set(known_inputs)) > 1
                            or (
                                known_inputs
                                and isinstance(target, int)
                                and target != known_inputs[0]
                            ),
                            f"{context}: Concat non-axis dimension mismatch",
                        )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: Concat output cannot be a view",
            )
            append(
                index,
                node,
                concat_attribute(axis, rank, output_rank, dtype, len(node.input)),
                f"{DTYPE_NAMES[dtype]}:r{rank}:axis={axis}:inputs={len(node.input)}",
            )
            continue

        if op_type == "Split":
            require_arity(node, index, 2, 2)
            reject(
                node.domain != "" or set(node_attrs) != {"axis"},
                f"{context}: Split attributes changed",
            )
            data, split_control = node.input
            axis = int(node_attrs["axis"])
            input_rank = analysis.ranks[data]
            normalized_axis = _normalize_axes((axis,), input_rank, context)[0]
            split_lengths = _small_int_initializer(analysis, split_control, context)
            reject(
                len(split_lengths) != 2
                or any(length <= 0 or length > 0xFFFF_FFFF for length in split_lengths)
                or analysis.dtypes[data] != 1
                or analysis.dtypes[split_control] != 7
                or any(analysis.dtypes[output] != 1 for output in node.output)
                or any(analysis.ranks[output] != input_rank for output in node.output),
                f"{context}: Split dtype/rank/length rejected",
            )
            input_shape = _declared_shape(analysis, data)
            output_shapes = [_declared_shape(analysis, output) for output in node.output]
            if input_shape is not None and isinstance(input_shape[normalized_axis], int):
                reject(
                    sum(split_lengths) != input_shape[normalized_axis],
                    f"{context}: Split lengths do not cover input axis",
                )
            for output_index, output_shape in enumerate(output_shapes):
                if output_shape is None:
                    continue
                for dimension, target in enumerate(output_shape):
                    expected = (
                        split_lengths[output_index]
                        if dimension == normalized_axis
                        else None if input_shape is None else input_shape[dimension]
                    )
                    reject(
                        isinstance(expected, int)
                        and isinstance(target, int)
                        and expected != target,
                        f"{context}: Split output dimension mismatch",
                    )
            reject(
                any(tensor_facts[output].alias_of is not None for output in node.output),
                f"{context}: Split outputs cannot be views",
            )
            append(
                index,
                node,
                split_attribute(
                    axis,
                    split_lengths[0],
                    split_lengths[1],
                    input_rank,
                    input_rank,
                    1,
                    2,
                ),
                f"FLOAT:r{input_rank}:axis={axis}:split="
                + ",".join(map(str, split_lengths)),
            )
            continue

        if op_type == "Expand":
            require_arity(node, index, 2, 1)
            reject(node.domain != "" or node_attrs, f"{context}: Expand attributes changed")
            data, shape_control = node.input
            output = node.output[0]
            dtype = analysis.dtypes[data]
            input_rank = analysis.ranks[data]
            output_rank = analysis.ranks[output]
            reject(
                dtype not in {1, 7}
                or analysis.dtypes[shape_control] != 7
                or analysis.ranks[shape_control] != 1
                or analysis.dtypes[output] != dtype
                or output_rank < input_rank
                or output_rank > 4,
                f"{context}: Expand dtype/rank rejected",
            )
            control_shape = _declared_shape(analysis, shape_control)
            reject(
                control_shape is not None
                and isinstance(control_shape[0], int)
                and control_shape[0] != output_rank,
                f"{context}: Expand control length mismatch",
            )
            if shape_control in analysis.initializers:
                target_dims = _small_int_initializer(analysis, shape_control, context)
                reject(
                    len(target_dims) != output_rank
                    or any(dim <= 0 or dim > 0x7FFF_FFFF for dim in target_dims),
                    f"{context}: Expand static target rejected",
                )
                control_mode = ATTRIBUTE_CONTROL_INITIALIZER
                producer_opcode = 0
            else:
                producer_index = analysis.producers.get(shape_control)
                reject(
                    producer_index is None
                    or graph.node[producer_index].op_type != "Where",
                    f"{context}: Expand dynamic target provenance changed",
                )
                target_dims = ()
                control_mode = ATTRIBUTE_CONTROL_DYNAMIC
                producer_opcode = AOT_OPCODES["Where"]
            input_shape = _declared_shape(analysis, data)
            output_shape = _declared_shape(analysis, output)
            if input_shape is not None and output_shape is not None:
                leading = output_rank - input_rank
                for input_axis, source in enumerate(input_shape):
                    target = output_shape[leading + input_axis]
                    reject(
                        isinstance(source, int)
                        and isinstance(target, int)
                        and source not in {1, target},
                        f"{context}: Expand broadcast mismatch",
                    )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: Expand output cannot be a view",
            )
            append(
                index,
                node,
                expand_attribute(
                    input_rank,
                    output_rank,
                    dtype,
                    control_mode,
                    1,
                    producer_opcode,
                    target_dims,
                ),
                f"{DTYPE_NAMES[dtype]}:r{input_rank}->r{output_rank}:"
                + ("static" if control_mode == ATTRIBUTE_CONTROL_INITIALIZER else "dynamic:Where"),
            )
            continue

        if op_type == "Shape":
            require_arity(node, index, 1, 1)
            reject(
                node.domain != "" or node_attrs != {"start": 0},
                f"{context}: Shape attributes changed",
            )
            data, output = node.input[0], node.output[0]
            input_rank = analysis.ranks[data]
            output_rank = analysis.ranks[output]
            reject(
                analysis.dtypes[data] not in {1, 7}
                or analysis.dtypes[output] != 7
                or input_rank == 0
                or input_rank > 4
                or output_rank != 1,
                f"{context}: Shape dtype/rank rejected",
            )
            output_shape = _declared_shape(analysis, output)
            reject(
                output_shape is not None
                and isinstance(output_shape[0], int)
                and output_shape[0] != input_rank,
                f"{context}: Shape result length mismatch",
            )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: Shape output cannot be a view",
            )
            append(
                index,
                node,
                shape_attribute(0, 0, 0, input_rank, output_rank),
                f"input-r{input_rank}:start=0:end=rank",
            )
            continue

        if op_type == "Slice":
            reject(
                len(node.input) < 3
                or len(node.input) > 5
                or any(not name for name in node.input)
                or len(node.output) != 1
                or not node.output[0],
                f"{context}: Slice arity rejected",
            )
            reject(node.domain != "" or node_attrs, f"{context}: Slice attributes changed")
            data, output = node.input[0], node.output[0]
            dtype = analysis.dtypes[data]
            input_rank = analysis.ranks[data]
            output_rank = analysis.ranks[output]
            reject(
                dtype not in {1, 7}
                or analysis.dtypes[output] != dtype
                or input_rank == 0
                or output_rank != input_rank,
                f"{context}: Slice data dtype/rank rejected",
            )
            axes_present = len(node.input) >= 4
            steps_present = len(node.input) == 5
            control_modes = [ATTRIBUTE_CONTROL_ABSENT] * 4
            control_values = [0] * 4
            producer_opcodes = [0] * 4
            for slot, control in enumerate(node.input[1:]):
                reject(
                    analysis.dtypes[control] != 7 or analysis.ranks[control] != 1,
                    f"{context}: Slice control dtype/rank rejected",
                )
                control_shape = _declared_shape(analysis, control)
                reject(
                    control_shape is not None
                    and isinstance(control_shape[0], int)
                    and control_shape[0] != 1,
                    f"{context}: Slice control must contain one value",
                )
                if control in analysis.initializers:
                    values = _small_int_initializer(analysis, control, context)
                    reject(len(values) != 1, f"{context}: Slice control widened")
                    control_modes[slot] = ATTRIBUTE_CONTROL_INITIALIZER
                    control_values[slot] = values[0]
                else:
                    producer_index = analysis.producers.get(control)
                    reject(
                        slot != 1
                        or producer_index is None
                        or graph.node[producer_index].op_type != "Unsqueeze",
                        f"{context}: Slice dynamic control provenance changed",
                    )
                    control_modes[slot] = ATTRIBUTE_CONTROL_DYNAMIC
                    producer_opcodes[slot] = AOT_OPCODES["Unsqueeze"]
            reject(
                control_modes[0] != ATTRIBUTE_CONTROL_INITIALIZER,
                f"{context}: Slice starts must be constant",
            )
            axis = control_values[2] if axes_present else 0
            normalized_axis = _normalize_axes((axis,), input_rank, context)[0]
            reject(
                steps_present
                and (
                    control_modes[3] != ATTRIBUTE_CONTROL_INITIALIZER
                    or control_values[3] != 1
                ),
                f"{context}: negative/non-unit Slice step rejected",
            )
            input_shape = _declared_shape(analysis, data)
            output_shape = _declared_shape(analysis, output)
            if input_shape is not None and output_shape is not None:
                for dimension, (source, target) in enumerate(zip(input_shape, output_shape)):
                    if dimension != normalized_axis:
                        reject(
                            isinstance(source, int)
                            and isinstance(target, int)
                            and source != target,
                            f"{context}: Slice non-axis dimension mismatch",
                        )
                    elif (
                        control_modes[1] == ATTRIBUTE_CONTROL_INITIALIZER
                        and isinstance(source, int)
                        and isinstance(target, int)
                    ):
                        start = control_values[0]
                        end = control_values[1]
                        normalized_start = (
                            max(0, min(source, start + source))
                            if start < 0
                            else min(source, start)
                        )
                        normalized_end = (
                            max(0, min(source, end + source))
                            if end < 0
                            else min(source, end)
                        )
                        reject(
                            max(0, normalized_end - normalized_start) != target,
                            f"{context}: Slice output dimension mismatch",
                        )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: Slice output cannot be a view",
            )
            provenance = []
            for slot, label in enumerate(("starts", "ends", "axes", "steps")):
                if control_modes[slot] == ATTRIBUTE_CONTROL_ABSENT:
                    provenance.append(f"{label}=default")
                elif control_modes[slot] == ATTRIBUTE_CONTROL_INITIALIZER:
                    provenance.append(f"{label}={control_values[slot]}")
                else:
                    provenance.append(f"{label}=dynamic:Unsqueeze")
            append(
                index,
                node,
                slice_attribute(
                    input_rank,
                    output_rank,
                    dtype,
                    len(node.input) - 1,
                    axes_present,
                    steps_present,
                    control_modes,
                    control_values,
                    producer_opcodes,
                ),
                f"{DTYPE_NAMES[dtype]}:r{input_rank}:" + ":".join(provenance),
            )
            continue

        if op_type == "Pad":
            require_arity(node, index, 2, 1)
            reject(
                node.domain != "" or node_attrs != {"mode": b"reflect"},
                f"{context}: non-reflect Pad rejected",
            )
            data, pads_control = node.input
            output = node.output[0]
            input_rank = analysis.ranks[data]
            output_rank = analysis.ranks[output]
            pads = _small_int_initializer(
                analysis, pads_control, context, max_count=8
            )
            reject(
                analysis.dtypes[data] != 1
                or analysis.dtypes[pads_control] != 7
                or analysis.dtypes[output] != 1
                or input_rank == 0
                or output_rank != input_rank
                or len(pads) != input_rank * 2
                or any(value < 0 or value > 0xFFFF_FFFF for value in pads),
                f"{context}: Pad dtype/rank/vector rejected",
            )
            input_shape = _declared_shape(analysis, data)
            output_shape = _declared_shape(analysis, output)
            if input_shape is not None:
                for dimension in range(input_rank):
                    before = pads[dimension]
                    after = pads[input_rank + dimension]
                    source = input_shape[dimension]
                    if isinstance(source, int):
                        reject(
                            (before != 0 and before >= source)
                            or (after != 0 and after >= source),
                            f"{context}: reflect Pad exceeds input dimension",
                        )
                        if output_shape is not None and isinstance(output_shape[dimension], int):
                            reject(
                                output_shape[dimension] != before + source + after,
                                f"{context}: Pad output dimension mismatch",
                            )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: Pad output cannot be a view",
            )
            append(
                index,
                node,
                pad_attribute(input_rank, output_rank, 1, pads),
                f"FLOAT:r{input_rank}:reflect:pads=" + ",".join(map(str, pads)),
            )
            continue

        if op_type == "NonZero":
            require_arity(node, index, 1, 1)
            reject(node.domain != "" or node_attrs, f"{context}: NonZero contract changed")
            data, output = node.input[0], node.output[0]
            input_rank = analysis.ranks[data]
            output_rank = analysis.ranks[output]
            reject(
                analysis.dtypes[data] != 9
                or analysis.dtypes[output] != 7
                or input_rank != 1
                or output_rank != 2,
                f"{context}: NonZero dtype/rank rejected",
            )
            _validate_declared_rank(analysis, data)
            _validate_declared_rank(analysis, output)
            output_shape = _declared_shape(analysis, output)
            reject(
                output_shape is not None
                and isinstance(output_shape[0], int)
                and output_shape[0] != input_rank,
                f"{context}: NonZero coordinate dimension mismatch",
            )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: NonZero output cannot be a view",
            )
            append(
                index,
                node,
                nonzero_attribute(input_rank, output_rank),
                "BOOL:r1->INT64:r2:row-major",
            )
            continue

        if op_type == "ScatterND":
            require_arity(node, index, 3, 1)
            reject(
                node.domain != "" or node_attrs != {"reduction": b"none"},
                f"{context}: ScatterND reduction contract changed",
            )
            data, indices, updates = node.input
            output = node.output[0]
            ranks = (
                analysis.ranks[data],
                analysis.ranks[indices],
                analysis.ranks[updates],
                analysis.ranks[output],
            )
            reject(
                (
                    analysis.dtypes[data],
                    analysis.dtypes[indices],
                    analysis.dtypes[updates],
                    analysis.dtypes[output],
                )
                != (1, 7, 1, 1)
                or ranks != (2, 3, 2, 2),
                f"{context}: ScatterND dtype/rank rejected",
            )
            for name in (data, indices, updates, output):
                _validate_declared_rank(analysis, name)
            indices_shape = _declared_shape(analysis, indices)
            reject(
                indices_shape is None
                or not isinstance(indices_shape[-1], int)
                or indices_shape[-1] != ranks[0],
                f"{context}: ScatterND index tuple length rejected",
            )
            _validate_same_numeric_shape(analysis, (data, output), context)
            updates_shape = _declared_shape(analysis, updates)
            if updates_shape is not None:
                for dimension, (index_dim, update_dim) in enumerate(
                    zip(indices_shape[:-1], updates_shape)
                ):
                    reject(
                        isinstance(index_dim, int)
                        and isinstance(update_dim, int)
                        and index_dim != update_dim,
                        f"{context}: ScatterND updates dimension {dimension} mismatch",
                    )

            data_producer = analysis.producers.get(data)
            indices_producer = analysis.producers.get(indices)
            updates_producer = analysis.producers.get(updates)
            reject(
                data_producer is None
                or graph.node[data_producer].op_type != "Squeeze"
                or indices_producer is None
                or graph.node[indices_producer].op_type != "Concat"
                or updates_producer is None
                or graph.node[updates_producer].op_type != "Reshape",
                f"{context}: ScatterND tail provenance changed",
            )
            assert indices_producer is not None and updates_producer is not None
            indices_node = graph.node[indices_producer]
            reject(
                indices_node.domain != ""
                or attributes(analysis.onnx, indices_node) != {"axis": -1}
                or len(indices_node.input) != 2,
                f"{context}: ScatterND index constructor changed",
            )

            def nonzero_ancestors(tensor_name: str) -> set[int]:
                pending = [tensor_name]
                visited: set[str] = set()
                found: set[int] = set()
                while pending:
                    name = pending.pop()
                    if name in visited:
                        continue
                    visited.add(name)
                    producer = analysis.producers.get(name)
                    if producer is None:
                        continue
                    producer_node = graph.node[producer]
                    if producer_node.op_type == "NonZero":
                        found.add(producer)
                    else:
                        pending.extend(input_name for input_name in producer_node.input if input_name)
                return found

            branch_nonzeros = [
                nonzero_ancestors(input_name) for input_name in indices_node.input
            ]
            reject(
                len(branch_nonzeros) != 2
                or len(branch_nonzeros[0]) != 1
                or branch_nonzeros[0] != branch_nonzeros[1],
                f"{context}: ScatterND indices are not derived from one NonZero",
            )
            updates_node = graph.node[updates_producer]
            reject(
                updates_node.domain != ""
                or attributes(analysis.onnx, updates_node) not in ({}, {"allowzero": 0})
                or len(updates_node.input) != 2,
                f"{context}: ScatterND updates Reshape changed",
            )
            shape_producer = analysis.producers.get(updates_node.input[1])
            reject(
                shape_producer is None
                or graph.node[shape_producer].op_type != "Concat"
                or attributes(analysis.onnx, graph.node[shape_producer]) != {"axis": 0},
                f"{context}: ScatterND updates shape provenance changed",
            )
            reject(
                tensor_facts[output].alias_of is not None,
                f"{context}: ScatterND output cannot be a view",
            )
            append(
                index,
                node,
                scatter_nd_attribute(2, 3, 2, 2, 1, 2),
                "FLOAT:r2:indices-r3:updates-r2:tuple=2:reduction=none:ordered-unique",
            )
            continue

        assert op_type in LOWERED_VIEW_OPS
        require_arity(node, index, 2, 1)
        reject(node.domain != "", f"{context}: view domain changed")
        data, control = node.input
        output = node.output[0]
        dtype = analysis.dtypes[data]
        reject(
            dtype not in {1, 6, 7}
            or analysis.dtypes[output] != dtype
            or analysis.dtypes[control] != 7
            or analysis.ranks[control] != 1,
            f"{context}: view dtype/controller rejected",
        )
        input_rank = analysis.ranks[data]
        output_rank = analysis.ranks[output]
        reject(input_rank > 4 or output_rank > 4, f"{context}: view rank exceeds four")
        _validate_declared_rank(analysis, data)
        _validate_declared_rank(analysis, control)
        _validate_declared_rank(analysis, output)
        input_root = tensor_facts[data].alias_of
        if input_root is None:
            input_root = tensor_facts[data].tensor_id
        reject(
            tensor_facts[output].alias_of != input_root,
            f"{context}: view alias root changed",
        )
        static_control = control in analysis.initializers
        parameters: tuple[int, ...]
        if op_type == "Reshape":
            reject(
                node_attrs not in ({}, {"allowzero": 0}),
                f"{context}: Reshape attributes changed",
            )
            reject(output_rank == 0, f"{context}: scalar Reshape not admitted")
            if static_control:
                parameters = _small_int_initializer(analysis, control, context)
                reject(
                    len(parameters) != output_rank,
                    f"{context}: Reshape controller/output rank mismatch",
                )
                _validate_reshape_static_shape(
                    analysis, data, output, parameters, context
                )
                control_variant = "static:" + ",".join(map(str, parameters))
            else:
                producer_index = analysis.producers.get(control)
                reject(
                    producer_index is None
                    or graph.node[producer_index].op_type != "Concat",
                    f"{context}: dynamic Reshape controller is not Concat",
                )
                control_shape = _declared_shape(analysis, control)
                reject(
                    control_shape is not None
                    and isinstance(control_shape[0], int)
                    and control_shape[0] != output_rank,
                    f"{context}: dynamic Reshape controller length mismatch",
                )
                parameters = ()
                control_variant = "dynamic:Concat"
        else:
            reject(node_attrs, f"{context}: {op_type} attributes changed")
            reject(not static_control, f"{context}: {op_type} axes must be constant")
            parameters = _small_int_initializer(analysis, control, context)
            if op_type == "Unsqueeze":
                reject(
                    output_rank != input_rank + len(parameters),
                    f"{context}: Unsqueeze rank mismatch",
                )
                _normalize_axes(parameters, output_rank, context)
            else:
                reject(
                    input_rank != output_rank + len(parameters),
                    f"{context}: Squeeze rank mismatch",
                )
                normalized = _normalize_axes(parameters, input_rank, context)
                data_shape = _declared_shape(analysis, data)
                if data_shape is not None:
                    reject(
                        any(
                            isinstance(data_shape[axis], int)
                            and data_shape[axis] != 1
                            for axis in normalized
                        ),
                        f"{context}: Squeeze axis is not unit-sized",
                    )
            control_variant = "static:" + ",".join(map(str, parameters))
        append(
            index,
            node,
            view_attribute(
                op_type,
                input_rank,
                output_rank,
                dtype,
                static_control=static_control,
                parameters=parameters,
            ),
            f"{DTYPE_NAMES[dtype]}:r{input_rank}->r{output_rank}:{control_variant}:alias",
        )

    return records


def infer_output_ranks(
    onnx: Any,
    graph: Any,
    initializers: Mapping[str, Any],
    declared_ranks: Mapping[str, int | None],
) -> dict[str, int]:
    """Prove ranks for every material tensor using ONNX operator semantics."""

    ranks: dict[str, int] = {
        name: rank for name, rank in declared_ranks.items() if rank is not None
    }
    # Rank-one INT64 tensors used as shape controllers carry a second static
    # descriptor: their element count. Propagating that count through the
    # iSTFT Shape/Slice/Concat tail proves the final dynamic Reshape rank as
    # two. Falling back to its data rank would incorrectly classify the
    # ScatterND updates as rank three.
    shape_vector_lengths: dict[str, int] = {
        name: int(tensor.dims[0])
        for name, tensor in initializers.items()
        if int(tensor.data_type) == 7 and len(tensor.dims) == 1
    }

    unary = {
        "Atan",
        "Cast",
        "Cos",
        "CumSum",
        "DequantizeLinear",
        "Exp",
        "FastGelu",
        "Floor",
        "LeakyRelu",
        "Pad",
        "Pow",
        "Resize",
        "Round",
        "Sigmoid",
        "Sin",
        "Slice",
        "Softmax",
        "Sqrt",
        "Tanh",
    }
    broadcast = {
        "Add",
        "And",
        "Div",
        "Equal",
        "Greater",
        "GreaterOrEqual",
        "Less",
        "Mul",
        "Sub",
        "Where",
    }

    def input_rank(node: Any, index: int = 0) -> int | None:
        if index >= len(node.input) or not node.input[index]:
            return None
        return ranks.get(node.input[index])

    for index, node in enumerate(graph.node):
        attrs = attributes(onnx, node)
        first = input_rank(node)

        if node.op_type == "Shape" and first is not None and node.output:
            start = int(attrs.get("start", 0))
            end = int(attrs.get("end", 2**63 - 1))
            normalized_start, normalized_end, step = slice(start, end).indices(first)
            shape_vector_lengths[node.output[0]] = len(
                range(normalized_start, normalized_end, step)
            )
        elif node.op_type == "Slice" and node.output and node.input:
            source_length = shape_vector_lengths.get(node.input[0])
            starts = axes_from_input(onnx, node, initializers, 1)
            ends = axes_from_input(onnx, node, initializers, 2)
            axes = axes_from_input(onnx, node, initializers, 3)
            steps = axes_from_input(onnx, node, initializers, 4)
            if (
                source_length is not None
                and starts is not None
                and ends is not None
                and len(starts) == len(ends) == 1
                and (axes is None or axes in {(0,), (-1,)})
                and (steps is None or steps == (1,))
            ):
                normalized_start, normalized_end, step = slice(
                    starts[0], ends[0], 1
                ).indices(source_length)
                shape_vector_lengths[node.output[0]] = len(
                    range(normalized_start, normalized_end, step)
                )
        elif node.op_type == "Concat" and node.output:
            axis = int(attrs.get("axis", 0))
            lengths = [shape_vector_lengths.get(name) for name in node.input]
            if axis in {0, -1} and lengths and all(length is not None for length in lengths):
                shape_vector_lengths[node.output[0]] = sum(
                    int(length) for length in lengths if length is not None
                )

        missing = [name for name in node.output if name and name not in ranks]
        if not missing:
            continue
        inferred: list[int | None]
        if node.op_type in unary:
            inferred = [first] * len(node.output)
        elif node.op_type in broadcast:
            present = [ranks[name] for name in node.input if name in ranks]
            inferred = [max(present) if present else None] * len(node.output)
        elif node.op_type == "DynamicQuantizeLinear":
            inferred = [first, 0, 0]
        elif node.op_type in {"Conv", "ConvInteger", "ConvTranspose"}:
            inferred = [first] * len(node.output)
        elif node.op_type in {"LayerNormalization", "SkipLayerNormalization"}:
            inferred = [first] * len(node.output)
        elif node.op_type == "Shape":
            inferred = [1]
        elif node.op_type == "Range":
            inferred = [1]
        elif node.op_type == "NonZero":
            inferred = [2]
        elif node.op_type == "ScatterND":
            inferred = [first]
        elif node.op_type in {"Concat", "Split"}:
            inferred = [first] * len(node.output)
        elif node.op_type == "Transpose":
            inferred = [first]
        elif node.op_type == "Gather":
            data_rank = input_rank(node, 0)
            indices_rank = input_rank(node, 1)
            inferred = [
                None
                if data_rank is None or indices_rank is None
                else data_rank + indices_rank - 1
            ]
        elif node.op_type in {"MatMul", "MatMulInteger"}:
            left, right = input_rank(node, 0), input_rank(node, 1)
            if left is None or right is None:
                result = None
            elif left == 1 and right == 1:
                result = 0
            elif left == 1:
                result = right - 1
            elif right == 1:
                result = left - 1
            else:
                result = max(left, right)
            inferred = [result]
        elif node.op_type in {"ReduceMean", "ReduceSum"}:
            keepdims = int(attrs.get("keepdims", 1))
            if first is None or keepdims:
                result = first
            else:
                axes = axes_from_input(onnx, node, initializers, 1)
                if axes is None and "axes" in attrs:
                    axes = tuple(int(axis) for axis in attrs["axes"])
                result = None if axes is None else first - len(set(axes))
            inferred = [result]
        elif node.op_type == "Unsqueeze":
            axes = axes_from_input(onnx, node, initializers, 1)
            if axes is None and "axes" in attrs:
                axes = tuple(int(axis) for axis in attrs["axes"])
            inferred = [None if first is None or axes is None else first + len(axes)]
        elif node.op_type == "Squeeze":
            axes = axes_from_input(onnx, node, initializers, 1)
            if axes is None and "axes" in attrs:
                axes = tuple(int(axis) for axis in attrs["axes"])
            inferred = [None if first is None or axes is None else first - len(axes)]
        elif node.op_type == "Reshape":
            shape_tensor = initializers.get(node.input[1]) if len(node.input) > 1 else None
            if shape_tensor is not None:
                result = len(initializer_values(onnx, shape_tensor))
            else:
                result = shape_vector_lengths.get(node.input[1])
            inferred = [result]
        elif node.op_type == "Expand":
            target_rank = (
                shape_vector_lengths.get(node.input[1]) if len(node.input) > 1 else None
            )
            inferred = [ranks.get(node.output[0], target_rank or first)]
        elif node.op_type == "ConstantOfShape":
            inferred = [ranks.get(node.output[0])]
        elif node.op_type == "LSTM":
            inferred = [4, 3, 3][: len(node.output)]
        elif node.op_type == "STFT":
            inferred = [None if first is None else first + 2]
        else:
            raise CompileError(
                f"node {index} {node.name!r}: no rank rule for {node_key(node)}"
            )

        reject(len(inferred) != len(node.output), f"node {index}: internal rank arity")
        for output, rank in zip(node.output, inferred):
            if not output or output in ranks:
                continue
            reject(rank is None, f"node {index} {node.name!r}: output rank is unproved")
            reject(rank < 0 or rank > 4, f"node {index} {node.name!r}: rank {rank} rejected")
            ranks[output] = rank

    missing = sorted(
        name for node in graph.node for name in node.output if name and name not in ranks
    )
    reject(bool(missing), f"unproved tensor ranks: {missing[:4]}")
    reject(any(rank > 4 for rank in ranks.values()), "tensor rank exceeds four")
    return ranks


def build_indexes(graph: Any) -> tuple[dict[str, int], dict[str, list[int]]]:
    producers: dict[str, int] = {}
    consumers: dict[str, list[int]] = defaultdict(list)
    input_names = [value.name for value in graph.input]
    initializer_names = [tensor.name for tensor in graph.initializer]
    reject(len(input_names) != len(set(input_names)), "duplicate graph input name")
    reject(
        len(initializer_names) != len(set(initializer_names)),
        "duplicate initializer name",
    )
    reject(
        bool(set(input_names) & set(initializer_names)),
        "initializer aliases a graph input",
    )
    known = set(input_names) | set(initializer_names)
    node_names: set[str] = set()
    for index, node in enumerate(graph.node):
        reject(not node.name, f"node {index}: empty name")
        reject(node.name in node_names, f"node {index}: duplicate node name {node.name!r}")
        node_names.add(node.name)
        for name in node.input:
            if not name:
                continue
            reject(name not in known, f"node {index} {node.name!r}: unresolved input {name!r}")
            consumers[name].append(index)
        for name in node.output:
            if not name:
                continue
            reject(name in known, f"node {index} {node.name!r}: duplicate value {name!r}")
            producers[name] = index
            known.add(name)
    for output in graph.output:
        reject(output.name not in known, f"unresolved graph output {output.name!r}")
    return producers, dict(consumers)


def infer_dtypes(
    graph: Any,
    value_infos: Mapping[str, Any],
    initializers: Mapping[str, Any],
) -> dict[str, int]:
    dtypes = {
        name: int(value.type.tensor_type.elem_type) for name, value in value_infos.items()
    }
    dtypes.update({name: int(tensor.data_type) for name, tensor in initializers.items()})
    passthrough = {"Transpose", "Reshape", "Squeeze", "Unsqueeze", "Identity"}
    for index, node in enumerate(graph.node):
        for output in node.output:
            if not output or output in dtypes:
                continue
            if node.op_type in passthrough and node.input[0] in dtypes:
                dtypes[output] = dtypes[node.input[0]]
            else:
                raise CompileError(
                    f"node {index} {node.name!r}: output dtype for {output!r} is unproved"
                )
    unsupported = sorted(
        (name, dtype) for name, dtype in dtypes.items() if dtype not in SUPPORTED_DTYPES
    )
    reject(bool(unsupported), f"unsupported tensor dtype: {unsupported[:4]}")
    return dtypes


def validate_pinned_model(
    onnx: Any, model: Any, model_bytes: int, model_sha256: str
) -> None:
    reject(model_bytes != PINNED_MODEL_BYTES, f"source bytes {model_bytes} do not match pin")
    reject(model_sha256 != PINNED_MODEL_SHA256, "source model SHA-256 does not match pin")
    reject(int(model.ir_version) != PINNED_IR_VERSION, "ONNX IR version changed")
    reject(
        (model.producer_name, model.producer_version) != PINNED_PRODUCER,
        "ONNX producer changed",
    )
    reject(model.domain != "" or model.model_version != 0, "model identity fields changed")
    reject(model.graph.name != PINNED_GRAPH_NAME, "graph name changed")
    reject(
        tuple((item.domain, int(item.version)) for item in model.opset_import)
        != PINNED_OPSETS,
        "opset imports changed",
    )
    reject(
        {item.key: item.value for item in model.metadata_props} != PINNED_METADATA,
        "RTen bridge metadata changed",
    )
    reject(bool(model.functions), "local ONNX functions are unsupported")
    reject(bool(model.graph.sparse_initializer), "sparse initializers are unsupported")
    reject(
        bool(model.graph.quantization_annotation), "quantization annotations are unsupported"
    )
    reject(
        len(model.graph.node) != 3_615
        or len(model.graph.initializer) != 762
        or len(model.graph.input) != 3
        or len(model.graph.output) != 1,
        "graph cardinality changed",
    )
    actual_counts = Counter(node.op_type for node in model.graph.node)
    reject(dict(actual_counts) != PINNED_NODE_COUNTS, "operator inventory changed")
    domains = Counter(node_domain(node) for node in model.graph.node)
    reject(dict(domains) != PINNED_DOMAIN_COUNTS, "operator domain inventory changed")
    allowed_domain_ops = {
        ("com.microsoft", "FastGelu"),
        ("com.microsoft", "SkipLayerNormalization"),
    }
    for index, node in enumerate(model.graph.node):
        if node.domain:
            reject(
                (node.domain, node.op_type) not in allowed_domain_ops,
                f"node {index}: unsupported domain operator {node_key(node)}",
            )
    onnx.checker.check_model(model)


def sole_consumer(
    graph: Any,
    consumers: Mapping[str, list[int]],
    tensor: str,
    op_type: str | None = None,
) -> tuple[int, Any]:
    uses = consumers.get(tensor, [])
    reject(len(uses) != 1, f"quant tensor {tensor!r}: expected one consumer, got {uses}")
    index = uses[0]
    node = graph.node[index]
    reject(op_type is not None and node.op_type != op_type, f"node {index}: expected {op_type}")
    return index, node


def other_input(node: Any, tensor: str) -> str:
    reject(len(node.input) != 2, f"node {node.name!r}: expected binary input")
    if node.input[0] == tensor:
        return node.input[1]
    if node.input[1] == tensor:
        return node.input[0]
    raise CompileError(f"node {node.name!r}: tensor {tensor!r} is not an input")


def recognize_scale(
    graph: Any,
    producers: Mapping[str, int],
    initializers: Mapping[str, Any],
    dql: Any,
    scale_tensor: str,
) -> int:
    scale_index = producers.get(scale_tensor)
    reject(scale_index is None, f"quant scale {scale_tensor!r}: no producer")
    scale = graph.node[scale_index]
    reject(scale.op_type != "Mul", f"node {scale_index}: quant scale is not Mul")
    reject(dql.output[1] not in scale.input, f"node {scale_index}: activation scale missing")
    weight_scale = other_input(scale, dql.output[1])
    reject(weight_scale not in initializers, f"node {scale_index}: weight scale is not constant")
    weight_scale_tensor = initializers[weight_scale]
    reject(
        int(weight_scale_tensor.data_type) != 1
        or len(weight_scale_tensor.dims) > 1
        or (weight_scale_tensor.dims and int(weight_scale_tensor.dims[0]) == 0),
        f"node {scale_index}: weight scale must be scalar/per-output FLOAT",
    )
    return scale_index


def recognize_quant_fusions(
    onnx: Any,
    graph: Any,
    producers: Mapping[str, int],
    consumers: Mapping[str, list[int]],
    initializers: Mapping[str, Any],
    dtypes: Mapping[str, int],
) -> list[QuantFusion]:
    fusions: list[QuantFusion] = []
    for kernel_index, kernel in enumerate(graph.node):
        if kernel.op_type not in {"MatMulInteger", "ConvInteger"}:
            continue
        reject(kernel.domain != "", f"node {kernel_index}: integer kernel domain changed")
        reject(
            len(kernel.input) != 4 or len(kernel.output) != 1,
            f"node {kernel_index}: integer kernel arity changed",
        )
        activation, weight, activation_zero, weight_zero = kernel.input
        dql_index = producers.get(activation)
        reject(dql_index is None, f"node {kernel_index}: activation is not produced")
        dql = graph.node[dql_index]
        reject(
            dql.op_type != "DynamicQuantizeLinear" or dql.domain != "",
            f"node {kernel_index}: activation is not DynamicQuantizeLinear",
        )
        reject(
            len(dql.output) != 3
            or activation != dql.output[0]
            or activation_zero != dql.output[2],
            f"node {kernel_index}: activation quant tuple changed",
        )
        reject(weight not in initializers, f"node {kernel_index}: weight is not constant")
        reject(weight_zero not in initializers, f"node {kernel_index}: weight zero is not constant")
        reject(
            dtypes[activation] != 2 or dtypes[activation_zero] != 2,
            f"node {kernel_index}: activation quant dtype must be UINT8",
        )
        reject(
            dtypes[weight] not in {2, 3} or dtypes[weight_zero] != dtypes[weight],
            f"node {kernel_index}: weight quant dtype mismatch",
        )
        reject(dtypes[kernel.output[0]] != 6, f"node {kernel_index}: accumulator is not INT32")

        integer_result = kernel.output[0]
        int32_bias = False
        quantized_bias_scale: str | None = None
        next_index, next_node = sole_consumer(graph, consumers, integer_result)
        if next_node.op_type == "Add":
            int32_bias = True
            bias_reshape = other_input(next_node, integer_result)
            reshape_index = producers.get(bias_reshape)
            reject(reshape_index is None, f"node {kernel_index}: integer bias has no producer")
            reshape = graph.node[reshape_index]
            reject(
                reshape.op_type != "Reshape",
                f"node {kernel_index}: integer bias is not reshaped",
            )
            bias_cast_index = producers.get(reshape.input[0])
            reject(bias_cast_index is None, f"node {kernel_index}: bias Cast missing")
            bias_cast = graph.node[bias_cast_index]
            reject(bias_cast.op_type != "Cast", f"node {kernel_index}: bias Cast changed")
            bias_floor_index = producers.get(bias_cast.input[0])
            reject(bias_floor_index is None, f"node {kernel_index}: bias Floor missing")
            bias_floor = graph.node[bias_floor_index]
            reject(bias_floor.op_type != "Floor", f"node {kernel_index}: bias Floor changed")
            bias_div_index = producers.get(bias_floor.input[0])
            reject(bias_div_index is None, f"node {kernel_index}: bias Div missing")
            bias_div = graph.node[bias_div_index]
            reject(bias_div.op_type != "Div", f"node {kernel_index}: bias Div changed")
            bias_constants = [name for name in bias_div.input if name in initializers]
            reject(len(bias_constants) != 1, f"node {kernel_index}: quantized bias source changed")
            bias_source = initializers[bias_constants[0]]
            quantized_bias_scale = other_input(bias_div, bias_constants[0])
            reject(
                int(bias_source.data_type) != 1
                or quantized_bias_scale not in producers,
                f"node {kernel_index}: quantized bias scale changed",
            )
            reject(
                int(attributes(onnx, bias_cast).get("to", -1)) != 6
                or dtypes[bias_cast.output[0]] != 6
                or dtypes[reshape.output[0]] != 6
                or dtypes[next_node.output[0]] != 6,
                f"node {kernel_index}: quantized bias is not INT32",
            )
            integer_result = next_node.output[0]
            next_index, next_node = sole_consumer(graph, consumers, integer_result, "Cast")
        else:
            reject(next_node.op_type != "Cast", f"node {kernel_index}: accumulator cast changed")

        cast_index, cast = next_index, next_node
        cast_attrs = attributes(onnx, cast)
        reject(
            int(cast_attrs.get("to", -1)) != 1,
            f"node {cast_index}: accumulator cast is not FLOAT",
        )
        mul_index, dequant_mul = sole_consumer(graph, consumers, cast.output[0], "Mul")
        scale_tensor = other_input(dequant_mul, cast.output[0])
        scale_index = recognize_scale(
            graph, producers, initializers, dql, scale_tensor
        )
        reject(
            quantized_bias_scale is not None and quantized_bias_scale != scale_tensor,
            f"node {kernel_index}: bias and output scales differ",
        )

        result = dequant_mul.output[0]
        float_bias = False
        result_uses = consumers.get(result, [])
        if len(result_uses) == 1:
            possible_add = graph.node[result_uses[0]]
            if possible_add.op_type == "Add":
                bias = other_input(possible_add, result)
                if bias in initializers and dtypes[bias] == 1:
                    float_bias = True
                    result = possible_add.output[0]

        fusions.append(
            QuantFusion(
                kind="qgemm" if kernel.op_type == "MatMulInteger" else "qconv1d",
                kernel_index=kernel_index,
                kernel_name=kernel.name,
                dynamic_quant_index=dql_index,
                scale_index=scale_index,
                cast_index=cast_index,
                dequant_mul_index=mul_index,
                int32_bias=int32_bias,
                float_bias=float_bias,
                result_tensor=result,
            )
        )

    expected = sum(1 for node in graph.node if node.op_type in {"MatMulInteger", "ConvInteger"})
    reject(len(fusions) != expected, "not every integer kernel has a recognized quant chain")
    return fusions


def validate_frame_phase(
    onnx: Any,
    graph: Any,
    producers: Mapping[str, int],
    consumers: Mapping[str, list[int]],
    initializers: Mapping[str, Any],
    dtypes: Mapping[str, int],
    ranks: Mapping[str, int],
) -> int:
    reject(len(graph.node) <= FRAME_RANGE_NODE_INDEX, "graph is too short for frame resolver")
    resolver = graph.node[FRAME_COUNT_NODE_INDEX]
    reject(
        resolver.name != FRAME_COUNT_NODE_NAME
        or resolver.op_type != "Gather"
        or list(resolver.output) != [FRAME_COUNT_TENSOR],
        "frame-count resolver identity changed",
    )
    reject(
        dtypes[FRAME_COUNT_TENSOR] != 7 or ranks[FRAME_COUNT_TENSOR] != 0,
        "frame count must be INT64 scalar",
    )
    reject(
        consumers.get(FRAME_COUNT_TENSOR) != [FRAME_RANGE_NODE_INDEX],
        "frame count consumer changed",
    )

    expected_chain = {
        1_738: ("Sigmoid", "/encoder/predictor/Sigmoid"),
        1_739: ("ReduceSum", "/encoder/predictor/ReduceSum"),
        1_740: ("Div", "/encoder/Div"),
        1_741: ("Round", "/encoder/Round"),
        1_742: ("Clip", "/encoder/Clip"),
        1_743: ("Cast", "/encoder/Cast"),
        1_744: ("Gather", "/encoder/Gather"),
        1_745: ("CumSum", "/encoder/CumSum"),
        1_746: ("Gather", "/encoder/Gather_1"),
        1_749: ("Range", "/encoder/Range"),
    }
    for index, identity in expected_chain.items():
        node = graph.node[index]
        reject((node.op_type, node.name) != identity, f"frame resolver node {index} changed")

    reduce = graph.node[1_739]
    reject(
        axes_from_input(onnx, reduce, initializers, 1) != (-1,)
        or int(attributes(onnx, reduce).get("keepdims", 1)) != 0,
        "duration ReduceSum contract changed",
    )
    reject(graph.node[1_740].input[1] != "speed", "duration speed input changed")
    clip = graph.node[1_742]
    clip_min = initializers.get(clip.input[1])
    reject(
        clip_min is None
        or initializer_values(onnx, clip_min) != (1.0,)
        or len(clip.input) < 3
        or clip.input[2] != "",
        "duration Clip(min=1,max=none) contract changed",
    )
    reject(int(attributes(onnx, graph.node[1_743]).get("to", -1)) != 7, "duration Cast changed")
    reject(
        axes_from_input(onnx, graph.node[1_745], initializers, 1) != (0,)
        or int(attributes(onnx, graph.node[1_745]).get("exclusive", 0)) != 0
        or int(attributes(onnx, graph.node[1_745]).get("reverse", 0)) != 0,
        "duration CumSum contract changed",
    )
    batch_index = initializer_values(onnx, initializers[graph.node[1_744].input[1]])
    last_index = initializer_values(onnx, initializers[resolver.input[1]])
    reject(batch_index != (0,) or last_index != (-1,), "duration Gather indices changed")
    frame_range = graph.node[FRAME_RANGE_NODE_INDEX]
    reject(
        frame_range.input[1] != FRAME_COUNT_TENSOR
        or initializer_values(onnx, initializers[frame_range.input[0]]) != (0,)
        or initializer_values(onnx, initializers[frame_range.input[2]]) != (1,),
        "frame Range(0,F,1) contract changed",
    )

    descendants: set[int] = set()
    pending = list(consumers.get(FRAME_COUNT_TENSOR, []))
    while pending:
        index = pending.pop()
        if index in descendants:
            continue
        descendants.add(index)
        for output in graph.node[index].output:
            pending.extend(consumers.get(output, []))
    phase_one_nodes = set(range(PHASE_ONE_RAW_START, len(graph.node)))
    companions = phase_one_nodes - descendants
    reject(len(descendants & phase_one_nodes) != 1_862, "frame descendant closure changed")
    reject(
        companions != {1_747, 1_748, 1_750, 1_752, 1_755, 1_759},
        f"phase-one alignment companion set changed: {sorted(companions)}",
    )

    # Topological construction already proves the backward half.  State it
    # explicitly here so a future scheduler cannot silently cross the barrier.
    for index in range(PHASE_ONE_RAW_START, len(graph.node)):
        for output in graph.node[index].output:
            for consumer in consumers.get(output, []):
                reject(consumer < PHASE_ONE_RAW_START, "phase-one value feeds phase zero")

    quant_indices = {
        index
        for index, node in enumerate(graph.node)
        if node.op_type in {"DynamicQuantizeLinear", "MatMulInteger", "ConvInteger"}
    }
    reject(
        any(
            index < PHASE_ONE_RAW_START <= consumer
            for index in quant_indices
            for output in graph.node[index].output
            for consumer in consumers.get(output, [])
            if graph.node[consumer].op_type in {"Cast", "Mul"}
        ),
        "quant lowering crosses the phase cut",
    )
    return PHASE_ONE_RAW_START


def assign_tensors(
    graph: Any,
    producers: Mapping[str, int],
    consumers: Mapping[str, list[int]],
    initializers: Mapping[str, Any],
    value_infos: Mapping[str, Any],
    dtypes: Mapping[str, int],
    ranks: Mapping[str, int],
    phase_cut: int,
) -> list[TensorFact]:
    names: list[str] = []
    names.extend(value.name for value in graph.input)
    names.extend(tensor.name for tensor in graph.initializer)
    names.extend(output for node in graph.node for output in node.output if output)
    reject(len(names) != len(set(names)), "tensor ID source order contains duplicates")
    ids = {name: index for index, name in enumerate(names)}
    outputs = {value.name for value in graph.output}
    inputs = {value.name for value in graph.input}

    alias_roots: dict[str, str] = {}
    for node in graph.node:
        if node.op_type not in VIEW_OPS or not node.output:
            continue
        source = node.input[0]
        root = alias_roots.get(source, source)
        for output in node.output:
            if output:
                alias_roots[output] = root

    last_use: dict[str, int] = {}
    for name in names:
        uses = consumers.get(name, [])
        end = max(uses) + 1 if uses else (producers.get(name, -1) + 1)
        if name in outputs:
            end = len(graph.node) + 1
        last_use[name] = max(end, 1)
    for alias, root in alias_roots.items():
        last_use[root] = max(last_use[root], last_use[alias])

    facts: list[TensorFact] = []
    for name in names:
        producer = producers.get(name)
        start = 0 if producer is None else producer
        end = last_use[name]
        if producer is not None and producer >= phase_cut:
            phase = 1
        elif end > phase_cut:
            phase = 2
        else:
            phase = 0
        value_info = value_infos.get(name)
        declared_shape = tensor_shape(value_info) if value_info is not None else None
        if name in initializers:
            declared_shape = tuple(int(dim) for dim in initializers[name].dims)
        root = alias_roots.get(name)
        facts.append(
            TensorFact(
                tensor_id=ids[name],
                name=name,
                dtype=dtypes[name],
                rank=ranks[name],
                declared_shape=declared_shape,
                producer=producer,
                initializer=name in initializers,
                graph_input=name in inputs,
                graph_output=name in outputs,
                alias_of=None if root is None else ids[root],
                live_start=start,
                live_end=end,
                phase=phase,
            )
        )
    return facts


def peak_live_tensors(tensors: Iterable[TensorFact], op_count: int) -> int:
    changes: Counter[int] = Counter()
    for tensor in tensors:
        if tensor.alias_of is not None:
            continue
        changes[tensor.live_start] += 1
        changes[tensor.live_end] -= 1
    live = peak = 0
    for time in range(op_count + 2):
        live += changes[time]
        peak = max(peak, live)
    return peak


def make_report(analysis: GraphAnalysis) -> dict[str, Any]:
    graph = analysis.model.graph
    fusions = analysis.quant_fusions
    ranks = Counter(tensor.rank for tensor in analysis.tensors)
    dtypes = Counter(DTYPE_NAMES[tensor.dtype] for tensor in analysis.tensors)
    known_declared = [
        tensor for tensor in analysis.tensors if tensor.declared_shape is not None
    ]
    unknown_declared = len(analysis.tensors) - len(known_declared)
    phase_counts = Counter(tensor.phase for tensor in analysis.tensors)
    matmuls = [fusion for fusion in fusions if fusion.kind == "qgemm"]
    convs = [fusion for fusion in fusions if fusion.kind == "qconv1d"]
    view_counts = Counter(
        graph.node[tensor.producer].op_type
        for tensor in analysis.tensors
        if tensor.alias_of is not None and tensor.producer is not None
    )
    op_counts = Counter(node.op_type for node in graph.node)
    domain_counts = Counter(node_domain(node) for node in graph.node)
    tensor_descriptor_digest = hashlib.sha256(
        canonical_json(
            [
                {
                    "id": tensor.tensor_id,
                    "name": tensor.name,
                    "dtype": tensor.dtype,
                    "rank": tensor.rank,
                    "shape": tensor.declared_shape,
                    "producer": tensor.producer,
                    "initializer": tensor.initializer,
                    "input": tensor.graph_input,
                    "output": tensor.graph_output,
                    "alias_of": tensor.alias_of,
                    "live": [tensor.live_start, tensor.live_end],
                    "phase": tensor.phase,
                }
                for tensor in analysis.tensors
            ]
        )
    ).hexdigest()
    fusion_plan_digest = hashlib.sha256(
        canonical_json(
            [
                {
                    "kind": fusion.kind,
                    "kernel": fusion.kernel_index,
                    "dql": fusion.dynamic_quant_index,
                    "scale": fusion.scale_index,
                    "cast": fusion.cast_index,
                    "dequant_mul": fusion.dequant_mul_index,
                    "int32_bias": fusion.int32_bias,
                    "float_bias": fusion.float_bias,
                    "result": fusion.result_tensor,
                }
                for fusion in fusions
            ]
        )
    ).hexdigest()
    lowering_counts = Counter(record.op_type for record in analysis.lowerings)
    lowering_variants: dict[str, Counter[str]] = defaultdict(Counter)
    attribute_bytes = Counter()
    static_views = dynamic_views = 0
    for record in analysis.lowerings:
        lowering_variants[record.op_type][record.variant] += 1
        attribute_bytes[len(record.attributes)] += 1
        if record.op_type in LOWERED_VIEW_OPS:
            decoded = inspect_attribute_record(record.attributes, record.opcode)
            if int(decoded["flags"]) & ATTRIBUTE_VIEW_STATIC_CONTROL:
                static_views += 1
            else:
                dynamic_views += 1
    unary_count = sum(lowering_counts[name] for name in LOWERED_UNARY_OPS)
    core_f32_count = sum(
        lowering_counts[name]
        for name in LOWERED_F32_OPS
        if name not in LOWERED_UNARY_OPS
    )
    view_count = sum(lowering_counts[name] for name in LOWERED_VIEW_OPS)
    material_layout_count = sum(
        lowering_counts[name] for name in LOWERED_MATERIAL_LAYOUT_OPS
    )
    return {
        "schema": ANALYSIS_SCHEMA,
        "tool_version": TOOL_VERSION,
        "source": {
            "model_file": analysis.model_path.name,
            "model_bytes": analysis.model_bytes,
            "model_sha256": analysis.model_sha256,
            "voices_file": None if analysis.voices_path is None else analysis.voices_path.name,
            "voices_bytes": analysis.voices_bytes,
            "voices_sha256": analysis.voices_sha256,
        },
        "onnx": {
            "ir_version": int(analysis.model.ir_version),
            "producer": analysis.model.producer_name,
            "producer_version": analysis.model.producer_version,
            "opsets": [
                {"domain": item.domain, "version": int(item.version)}
                for item in analysis.model.opset_import
            ],
            "metadata": {
                item.key: item.value for item in analysis.model.metadata_props
            },
        },
        "graph": {
            "name": graph.name,
            "nodes": len(graph.node),
            "initializers": len(graph.initializer),
            "inputs": len(graph.input),
            "outputs": len(graph.output),
            "tensors": len(analysis.tensors),
            "operator_counts": dict(sorted(op_counts.items())),
            "domain_counts": dict(sorted(domain_counts.items())),
        },
        "tensor_contract": {
            "dtype_counts": dict(sorted(dtypes.items())),
            "rank_counts": {str(rank): count for rank, count in sorted(ranks.items())},
            "max_rank": max(ranks),
            "declared_shapes": len(known_declared),
            "ranks_inferred_without_declared_shape": unknown_declared,
            "view_aliases": sum(view_counts.values()),
            "view_alias_counts": dict(sorted(view_counts.items())),
            "descriptor_sha256": tensor_descriptor_digest,
            "phase0_only": phase_counts[0],
            "phase1_only": phase_counts[1],
            "shared": phase_counts[2],
            "peak_live_owners": peak_live_tensors(analysis.tensors, len(graph.node)),
        },
        "quantized_lowering": {
            "matmul_integer": len(matmuls),
            "matmul_recognized": len(matmuls),
            "matmul_float_bias": sum(fusion.float_bias for fusion in matmuls),
            "conv_integer": len(convs),
            "conv_recognized": len(convs),
            "conv_int32_bias": sum(fusion.int32_bias for fusion in convs),
            "conv_direct": sum(not fusion.int32_bias for fusion in convs),
            "conv_float_bias": sum(fusion.float_bias for fusion in convs),
            "dynamic_quantize_linear": sum(
                node.op_type == "DynamicQuantizeLinear" for node in graph.node
            ),
            "native_matmul_opcode": "DynamicQuantizedGemm:0x0300",
            "native_conv_opcode": "DynamicQuantizedConv1d:0x0301",
            "plan_sha256": fusion_plan_digest,
            "unsupported": 0,
        },
        "cpu_attribute_lowering": {
            "abi": "trueos.kokoro-op-attributes.v1",
            "abi_version": ATTRIBUTE_ABI_VERSION,
            "record_header": "<u16 version,u16 kind/opcode,u32 total_bytes>",
            "records": len(analysis.lowerings),
            "f32_core_records": core_f32_count,
            "f32_unary_records": unary_count,
            "f32_total_records": core_f32_count + unary_count,
            "layout_material_records": material_layout_count,
            "layout_view_records": view_count,
            "layout_total_records": material_layout_count + view_count,
            "view_alias_records": view_count,
            "view_static_controllers": static_views,
            "view_dynamic_controllers": dynamic_views,
            "excluded_non_f32_add": op_counts["Add"] - lowering_counts["Add"],
            "operator_counts": dict(sorted(lowering_counts.items())),
            "layout_operator_counts": {
                name: lowering_counts[name] for name in sorted(LOWERED_LAYOUT_OPS)
            },
            "attribute_size_counts": {
                str(size): count for size, count in sorted(attribute_bytes.items())
            },
            "variants": {
                op_type: dict(sorted(variants.items()))
                for op_type, variants in sorted(lowering_variants.items())
            },
            "plan_sha256": lowering_plan_sha256(analysis.lowerings),
            "unsupported_admitted": 0,
            "executable_graph_emitted": False,
        },
        "phases": {
            "count": 2,
            "phase0_source_nodes": [0, analysis.phase_cut],
            "phase1_source_nodes": [analysis.phase_cut, len(graph.node)],
            "resolve_decoder_shape": {
                "node_index": FRAME_COUNT_NODE_INDEX,
                "node_name": FRAME_COUNT_NODE_NAME,
                "tensor": FRAME_COUNT_TENSOR,
                "dtype": "INT64",
                "rank": 0,
                "first_sized_consumer_index": FRAME_RANGE_NODE_INDEX,
                "first_sized_consumer_name": FRAME_RANGE_NODE_NAME,
                "formula": "sum(max(1, round(sum(sigmoid(duration_logits), axis=-1) / speed)))",
            },
            "phase1_frame_descendants": 1_862,
            "phase1_alignment_companions": [1_747, 1_748, 1_750, 1_752, 1_755, 1_759],
            "topology_violations": 0,
            "fusion_crossings": 0,
        },
        "result": "accepted",
    }


def analyze(
    model_path: Path,
    voices_path: Path | None,
    *,
    pinned: bool = True,
) -> GraphAnalysis:
    onnx = require_onnx()
    model_path = model_path.resolve()
    reject(not model_path.is_file(), f"model does not exist: {model_path}")
    model_bytes, model_sha256 = sha256_file(model_path)
    voices_bytes = 0
    voices_sha256 = "0" * 64
    if voices_path is not None:
        voices_path = voices_path.resolve()
        reject(not voices_path.is_file(), f"voices archive does not exist: {voices_path}")
        voices_bytes, voices_sha256 = sha256_file(voices_path)
    if pinned:
        reject(voices_path is None, "pinned analysis requires voices-v1.0.bin")
        reject(
            voices_bytes != PINNED_VOICES_BYTES or voices_sha256 != PINNED_VOICES_SHA256,
            "voices archive provenance does not match pin",
        )

    model = onnx.load(str(model_path), load_external_data=False)
    if pinned:
        validate_pinned_model(onnx, model, model_bytes, model_sha256)
    else:
        onnx.checker.check_model(model)
    graph = model.graph
    initializers = {tensor.name: tensor for tensor in graph.initializer}
    raw_value_infos = list(graph.input) + list(graph.value_info) + list(graph.output)
    value_infos = {
        value.name: value
        for value in raw_value_infos
    }
    reject(len(initializers) != len(graph.initializer), "duplicate initializer name")
    reject(
        len(value_infos) != len(raw_value_infos),
        "duplicate value-info name",
    )
    producers, consumers = build_indexes(graph)
    dtypes = infer_dtypes(graph, value_infos, initializers)
    declared_ranks: dict[str, int | None] = {
        name: None if tensor_shape(value) is None else len(tensor_shape(value) or ())
        for name, value in value_infos.items()
    }
    declared_ranks.update(
        {name: len(tensor.dims) for name, tensor in initializers.items()}
    )
    ranks = infer_output_ranks(onnx, graph, initializers, declared_ranks)
    quant_fusions = recognize_quant_fusions(
        onnx, graph, producers, consumers, initializers, dtypes
    )
    if pinned:
        matmuls = [fusion for fusion in quant_fusions if fusion.kind == "qgemm"]
        convs = [fusion for fusion in quant_fusions if fusion.kind == "qconv1d"]
        reject(
            len(matmuls) != 148
            or sum(fusion.float_bias for fusion in matmuls) != 136
            or len(convs) != 87
            or sum(fusion.int32_bias for fusion in convs) != 80,
            "quantized lowering inventory changed",
        )
        phase_cut = validate_frame_phase(
            onnx, graph, producers, consumers, initializers, dtypes, ranks
        )
        for fusion in quant_fusions:
            indices = (
                fusion.dynamic_quant_index,
                fusion.kernel_index,
                fusion.scale_index,
                fusion.cast_index,
                fusion.dequant_mul_index,
            )
            reject(
                min(indices) < phase_cut <= max(indices),
                f"quant fusion at node {fusion.kernel_index} crosses phase boundary",
            )
    else:
        # Synthetic graphs use a named marker so their phase split remains
        # semantic while avoiding the enormous pinned node indices.
        markers = [
            index + 1
            for index, node in enumerate(graph.node)
            if node.name == "trueos.test.ResolveDecoderShape"
        ]
        reject(len(markers) != 1, "synthetic graph requires one ResolveDecoderShape marker")
        phase_cut = markers[0]
    tensors = assign_tensors(
        graph,
        producers,
        consumers,
        initializers,
        value_infos,
        dtypes,
        ranks,
        phase_cut,
    )
    analysis = GraphAnalysis(
        model_path=model_path,
        model_bytes=model_bytes,
        model_sha256=model_sha256,
        voices_path=voices_path,
        voices_bytes=voices_bytes,
        voices_sha256=voices_sha256,
        model=model,
        onnx=onnx,
        producers=producers,
        consumers=consumers,
        initializers=initializers,
        value_infos=value_infos,
        dtypes=dtypes,
        ranks=ranks,
        tensors=tensors,
        quant_fusions=quant_fusions,
        phase_cut=phase_cut,
    )
    analysis.lowerings = build_supported_lowerings(analysis)
    if pinned:
        lowering_counts = Counter(record.op_type for record in analysis.lowerings)
        reject(
            dict(sorted(lowering_counts.items())) != PINNED_LOWERING_COUNTS,
            "CPU lowering inventory changed",
        )
        lowering_digest = lowering_plan_sha256(analysis.lowerings)
        reject(
            lowering_digest != PINNED_LOWERING_SHA256,
            f"CPU lowering plan digest changed: {lowering_digest}",
        )
    analysis.report = make_report(analysis)
    return analysis


def write_atomic(path: Path, payload: bytes, force: bool) -> None:
    reject(path.exists() and not force, f"destination exists: {path} (use --force)")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    reject(temporary.exists(), f"stale temporary output exists: {temporary}")
    try:
        temporary.write_bytes(payload)
        temporary.replace(path)
    finally:
        if temporary.exists():
            temporary.unlink()


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    analyze_parser = subparsers.add_parser("analyze", help="validate and report")
    analyze_parser.add_argument("model", type=Path)
    analyze_parser.add_argument("--voices", type=Path, required=True)
    analyze_parser.add_argument("--report", type=Path)
    analyze_parser.add_argument("--force", action="store_true")
    fixture_parser = subparsers.add_parser(
        "fixture", help="emit the tiny deterministic v1 conformance artifact"
    )
    fixture_parser.add_argument("output", type=Path)
    fixture_parser.add_argument("--force", action="store_true")
    attribute_fixture_parser = subparsers.add_parser(
        "attribute-fixture",
        help="emit one canonical v1 record for each admitted CPU attribute kind",
    )
    attribute_fixture_parser.add_argument("output", type=Path)
    attribute_fixture_parser.add_argument("--force", action="store_true")
    inspect_parser = subparsers.add_parser(
        "inspect", help="verify and summarize a sealed v1 artifact"
    )
    inspect_parser.add_argument("artifact", type=Path)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        if args.command == "analyze":
            result = analyze(args.model, args.voices)
            payload = canonical_json(result.report)
            if args.report is None:
                sys.stdout.buffer.write(payload)
            else:
                write_atomic(args.report, payload, args.force)
            return 0
        if args.command == "fixture":
            write_atomic(args.output, synthetic_fixture_artifact(), args.force)
            return 0
        if args.command == "attribute-fixture":
            write_atomic(args.output, synthetic_attribute_fixture_artifact(), args.force)
            return 0
        if args.command == "inspect":
            sys.stdout.buffer.write(canonical_json(inspect_aot(args.artifact.read_bytes())))
            return 0
        raise CompileError(f"unsupported command {args.command!r}")
    except (CompileError, OSError) as error:
        print(f"kokoro-aot: rejected: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
