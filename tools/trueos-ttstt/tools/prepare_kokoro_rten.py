#!/usr/bin/env python3
"""Prepare the quantized Kokoro ONNX graph for RTen.

The taylorchu v0.2.0 model contains two ONNX Runtime-only operators which
RTen intentionally does not implement:

* six ``com.microsoft.DynamicQuantizeLSTM`` nodes;
* one ``com.microsoft.FusedMatMul`` node.

This tool performs the narrow, deterministic bridge which was validated for
that graph. LSTM weights are dequantized once to f32, transposed to standard
ONNX LSTM layout and attached to ordinary ``LSTM`` nodes. The fused matmul is
expanded to ``Transpose`` + ``MatMul``. Quantized convolution and matrix
multiplication everywhere else remain quantized.
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import sys
import tempfile

try:
    import numpy as np
    import onnx
    from onnx import helper, numpy_helper
except ImportError as error:
    raise SystemExit(
        "missing bridge dependency; install it with "
        "`python3 -m pip install 'onnx>=1.16,<2' numpy`"
    ) from error


BRIDGE_VERSION = "1"
EXPECTED_DYNAMIC_LSTMS = 6
EXPECTED_FUSED_MATMULS = 1


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="source quantized Kokoro .onnx")
    parser.add_argument("output", type=Path, help="destination kokoro-rten.onnx")
    parser.add_argument(
        "--force", action="store_true", help="replace an existing destination"
    )
    return parser.parse_args()


def attributes(node: onnx.NodeProto) -> dict[str, object]:
    return {
        attribute.name: helper.get_attribute_value(attribute)
        for attribute in node.attribute
    }


def dequantized_lstm_initializer(
    quantized: onnx.TensorProto,
    scale: onnx.TensorProto,
    zero_point: onnx.TensorProto,
) -> onnx.TensorProto:
    q = numpy_helper.to_array(quantized)
    scales = numpy_helper.to_array(scale).astype(np.float32, copy=False)
    zero_points = numpy_helper.to_array(zero_point).astype(np.float32, copy=False)

    if q.ndim != 3 or scales.shape != (q.shape[0],):
        raise ValueError(
            f"unexpected DynamicQuantizeLSTM weight layout for {quantized.name}: "
            f"q={q.shape}, scale={scales.shape}"
        )
    if zero_points.shape not in ((), (q.shape[0],)):
        raise ValueError(
            f"unexpected zero-point layout for {quantized.name}: {zero_points.shape}"
        )

    # The contrib operator stores [direction, input, 4*hidden]. Standard ONNX
    # LSTM consumes [direction, 4*hidden, input]. Scale/zero point are per
    # direction in this model.
    broadcast_shape = (q.shape[0], 1, 1)
    zero_shape = (1, 1, 1) if zero_points.shape == () else broadcast_shape
    values = q.astype(np.float32) - zero_points.reshape(zero_shape)
    values *= scales.reshape(broadcast_shape)
    values = np.ascontiguousarray(values.transpose(0, 2, 1))
    return numpy_helper.from_array(values, f"{quantized.name}__dequant_f32")


def bridge_dynamic_lstm(
    node: onnx.NodeProto,
    initializers: dict[str, onnx.TensorProto],
) -> tuple[list[onnx.NodeProto], list[onnx.TensorProto], set[str]]:
    if len(node.input) < 12:
        raise ValueError(f"{node.name}: DynamicQuantizeLSTM has too few inputs")

    weight_specs = ((1, 8, 9), (2, 10, 11))
    replacement_names: list[str] = []
    added: list[onnx.TensorProto] = []
    removable: set[str] = set()
    for quant_index, scale_index, zero_index in weight_specs:
        names = (node.input[quant_index], node.input[scale_index], node.input[zero_index])
        try:
            quantized, scale, zero_point = (initializers[name] for name in names)
        except KeyError as error:
            raise ValueError(f"{node.name}: input {error.args[0]!r} is not an initializer")
        converted = dequantized_lstm_initializer(quantized, scale, zero_point)
        replacement_names.append(converted.name)
        added.append(converted)
        removable.update(names)

    attrs = attributes(node)
    allowed_attrs = {
        name: attrs[name]
        for name in ("direction", "hidden_size", "input_forget")
        if name in attrs
    }
    inputs = list(node.input[:8])
    inputs[1:3] = replacement_names
    # RTen's standard LSTM currently implements inputs through initial_c but
    # not the optional peephole input P. The source graph leaves P empty, so
    # omit that trailing placeholder while preserving the empty sequence_lens
    # placeholder in the middle.
    while inputs and not inputs[-1]:
        inputs.pop()
    standard = helper.make_node(
        "LSTM",
        inputs,
        list(node.output),
        name=f"{node.name}__standard",
        **allowed_attrs,
    )
    return [standard], added, removable


def bridge_fused_matmul(node: onnx.NodeProto) -> list[onnx.NodeProto]:
    attrs = attributes(node)
    expected = {
        "transA": 1,
        "transB": 0,
        "transBatchA": 0,
        "transBatchB": 0,
        "alpha": 1.0,
    }
    for name, value in expected.items():
        if attrs.get(name, 0 if name != "alpha" else 1.0) != value:
            raise ValueError(
                f"{node.name}: unsupported FusedMatMul attribute {name}={attrs.get(name)!r}"
            )
    if len(node.input) != 2 or len(node.output) != 1:
        raise ValueError(f"{node.name}: expected a two-input, one-output FusedMatMul")

    transposed = f"{node.input[0]}__last2_transposed"
    prefix = node.name or "FusedMatMul"
    return [
        helper.make_node(
            "Transpose",
            [node.input[0]],
            [transposed],
            name=f"{prefix}__transposeA",
            perm=[0, 2, 1],
        ),
        helper.make_node(
            "MatMul",
            [transposed, node.input[1]],
            list(node.output),
            name=f"{prefix}__matmul",
        ),
    ]


def bridge(model: onnx.ModelProto) -> tuple[int, int, int]:
    if model.graph is None:
        raise ValueError("model has no graph")
    initializers = {tensor.name: tensor for tensor in model.graph.initializer}
    new_nodes: list[onnx.NodeProto] = []
    added_initializers: list[onnx.TensorProto] = []
    removable_initializers: set[str] = set()
    lstm_count = 0
    matmul_count = 0

    for node in model.graph.node:
        if node.domain == "com.microsoft" and node.op_type == "DynamicQuantizeLSTM":
            nodes, added, removable = bridge_dynamic_lstm(node, initializers)
            new_nodes.extend(nodes)
            added_initializers.extend(added)
            removable_initializers.update(removable)
            lstm_count += 1
        elif node.domain == "com.microsoft" and node.op_type == "FusedMatMul":
            new_nodes.extend(bridge_fused_matmul(node))
            matmul_count += 1
        else:
            new_nodes.append(node)

    if lstm_count != EXPECTED_DYNAMIC_LSTMS or matmul_count != EXPECTED_FUSED_MATMULS:
        raise ValueError(
            "source graph does not match the validated Kokoro quant graph: "
            f"found {lstm_count} DynamicQuantizeLSTM and {matmul_count} FusedMatMul; "
            f"expected {EXPECTED_DYNAMIC_LSTMS} and {EXPECTED_FUSED_MATMULS}"
        )

    del model.graph.node[:]
    model.graph.node.extend(new_nodes)
    model.graph.initializer.extend(added_initializers)

    referenced = {name for node in new_nodes for name in node.input if name}
    kept = [
        tensor
        for tensor in model.graph.initializer
        if tensor.name not in removable_initializers or tensor.name in referenced
    ]
    removed_count = len(model.graph.initializer) - len(kept)
    del model.graph.initializer[:]
    model.graph.initializer.extend(kept)

    metadata = {entry.key: entry for entry in model.metadata_props}
    key = "trueos.ttstt.rten_bridge"
    if key in metadata:
        metadata[key].value = BRIDGE_VERSION
    else:
        entry = model.metadata_props.add()
        entry.key = key
        entry.value = BRIDGE_VERSION
    return lstm_count, matmul_count, removed_count


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while block := source.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def main() -> int:
    args = parse_args()
    source = args.source.resolve()
    output = args.output.resolve()
    if source == output:
        raise SystemExit("source and output must be different files")
    if not source.is_file():
        raise SystemExit(f"source model does not exist: {source}")
    if output.exists() and not args.force:
        raise SystemExit(f"output already exists: {output} (pass --force to replace it)")

    model = onnx.load(source)
    lstms, matmuls, removed = bridge(model)
    onnx.checker.check_model(model)

    output.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary_name = tempfile.mkstemp(
        prefix=f".{output.name}.", suffix=".tmp", dir=output.parent
    )
    os.close(fd)
    temporary = Path(temporary_name)
    try:
        onnx.save(model, temporary)
        os.replace(temporary, output)
    finally:
        temporary.unlink(missing_ok=True)

    print(f"source_sha256={sha256(source)}")
    print(f"output_sha256={sha256(output)}")
    print(
        f"bridged_dynamic_quantize_lstm={lstms} "
        f"bridged_fused_matmul={matmuls} pruned_initializers={removed}"
    )
    print(f"output={output} bytes={output.stat().st_size}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, onnx.checker.ValidationError) as error:
        raise SystemExit(f"bridge failed: {error}") from error
