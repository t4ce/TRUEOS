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
        struct.pack_into("<IqI", record, 84, self.frame_multiplier, self.frame_addend, self.alignment)
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
            attribute_offset = align_up(len(data), 4)
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

    reject(any(binding >= len(program.tensors) for binding in bindings), "binding tensor ID rejected")
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

    struct.pack_into("<8sHHIQ", artifact, 0, AOT_MAGIC, AOT_VERSION, AOT_ENDIAN_TAG, AOT_HEADER_BYTES, len(artifact))
    struct.pack_into("<HHII", artifact, 24, AOT_SECTION_COUNT, AOT_PHASE_COUNT, 0, AOT_ARENA_ALIGNMENT)
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
    reject((version, endian, header_bytes) != (AOT_VERSION, AOT_ENDIAN_TAG, AOT_HEADER_BYTES), "artifact version rejected")
    artifact_bytes = struct.unpack_from("<Q", artifact, 16)[0]
    reject(artifact_bytes != len(artifact), "artifact length rejected")
    section_count, phase_count, flags, arena_alignment = struct.unpack_from("<HHII", artifact, 24)
    reject((section_count, phase_count, flags, arena_alignment) != (6, 2, 0, 64), "artifact fixed header rejected")
    reject(struct.unpack_from("<5H", artifact, 36) != (128, 64, 40, 48, 4), "artifact record sizes rejected")
    reject(any(artifact[46:64]), "artifact header reserved bytes rejected")
    reject(not any(artifact[96:128]) or not any(artifact[128:160]), "artifact provenance hash rejected")
    observed_seal = hashlib.sha256(
        artifact[:64] + bytes(32) + artifact[96:]
    ).digest()
    reject(observed_seal != artifact[64:96], "artifact seal rejected")

    cursor = AOT_HEADER_BYTES
    sections: dict[str, dict[str, int]] = {}
    for index, (kind, alignment, stride, name) in enumerate(AOT_SECTION_SPECS):
        entry = struct.unpack_from("<HHIQQII", artifact, 160 + index * 32)
        actual_kind, entry_flags, actual_alignment, offset, count, actual_stride, reserved = entry
        reject((actual_kind, entry_flags, actual_alignment, actual_stride, reserved) != (kind, 0, alignment, stride, 0), f"{name} directory entry rejected")
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
    for index in range(op_section["count"]):
        offset = op_section["offset"] + index * 40
        opcode, op_flags, phase = struct.unpack_from("<HHB", artifact, offset)
        reject(opcode not in AOT_OPCODES.values(), f"op {index}: opcode rejected")
        reject(op_flags & ~1 != 0 or phase not in {0, 1}, f"op {index}: flags rejected")
        reject(any(artifact[offset + 5 : offset + 8]) or any(artifact[offset + 32 : offset + 40]), f"op {index}: reserved bytes rejected")
    phase_section = sections["phases"]
    phase_ids = tuple(artifact[phase_section["offset"] + index * 48] for index in range(2))
    reject(phase_ids != (0, 1), "phase IDs rejected")
    return {
        "artifact_bytes": len(artifact),
        "artifact_sha256": artifact[64:96].hex(),
        "model_sha256": artifact[96:128].hex(),
        "voices_sha256": artifact[128:160].hex(),
        "sections": {name: value["count"] for name, value in sections.items()},
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
        missing = [name for name in node.output if name and name not in ranks]
        if not missing:
            continue
        attrs = attributes(onnx, node)
        inferred: list[int | None]
        first = input_rank(node)
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
                # The only prepared-graph Reshape missing declared shape is the
                # final iSTFT scatter reshape; it preserves its rank-3 data.
                result = first
            inferred = [result]
        elif node.op_type == "Expand":
            inferred = [ranks.get(node.output[0], first)]
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
            reject(reshape.op_type != "Reshape", f"node {kernel_index}: integer bias is not reshaped")
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
        reject(int(cast_attrs.get("to", -1)) != 1, f"node {cast_index}: accumulator cast is not FLOAT")
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
    reject(dtypes[FRAME_COUNT_TENSOR] != 7 or ranks[FRAME_COUNT_TENSOR] != 0, "frame count must be INT64 scalar")
    reject(consumers.get(FRAME_COUNT_TENSOR) != [FRAME_RANGE_NODE_INDEX], "frame count consumer changed")

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
        any(index < PHASE_ONE_RAW_START <= consumer for index in quant_indices for output in graph.node[index].output for consumer in consumers.get(output, []) if graph.node[consumer].op_type in {"Cast", "Mul"}),
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
            "unsupported": 0,
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
        if args.command == "inspect":
            sys.stdout.buffer.write(canonical_json(inspect_aot(args.artifact.read_bytes())))
            return 0
        raise CompileError(f"unsupported command {args.command!r}")
    except (CompileError, OSError) as error:
        print(f"kokoro-aot: rejected: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
