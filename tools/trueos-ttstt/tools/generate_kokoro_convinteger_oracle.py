#!/usr/bin/env python3
"""Generate and verify a compact Kokoro ConvInteger kernel oracle.

The fixture is derived from a real Kokoro inference for the deterministic IPA
input used by the ttstt RTen/ORT smoke test.  It stores only three receptive
fields and two output-channel weight planes from the hottest ORT ConvInteger
node.  This is enough to validate the exact integer accumulator, including
left and right padding, without checking a multi-megabyte activation into Git.

Generation requires the pinned Python packages listed in ``PINNED_TOOLS``.
Self-verification of an existing fixture uses only the Python standard library.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
from pathlib import Path
import struct
import sys
import tempfile
from typing import Any


SCHEMA = "trueos.ttstt.kokoro-convinteger-oracle.v1"
SOURCE_MODEL_SHA256 = "6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb"
SOURCE_VOICES_SHA256 = "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d"
PINNED_TOOLS = {
    "numpy": "2.5.1",
    "onnx": "1.22.0",
    "onnxruntime": "1.28.0",
}

PHONEMES = "həlˈoʊ fɹʌm ɹʌst"
TOKEN_IDS = [50, 83, 54, 156, 57, 135, 16, 48, 123, 138, 55, 16, 123, 138, 61, 62]
PADDED_TOKEN_IDS = [0, *TOKEN_IDS, 0]
VOICE = "af_heart"
STYLE_INDEX = len(TOKEN_IDS)
SPEED = 1.0

# This was the highest cumulative ConvInteger node in the pinned ORT profile.
# It also exercises non-trivial dilation and padding.  RTen's top nodes are the
# same [1, 128, L] x [128, 128, 11] generator family.
TARGET_NODE = "/decoder/decoder/generator/noise_res.1/convs1.2/Conv_quant"
TARGET_GRAPH_INDEX = 2570
OUTPUT_CHANNELS = [0, 127]

EXPECTED_ATTRIBUTES = {
    "auto_pad": b"NOTSET",
    "dilations": [5],
    "group": 1,
    "kernel_shape": [11],
    "pads": [25, 25],
    "strides": [1],
}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def encode_bytes(data: bytes) -> dict[str, Any]:
    return {
        "encoding": "base64",
        "byte_length": len(data),
        "sha256": sha256_bytes(data),
        "data": base64.b64encode(data).decode("ascii"),
    }


def decode_bytes(field: dict[str, Any], label: str) -> bytes:
    if field.get("encoding") != "base64":
        raise ValueError(f"{label}: expected base64 encoding")
    data = base64.b64decode(field["data"], validate=True)
    if len(data) != field["byte_length"]:
        raise ValueError(f"{label}: byte length mismatch")
    if sha256_bytes(data) != field["sha256"]:
        raise ValueError(f"{label}: SHA-256 mismatch")
    return data


def canonical_json(fixture: dict[str, Any]) -> bytes:
    return (json.dumps(fixture, indent=2, sort_keys=True) + "\n").encode("utf-8")


def verify_fixture(path: Path) -> str:
    """Verify encoded hashes and recompute every golden accumulator exactly."""

    fixture = json.loads(path.read_text(encoding="utf-8"))
    if fixture.get("schema") != SCHEMA:
        raise ValueError(f"unexpected fixture schema {fixture.get('schema')!r}")

    compact = fixture["compact_fixture"]
    patch_shape = compact["patches"]["shape"]
    weight_shape = compact["weights"]["shape"]
    expected_shape = compact["expected_i32"]["shape"]
    if len(patch_shape) != 3 or len(weight_shape) != 3 or len(expected_shape) != 2:
        raise ValueError("unexpected compact tensor rank")

    patches = decode_bytes(compact["patches"]["bytes"], "patches")
    weights = decode_bytes(compact["weights"]["bytes"], "weights")
    expected_bytes = decode_bytes(compact["expected_i32"]["bytes"], "expected")

    positions, input_channels, kernel_width = patch_shape
    output_channels, weight_input_channels, weight_kernel_width = weight_shape
    if (weight_input_channels, weight_kernel_width) != (input_channels, kernel_width):
        raise ValueError("patch/weight dimensions disagree")
    if expected_shape != [positions, output_channels]:
        raise ValueError("expected-output dimensions disagree")
    if len(patches) != positions * input_channels * kernel_width:
        raise ValueError("patch payload size disagrees with shape")
    if len(weights) != output_channels * input_channels * kernel_width:
        raise ValueError("weight payload size disagrees with shape")
    if len(expected_bytes) != positions * output_channels * 4:
        raise ValueError("expected payload size disagrees with shape")

    x_zero_point = int(fixture["operator"]["x_zero_point"]["value"])
    w_zero_point_meta = fixture["operator"]["w_zero_point"]
    if w_zero_point_meta["broadcast"] == "scalar":
        w_zero_points = [int(w_zero_point_meta["value"])] * output_channels
    elif w_zero_point_meta["broadcast"] == "per_output_channel":
        w_zero_points = [int(value) for value in w_zero_point_meta["selected_values"]]
    else:
        raise ValueError("unsupported weight zero-point broadcast")

    expected = list(struct.unpack(f"<{positions * output_channels}i", expected_bytes))
    actual: list[int] = []
    plane = input_channels * kernel_width
    for position_index in range(positions):
        patch_base = position_index * plane
        for output_index in range(output_channels):
            weight_base = output_index * plane
            w_zero_point = w_zero_points[output_index]
            accumulator = 0
            for offset in range(plane):
                accumulator += (patches[patch_base + offset] - x_zero_point) * (
                    weights[weight_base + offset] - w_zero_point
                )
            actual.append(accumulator)

    if actual != expected:
        raise ValueError(f"accumulator mismatch: expected {expected}, got {actual}")
    if compact["expected_i32"]["values"] != [
        actual[index : index + output_channels]
        for index in range(0, len(actual), output_channels)
    ]:
        raise ValueError("human-readable expected values disagree with encoded values")

    file_hash = sha256_file(path)
    print(
        f"verified {path}: {positions * output_channels} exact accumulators, "
        f"fixture_sha256={file_hash}"
    )
    return file_hash


def require_tool_versions(allow_mismatch: bool) -> tuple[Any, Any, Any]:
    try:
        import numpy as np
        import onnx
        import onnxruntime as ort
    except ImportError as error:
        raise SystemExit(
            "generation requires numpy, onnx and onnxruntime; install the pinned "
            "versions documented by --help"
        ) from error

    actual = {
        "numpy": np.__version__,
        "onnx": onnx.__version__,
        "onnxruntime": ort.__version__,
    }
    if actual != PINNED_TOOLS and not allow_mismatch:
        raise SystemExit(
            f"tool version mismatch: expected {PINNED_TOOLS}, got {actual}; "
            "use --allow-tool-version-mismatch only to investigate a new oracle"
        )
    return np, onnx, ort


def checked_source_hash(path: Path, expected: str, label: str) -> str:
    actual = sha256_file(path)
    if actual != expected:
        raise ValueError(f"{label} SHA-256 mismatch: expected {expected}, got {actual}")
    return actual


def array_descriptor(np: Any, array: Any) -> dict[str, Any]:
    # np.ascontiguousarray promotes rank-zero tensors to rank one. Preserve
    # scalar shape because zero-point broadcast semantics depend on it.
    contiguous = np.asarray(array)
    if not contiguous.flags.c_contiguous:
        contiguous = np.ascontiguousarray(contiguous)
    return {
        "dtype": str(contiguous.dtype),
        "shape": list(contiguous.shape),
        "byte_length": contiguous.nbytes,
        "sha256": sha256_bytes(contiguous.tobytes(order="C")),
        "min": contiguous.min().item(),
        "max": contiguous.max().item(),
    }


def generate_fixture(
    model_path: Path,
    voices_path: Path,
    allow_version_mismatch: bool,
) -> dict[str, Any]:
    np, onnx, ort = require_tool_versions(allow_version_mismatch)
    from onnx import TensorProto, helper, numpy_helper

    model_hash = checked_source_hash(model_path, SOURCE_MODEL_SHA256, "model")
    voices_hash = checked_source_hash(voices_path, SOURCE_VOICES_SHA256, "voices")
    model = onnx.load(model_path)
    nodes = list(model.graph.node)
    try:
        node = next(candidate for candidate in nodes if candidate.name == TARGET_NODE)
    except StopIteration as error:
        raise ValueError(f"target node {TARGET_NODE!r} is missing") from error
    if nodes.index(node) != TARGET_GRAPH_INDEX or node.op_type != "ConvInteger":
        raise ValueError("target node index/type changed")

    attributes = {
        attribute.name: helper.get_attribute_value(attribute) for attribute in node.attribute
    }
    if attributes != EXPECTED_ATTRIBUTES:
        raise ValueError(f"target attributes changed: {attributes}")
    if len(node.input) != 4 or len(node.output) != 1:
        raise ValueError("expected four ConvInteger inputs and one output")

    initializers = {initializer.name: initializer for initializer in model.graph.initializer}
    weight = np.ascontiguousarray(numpy_helper.to_array(initializers[node.input[1]]))
    w_zero_point = np.asarray(numpy_helper.to_array(initializers[node.input[3]]))
    if weight.dtype != np.uint8 or weight.shape != (128, 128, 11):
        raise ValueError(f"unexpected weight tensor {weight.dtype} {weight.shape}")
    if w_zero_point.dtype != np.uint8 or w_zero_point.shape not in [(), (128,)]:
        raise ValueError(
            f"weight zero point must be scalar or per-output-channel u8, got "
            f"{w_zero_point.dtype} {w_zero_point.shape}"
        )

    activation_name, _, x_scale_name, x_zero_point_name = (
        node.input[0],
        node.input[1],
        node.input[0].removesuffix("_dynamic_quantized") + "_scale",
        node.input[2],
    )
    output_name = node.output[0]

    # Appending intermediates as graph outputs prevents optimizer removal and
    # lets ORT return the exact bytes presented to ConvInteger.
    capture_specs = [
        (activation_name, TensorProto.UINT8, [1, 128, "conv_length"]),
        (x_scale_name, TensorProto.FLOAT, []),
        (x_zero_point_name, TensorProto.UINT8, []),
        (output_name, TensorProto.INT32, [1, 128, "conv_length"]),
    ]
    for name, dtype, shape in capture_specs:
        model.graph.output.append(helper.make_tensor_value_info(name, dtype, shape))

    with np.load(voices_path, allow_pickle=False) as voices:
        style = np.asarray(voices[VOICE][STYLE_INDEX], dtype=np.float32).reshape(1, 256)
    tokens = np.asarray([PADDED_TOKEN_IDS], dtype=np.int64)
    speed = np.asarray([SPEED], dtype=np.float32)

    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_DISABLE_ALL
    options.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1

    with tempfile.TemporaryDirectory(prefix="ttstt-kokoro-conv-") as temp_dir:
        capture_model = Path(temp_dir) / "capture.onnx"
        onnx.save_model(model, capture_model)
        session = ort.InferenceSession(
            str(capture_model), sess_options=options, providers=["CPUExecutionProvider"]
        )
        activation, x_scale, x_zero_point, output = session.run(
            [name for name, _, _ in capture_specs],
            {"tokens": tokens, "style": style, "speed": speed},
        )

    activation = np.ascontiguousarray(activation)
    x_scale = np.asarray(x_scale)
    x_zero_point = np.asarray(x_zero_point)
    output = np.ascontiguousarray(output)
    if activation.dtype != np.uint8 or activation.ndim != 3:
        raise ValueError(f"unexpected activation {activation.dtype} {activation.shape}")
    if output.dtype != np.int32 or output.shape != activation.shape:
        raise ValueError(f"unexpected output {output.dtype} {output.shape}")
    if x_zero_point.dtype != np.uint8 or x_zero_point.shape != ():
        raise ValueError(f"unexpected x zero point {x_zero_point.dtype} {x_zero_point.shape}")

    _, input_channels, input_width = activation.shape
    output_channels, weight_input_channels, kernel_width = weight.shape
    if input_channels != weight_input_channels or output_channels != 128:
        raise ValueError("activation and weight channel dimensions disagree")
    dilation = int(attributes["dilations"][0])
    stride = int(attributes["strides"][0])
    pad_left, pad_right = [int(value) for value in attributes["pads"]]
    expected_output_width = (
        input_width + pad_left + pad_right - dilation * (kernel_width - 1) - 1
    ) // stride + 1
    if output.shape != (1, output_channels, expected_output_width):
        raise ValueError("ConvInteger output shape formula disagrees with ORT")

    positions = [0, expected_output_width // 2, expected_output_width - 1]
    x_zero_point_value = int(x_zero_point)
    patches = np.full(
        (len(positions), input_channels, kernel_width),
        x_zero_point_value,
        dtype=np.uint8,
    )
    valid_kernel_indices: list[list[int]] = []
    for position_index, output_position in enumerate(positions):
        valid = []
        for kernel_index in range(kernel_width):
            input_position = output_position * stride - pad_left + kernel_index * dilation
            if 0 <= input_position < input_width:
                patches[position_index, :, kernel_index] = activation[0, :, input_position]
                valid.append(kernel_index)
        valid_kernel_indices.append(valid)

    selected_weights = np.ascontiguousarray(weight[OUTPUT_CHANNELS, :, :])
    if w_zero_point.shape == ():
        selected_w_zero_points = np.full(
            len(OUTPUT_CHANNELS), int(w_zero_point), dtype=np.int64
        )
        w_zero_point_json = {
            "dtype": "uint8",
            "shape": [],
            "broadcast": "scalar",
            "value": int(w_zero_point),
        }
    else:
        selected_w_zero_points = w_zero_point[OUTPUT_CHANNELS].astype(np.int64)
        w_zero_point_json = {
            "dtype": "uint8",
            "shape": [output_channels],
            "broadcast": "per_output_channel",
            "selected_values": selected_w_zero_points.tolist(),
        }

    centered_patches = patches.astype(np.int64) - x_zero_point_value
    centered_weights = selected_weights.astype(np.int64) - selected_w_zero_points[:, None, None]
    manual = np.sum(
        centered_patches[:, None, :, :] * centered_weights[None, :, :, :],
        axis=(2, 3),
        dtype=np.int64,
    )
    ort_expected = output[0, OUTPUT_CHANNELS, :][:, positions].transpose()
    if not np.array_equal(manual, ort_expected.astype(np.int64)):
        raise ValueError(f"manual accumulator differs from ORT: {manual} vs {ort_expected}")
    if np.any(manual < np.iinfo(np.int32).min) or np.any(manual > np.iinfo(np.int32).max):
        raise ValueError("golden accumulator is outside int32 range")
    expected_i32 = np.ascontiguousarray(manual.astype("<i4"))

    patch_bytes = patches.tobytes(order="C")
    weight_bytes = selected_weights.tobytes(order="C")
    expected_bytes = expected_i32.tobytes(order="C")
    fixture = {
        "schema": SCHEMA,
        "source": {
            "model_file": "kokoro-quant-convinteger.onnx",
            "model_sha256": model_hash,
            "voices_file": "voices-v1.0.bin",
            "voices_sha256": voices_hash,
            "tools": {
                "numpy": np.__version__,
                "onnx": onnx.__version__,
                "onnxruntime": ort.__version__,
            },
            "ort_capture": {
                "graph_optimization": "disabled",
                "execution": "sequential",
                "intra_threads": 1,
                "inter_threads": 1,
                "provider": "CPUExecutionProvider",
            },
        },
        "inference_input": {
            "phonemes": PHONEMES,
            "token_ids_without_bos_eos": TOKEN_IDS,
            "tokens": {**array_descriptor(np, tokens), "values": PADDED_TOKEN_IDS},
            "voice": VOICE,
            "style_index": STYLE_INDEX,
            "style": array_descriptor(np, style.astype("<f4", copy=False)),
            "speed": SPEED,
        },
        "operator": {
            "name": TARGET_NODE,
            "graph_index": TARGET_GRAPH_INDEX,
            "op_type": "ConvInteger",
            "layout": {"input": "NCW", "weight": "MCK", "output": "NMW"},
            "inputs": list(node.input),
            "output": output_name,
            "attributes": {
                "auto_pad": "NOTSET",
                "dilations": [dilation],
                "group": int(attributes["group"]),
                "kernel_shape": [kernel_width],
                "pads": [pad_left, pad_right],
                "strides": [stride],
            },
            "activation": array_descriptor(np, activation),
            "weight": array_descriptor(np, weight),
            "x_scale_for_downstream_dequantization": array_descriptor(np, x_scale),
            "x_zero_point": {
                **array_descriptor(np, x_zero_point),
                "broadcast": "scalar",
                "value": x_zero_point_value,
            },
            "w_zero_point": w_zero_point_json,
            "accumulator_output": array_descriptor(np, output.astype("<i4", copy=False)),
            "equation": "sum_c,k ((x_q - x_zero_point) * (w_q - w_zero_point))",
            "padding_integer_value": x_zero_point_value,
        },
        "compact_fixture": {
            "description": (
                "Actual receptive fields for left edge, center and right edge; "
                "out-of-bounds taps are materialized as x_zero_point and therefore add zero."
            ),
            "positions": positions,
            "output_channels": OUTPUT_CHANNELS,
            "valid_kernel_indices": valid_kernel_indices,
            "patches": {
                "dtype": "uint8",
                "layout": "position,input_channel,kernel_index",
                "shape": list(patches.shape),
                "bytes": encode_bytes(patch_bytes),
            },
            "weights": {
                "dtype": "uint8",
                "layout": "selected_output_channel,input_channel,kernel_index",
                "shape": list(selected_weights.shape),
                "bytes": encode_bytes(weight_bytes),
            },
            "expected_i32": {
                "dtype": "int32-le",
                "layout": "position,selected_output_channel",
                "shape": list(expected_i32.shape),
                "values": expected_i32.tolist(),
                "bytes": encode_bytes(expected_bytes),
            },
            "tolerance": {"absolute": 0, "relative": 0, "require_bit_exact": True},
        },
    }
    return fixture


def parse_args() -> argparse.Namespace:
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Pinned generation environment:\n"
            "  python3 -m venv /tmp/ttstt-conv-oracle-venv\n"
            "  /tmp/ttstt-conv-oracle-venv/bin/pip install "
            "numpy==2.5.1 onnx==1.22.0 onnxruntime==1.28.0"
        ),
    )
    parser.add_argument(
        "--model",
        type=Path,
        default=repo / ".ttstt/models/kokoro/kokoro-quant-convinteger.onnx",
    )
    parser.add_argument(
        "--voices",
        type=Path,
        default=repo.parent / "ttstt/models/kokoro/voices-v1.0.bin",
    )
    parser.add_argument("--output", type=Path, help="write a generated fixture")
    parser.add_argument(
        "--check",
        type=Path,
        help="regenerate and require byte-identical canonical JSON",
    )
    parser.add_argument(
        "--verify-fixture",
        type=Path,
        help="verify encoded hashes and accumulators without ONNX dependencies",
    )
    parser.add_argument("--force", action="store_true", help="replace --output")
    parser.add_argument(
        "--allow-tool-version-mismatch",
        action="store_true",
        help="permit generation with versions other than PINNED_TOOLS",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.verify_fixture is not None:
        if args.output is not None or args.check is not None:
            raise SystemExit("--verify-fixture cannot be combined with --output or --check")
        verify_fixture(args.verify_fixture)
        return 0

    fixture = generate_fixture(args.model, args.voices, args.allow_tool_version_mismatch)
    encoded = canonical_json(fixture)
    generated_hash = sha256_bytes(encoded)

    if args.check is not None:
        expected = args.check.read_bytes()
        if encoded != expected:
            raise SystemExit(
                f"generated oracle differs from {args.check}; "
                f"generated_sha256={generated_hash} expected_sha256={sha256_bytes(expected)}"
            )
        print(f"reproduced {args.check}: fixture_sha256={generated_hash}")

    if args.output is not None:
        if args.output.exists() and not args.force:
            raise SystemExit(f"refusing to replace {args.output}; pass --force")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(encoded)
        print(f"wrote {args.output}: fixture_sha256={generated_hash}")

    if args.output is None and args.check is None:
        print(f"generated fixture_sha256={generated_hash} (use --output or --check)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1) from error
