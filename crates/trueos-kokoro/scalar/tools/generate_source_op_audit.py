#!/usr/bin/env python3
"""Audit the pinned source graph and generate compact ORT source-op fixtures."""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import struct
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
from onnx import TensorProto, helper, numpy_helper


MODEL_SHA256 = "239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29"
TOTAL_NODES = 3_615
POW_MAGIC = b"KPOW1271"
POW_HEADER = struct.Struct("<8s2I")
SCALAR_MAGIC = b"KSRC1271"
SCALAR_HEADER = struct.Struct("<8s5I")

POW_INPUT = np.asarray(
    [
        -4.0,
        -1.5,
        -0.0,
        0.0,
        2.0**-10,
        0.5,
        1.0,
        3.25,
        100.0,
        -100.0,
        8192.0,
        -8192.0,
    ],
    dtype=np.float32,
).reshape(2, 2, 3)

LESS_I64_LHS = np.asarray([[-3, 0, 7, 19]], dtype=np.int64)
LESS_I64_RHS = np.asarray([[-2], [0], [8]], dtype=np.int64)
LESS_F32_LHS = np.asarray(-0.125, dtype=np.float32)
LESS_F32_RHS = np.asarray(0.0, dtype=np.float32)
ADD_I64_LHS = np.asarray([[-5], [10]], dtype=np.int64)
ADD_I64_RHS = np.asarray([[1, 2, 3]], dtype=np.int64)
DEQUANT_INPUT = np.asarray(
    [-128, -96, -64, -32, -7, -1, 0, 1, 7, 16, 31, 63, 95, 112, 126, 127],
    dtype=np.int8,
).reshape(2, 2, 4)
DEQUANT_ZERO_POINT = np.asarray(0, dtype=np.int8)
DEQUANT_SCALE_BITS = (0x3BAC9658, 0x3D8CC081, 0x3BA0F77C, 0x3C281417)
DEQUANT_SCALES = np.asarray(DEQUANT_SCALE_BITS, dtype=np.uint32).view(np.float32)

LAYOUT_OPS = {
    "Concat",
    "Expand",
    "Gather",
    "NonZero",
    "Pad",
    "Reshape",
    "ScatterND",
    "Shape",
    "Slice",
    "Split",
    "Squeeze",
    "Transpose",
    "Unsqueeze",
}
F32_ALWAYS_OPS = {
    "Atan",
    "Cos",
    "Exp",
    "FastGelu",
    "Floor",
    "LayerNormalization",
    "LeakyRelu",
    "Pow",
    "ReduceMean",
    "Round",
    "Sigmoid",
    "Sin",
    "SkipLayerNormalization",
    "Softmax",
    "Sqrt",
    "Tanh",
}
SCALAR_ALWAYS_OPS = {
    "And",
    "Cast",
    "ConstantOfShape",
    "CumSum",
    "DequantizeLinear",
    "Equal",
    "Greater",
    "GreaterOrEqual",
    "Less",
    "Range",
    "Where",
}
EXPECTED_CATEGORIES = {
    "f32": 1_935,
    "scalar": 366,
    "layout": 811,
    "gemm": 27,
    "conv": 7,
    "resize": 6,
    "lstm": 6,
    "stft": 1,
    "quant": 454,
    "duration": 2,
}


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tensor_types(model: onnx.ModelProto) -> dict[str, str]:
    types: dict[str, str] = {}
    for value in list(model.graph.input) + list(model.graph.value_info) + list(model.graph.output):
        tensor_type = value.type.tensor_type
        if tensor_type.elem_type:
            types[value.name] = TensorProto.DataType.Name(tensor_type.elem_type)
    for initializer in model.graph.initializer:
        types[initializer.name] = TensorProto.DataType.Name(initializer.data_type)
    return types


def classify_node(node: onnx.NodeProto, types: dict[str, str]) -> str | None:
    operation = node.op_type
    if operation in LAYOUT_OPS:
        return "layout"
    if operation in F32_ALWAYS_OPS:
        return "f32"
    if operation in SCALAR_ALWAYS_OPS:
        return "scalar"
    if operation in {"Mul", "Div", "Sub"}:
        return "f32" if types.get(node.input[0]) == "FLOAT" else None
    if operation == "Add":
        dtype = types.get(node.input[0])
        return {"FLOAT": "f32", "INT32": "quant", "INT64": "scalar"}.get(dtype)
    if operation == "MatMul":
        return "gemm"
    if operation in {"Conv", "ConvTranspose"}:
        return "conv"
    if operation == "Resize":
        return "resize"
    if operation == "LSTM":
        return "lstm"
    if operation == "STFT":
        return "stft"
    if operation in {"DynamicQuantizeLinear", "ConvInteger", "MatMulInteger"}:
        return "quant"
    if operation in {"Clip", "ReduceSum"}:
        return "duration"
    return None


def audit_model(model_path: Path) -> tuple[onnx.ModelProto, dict[str, object]]:
    digest = file_sha256(model_path)
    if digest != MODEL_SHA256:
        raise AssertionError(f"model SHA-256 changed: expected {MODEL_SHA256}, got {digest}")
    model = onnx.load(model_path)
    if len(model.graph.node) != TOTAL_NODES:
        raise AssertionError(f"expected {TOTAL_NODES} nodes, found {len(model.graph.node)}")

    types = tensor_types(model)
    initializers = {initializer.name: initializer for initializer in model.graph.initializer}

    pow_nodes = [(index, node) for index, node in enumerate(model.graph.node) if node.op_type == "Pow"]
    if len(pow_nodes) != 50:
        raise AssertionError(f"expected 50 Pow nodes, found {len(pow_nodes)}")
    exponent_inputs = set()
    for _, node in pow_nodes:
        exponent = numpy_helper.to_array(initializers[node.input[1]])
        if exponent.shape != () or exponent.dtype != np.float32 or exponent.view(np.uint32).item() != 0x40000000:
            raise AssertionError(f"non-square Pow node {node.name!r}")
        exponent_inputs.add(node.input[1])
    if len(exponent_inputs) != 1:
        raise AssertionError("Pow nodes do not share one sealed exponent initializer")

    less_nodes = [(index, node) for index, node in enumerate(model.graph.node) if node.op_type == "Less"]
    if len(less_nodes) != 2:
        raise AssertionError(f"expected two Less nodes, found {len(less_nodes)}")
    less_signatures = sorted((types[node.input[0]], types[node.input[1]]) for _, node in less_nodes)
    if less_signatures != [("FLOAT", "FLOAT"), ("INT64", "INT64")]:
        raise AssertionError(f"unexpected Less signatures: {less_signatures!r}")
    float_less = next(node for _, node in less_nodes if types[node.input[0]] == "FLOAT")
    less_rhs = numpy_helper.to_array(initializers[float_less.input[1]])
    if less_rhs.shape != () or less_rhs.dtype != np.float32 or less_rhs.view(np.uint32).item() != 0:
        raise AssertionError("pinned FLOAT Less RHS is not scalar +0.0")

    dequant_nodes = [
        (index, node)
        for index, node in enumerate(model.graph.node)
        if node.op_type == "DequantizeLinear"
    ]
    if [index for index, _ in dequant_nodes] != [5, 6, 388, 544]:
        raise AssertionError("embedding DequantizeLinear node indices changed")
    actual_scale_bits = []
    for _, node in dequant_nodes:
        if types[node.input[0]] != "INT8" or types[node.output[0]] != "FLOAT":
            raise AssertionError(f"unexpected DequantizeLinear dtype at {node.name!r}")
        scale = numpy_helper.to_array(initializers[node.input[1]])
        zero_point = numpy_helper.to_array(initializers[node.input[2]])
        if scale.shape != () or scale.dtype != np.float32 or not float(scale) > 0.0:
            raise AssertionError(f"non-scalar/invalid scale at {node.name!r}")
        if zero_point.shape != () or zero_point.dtype != np.int8 or int(zero_point) != 0:
            raise AssertionError(f"non-scalar/nonzero INT8 zero point at {node.name!r}")
        actual_scale_bits.append(scale.view(np.uint32).item())
    if tuple(actual_scale_bits) != DEQUANT_SCALE_BITS:
        raise AssertionError(f"embedding scale bits changed: {actual_scale_bits!r}")

    int64_adds = [
        (index, node)
        for index, node in enumerate(model.graph.node)
        if node.op_type == "Add" and types.get(node.input[0]) == "INT64"
    ]
    if len(int64_adds) != 1 or int64_adds[0][0] != 3593:
        raise AssertionError(f"unexpected INT64 Add nodes: {int64_adds!r}")

    category_counts: collections.Counter[str] = collections.Counter()
    category_operators: dict[str, collections.Counter[str]] = collections.defaultdict(collections.Counter)
    uncovered = []
    for index, node in enumerate(model.graph.node):
        category = classify_node(node, types)
        if category is None:
            uncovered.append({"index": index, "name": node.name, "op_type": node.op_type})
            continue
        category_counts[category] += 1
        category_operators[category][node.op_type] += 1

    if uncovered:
        raise AssertionError(f"uncovered source nodes: {uncovered!r}")
    if dict(category_counts) != EXPECTED_CATEGORIES:
        raise AssertionError(f"coverage categories changed: {dict(category_counts)!r}")
    if sum(category_counts.values()) != TOTAL_NODES:
        raise AssertionError("coverage categories do not reconcile to total nodes")

    operator_counts = collections.Counter(node.op_type for node in model.graph.node)
    audit = {
        "schema": "trueos-kokoro-source-coverage-v1",
        "model_sha256": MODEL_SHA256,
        "source_nodes": TOTAL_NODES,
        "covered_nodes": sum(category_counts.values()),
        "uncovered_nodes": [],
        "category_counts": dict(sorted(category_counts.items())),
        "category_operator_counts": {
            category: dict(sorted(counts.items()))
            for category, counts in sorted(category_operators.items())
        },
        "operator_counts": dict(sorted(operator_counts.items())),
        "classification_notes": {
            "quant": "139 DynamicQuantizeLinear + 87 ConvInteger + 148 MatMulInteger + 80 INT32 Add epilogues",
            "duration": "ReduceSum and Clip are owned by the exact duration fusion; its remaining source nodes use generic lanes",
            "scalar": "includes four INT8 scalar-parameter DequantizeLinear nodes and the sole INT64 Add",
            "non_overlapping": True,
        },
        "audited_nodes": {
            "pow_square": [index for index, _ in pow_nodes],
            "less": [index for index, _ in less_nodes],
            "embedding_dequantize_linear": [index for index, _ in dequant_nodes],
            "int64_add": [index for index, _ in int64_adds],
        },
    }
    return model, audit


def session_for(graph: onnx.GraphProto) -> ort.InferenceSession:
    model = helper.make_model(
        graph,
        ir_version=10,
        opset_imports=[helper.make_opsetid("", 20)],
        producer_name="trueos-kokoro-source-op-fixture",
    )
    onnx.checker.check_model(model)
    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_DISABLE_ALL
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1
    return ort.InferenceSession(
        model.SerializeToString(),
        sess_options=options,
        providers=["CPUExecutionProvider"],
    )


def pow_output() -> np.ndarray:
    graph = helper.make_graph(
        [helper.make_node("Pow", ["input", "exponent"], ["output"])],
        "trueos_pow_square_oracle",
        [],
        [helper.make_tensor_value_info("output", TensorProto.FLOAT, [2, 2, 3])],
        [
            numpy_helper.from_array(POW_INPUT, "input"),
            numpy_helper.from_array(np.asarray(2.0, dtype=np.float32), "exponent"),
        ],
    )
    (output,) = session_for(graph).run(None, {})
    return np.asarray(output, dtype="<f4").reshape(-1)


def scalar_outputs() -> tuple[np.ndarray, ...]:
    nodes = [
        helper.make_node("Less", ["less_i64_lhs", "less_i64_rhs"], ["less_i64"]),
        helper.make_node("Less", ["less_f32_lhs", "less_f32_rhs"], ["less_f32"]),
        helper.make_node("Add", ["add_i64_lhs", "add_i64_rhs"], ["add_i64"]),
    ]
    initializers = [
        numpy_helper.from_array(LESS_I64_LHS, "less_i64_lhs"),
        numpy_helper.from_array(LESS_I64_RHS, "less_i64_rhs"),
        numpy_helper.from_array(LESS_F32_LHS, "less_f32_lhs"),
        numpy_helper.from_array(LESS_F32_RHS, "less_f32_rhs"),
        numpy_helper.from_array(ADD_I64_LHS, "add_i64_lhs"),
        numpy_helper.from_array(ADD_I64_RHS, "add_i64_rhs"),
        numpy_helper.from_array(DEQUANT_INPUT, "dequant_input"),
        numpy_helper.from_array(DEQUANT_ZERO_POINT, "dequant_zero_point"),
    ]
    outputs = [
        helper.make_tensor_value_info("less_i64", TensorProto.BOOL, [3, 4]),
        helper.make_tensor_value_info("less_f32", TensorProto.BOOL, []),
        helper.make_tensor_value_info("add_i64", TensorProto.INT64, [2, 3]),
    ]
    for index, scale in enumerate(DEQUANT_SCALES):
        scale_name = f"dequant_scale_{index}"
        output_name = f"dequant_{index}"
        initializers.append(numpy_helper.from_array(np.asarray(scale, dtype=np.float32), scale_name))
        nodes.append(
            helper.make_node(
                "DequantizeLinear",
                ["dequant_input", scale_name, "dequant_zero_point"],
                [output_name],
            )
        )
        outputs.append(
            helper.make_tensor_value_info(output_name, TensorProto.FLOAT, [2, 2, 4])
        )
    graph = helper.make_graph(nodes, "trueos_scalar_source_ops_oracle", [], outputs, initializers)
    values = session_for(graph).run(None, {})
    less_i64 = np.asarray(values[0], dtype=np.bool_).reshape(-1)
    less_f32 = np.asarray(values[1], dtype=np.bool_).reshape(-1)
    add_i64 = np.asarray(values[2], dtype="<i8").reshape(-1)
    dequant = np.concatenate(
        [np.asarray(value, dtype="<f4").reshape(-1) for value in values[3:]]
    )
    return less_i64, less_f32, add_i64, dequant


def encode_pow(output: np.ndarray) -> bytes:
    input_values = np.asarray(POW_INPUT, dtype="<f4").reshape(-1)
    return POW_HEADER.pack(POW_MAGIC, 1, input_values.size) + input_values.tobytes() + output.tobytes()


def encode_scalar(outputs: tuple[np.ndarray, ...]) -> bytes:
    less_i64, less_f32, add_i64, dequant = outputs
    return (
        SCALAR_HEADER.pack(
            SCALAR_MAGIC,
            1,
            less_i64.size,
            less_f32.size,
            add_i64.size,
            dequant.size,
        )
        + less_i64.astype(np.uint8).tobytes()
        + less_f32.astype(np.uint8).tobytes()
        + add_i64.tobytes()
        + dequant.tobytes()
    )


def fixture_metadata(name: str, blob: bytes, details: dict[str, object]) -> dict[str, object]:
    return {
        "schema": "trueos-kokoro-source-op-ort-fixture-v1",
        "fixture": name,
        "sha256": hashlib.sha256(blob).hexdigest(),
        "model_sha256": MODEL_SHA256,
        "onnxruntime_version": ort.__version__,
        "onnx_version": onnx.__version__,
        "execution_provider": "CPUExecutionProvider",
        "graph_optimization": "ORT_DISABLE_ALL",
        "intra_op_threads": 1,
        "inter_op_threads": 1,
        "opset": 20,
        "ir_version": 10,
        **details,
    }


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def verify_equal(label: str, actual: bytes, expected_path: Path) -> None:
    expected = expected_path.read_bytes()
    if actual != expected:
        raise AssertionError(
            f"{label} differs under ORT {ort.__version__}: "
            f"expected {hashlib.sha256(expected).hexdigest()}, got {hashlib.sha256(actual).hexdigest()}"
        )
    print(f"verified {label}: ORT {ort.__version__}, bit-exact {len(actual)} bytes")


def main() -> None:
    repo_root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser()
    parser.add_argument("--verify", action="store_true")
    parser.add_argument(
        "--model",
        type=Path,
        default=repo_root / "tools/ttstt/models/kokoro/kokoro-rten.onnx",
    )
    args = parser.parse_args()

    _, audit = audit_model(args.model)
    scalar_dir = repo_root / "crates/trueos-kokoro-scalar/tests/fixtures"
    f32_dir = repo_root / "crates/trueos-kokoro-f32/tests/fixtures"
    scalar_dir.mkdir(parents=True, exist_ok=True)
    f32_dir.mkdir(parents=True, exist_ok=True)

    pow_blob = encode_pow(pow_output())
    scalar_blob = encode_scalar(scalar_outputs())
    pow_path = f32_dir / "ort127_pow_square.bin"
    scalar_path = scalar_dir / "ort127_source_ops.bin"
    if args.verify:
        verify_equal("Pow square fixture", pow_blob, pow_path)
        verify_equal("scalar source-op fixture", scalar_blob, scalar_path)
        print(f"coverage verified: {audit['covered_nodes']}/{audit['source_nodes']} source nodes")
        return

    pow_path.write_bytes(pow_blob)
    scalar_path.write_bytes(scalar_blob)
    write_json(
        f32_dir / "ort127_pow_square.json",
        fixture_metadata(
            pow_path.name,
            pow_blob,
            {
                "operation": "Pow",
                "input_shape": list(POW_INPUT.shape),
                "exponent_dtype": "FLOAT",
                "exponent_bits": "0x40000000",
                "payload": "header, input FLOAT bits, output FLOAT bits",
            },
        ),
    )
    write_json(
        scalar_dir / "ort127_source_ops.json",
        fixture_metadata(
            scalar_path.name,
            scalar_blob,
            {
                "operations": ["Less(INT64)", "Less(FLOAT)", "Add(INT64)", "DequantizeLinear(INT8)"],
                "less_i64_shapes": [[1, 4], [3, 1], [3, 4]],
                "less_f32_shapes": [[], [], []],
                "add_i64_shapes": [[2, 1], [1, 3], [2, 3]],
                "dequant_shape": list(DEQUANT_INPUT.shape),
                "dequant_scale_bits": [f"0x{value:08x}" for value in DEQUANT_SCALE_BITS],
                "dequant_zero_point": 0,
                "payload": "header, Less BOOL outputs, Add INT64 output, four DequantizeLinear FLOAT outputs",
            },
        ),
    )
    write_json(scalar_dir / "pinned_coverage_3615.json", audit)
    print(f"wrote {pow_path}: sha256={hashlib.sha256(pow_blob).hexdigest()}")
    print(f"wrote {scalar_path}: sha256={hashlib.sha256(scalar_blob).hexdigest()}")
    print(f"coverage: {audit['covered_nodes']}/{audit['source_nodes']}, uncovered=0")


if __name__ == "__main__":
    main()
