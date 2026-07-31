#!/usr/bin/env python3
"""Audit the pinned STFT and generate stable ONNX Runtime fixtures."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
from onnx import TensorProto, helper, numpy_helper


FRAME_STEP = 5
FRAME_LENGTH = 20
OUTPUT_BINS = 11
MAGIC = b"KORSTFT1"
FORMAT_VERSION = 1
HEADER = struct.Struct("<8s5I")
PINNED_NODE_INDEX = 2159
PINNED_NODE_NAME = "/decoder/decoder/generator/STFT"
PINNED_MODEL_SHA256 = "239d9f4df112a375bea52146570b97eb5c5af727c007761ee121ed123fd1ab29"

WINDOW_BITS = (
    0x00000000,
    0x3CC878F6,
    0x3DC3910D,
    0x3E530DD0,
    0x3EB0E443,
    0x3F000000,
    0x3F278DDE,
    0x3F4B3C8C,
    0x3F678DDE,
    0x3F79BC38,
    0x3F800000,
    0x3F79BC38,
    0x3F678DDE,
    0x3F4B3C8C,
    0x3F278DDE,
    0x3F000000,
    0x3EB0E443,
    0x3E530DD0,
    0x3DC3910D,
    0x3CC878F6,
)
COS_BITS = (
    0x3F800000,
    0x3F737871,
    0x3F4F1BBD,
    0x3F167918,
    0x3E9E377A,
    0x00000000,
    0xBE9E377A,
    0xBF167918,
    0xBF4F1BBD,
    0xBF737871,
    0xBF800000,
    0xBF737871,
    0xBF4F1BBD,
    0xBF167918,
    0xBE9E377A,
    0x00000000,
    0x3E9E377A,
    0x3F167918,
    0x3F4F1BBD,
    0x3F737871,
)
NEG_SIN_BITS = (
    0x00000000,
    0xBE9E377A,
    0xBF167918,
    0xBF4F1BBD,
    0xBF737871,
    0xBF800000,
    0xBF737871,
    0xBF4F1BBD,
    0xBF167918,
    0xBE9E377A,
    0x00000000,
    0x3E9E377A,
    0x3F167918,
    0x3F4F1BBD,
    0x3F737871,
    0x3F800000,
    0x3F737871,
    0x3F4F1BBD,
    0x3F167918,
    0x3E9E377A,
)
CASES = (
    ("b1_l20_minimum", 1, 20),
    ("b2_l24_incomplete_tail", 2, 24),
    ("b2_l25_second_frame", 2, 25),
)


def floats_from_bits(bits: tuple[int, ...]) -> np.ndarray:
    return np.asarray(bits, dtype=np.uint32).view(np.float32)


def audit_root_tables() -> None:
    """Verify baked roots against nearest f32 mathematical values."""
    expected_cos = []
    expected_neg_sin = []
    for index in range(FRAME_LENGTH):
        angle = 2.0 * math.pi * index / FRAME_LENGTH
        cosine = math.cos(angle)
        negative_sine = -math.sin(angle)
        if abs(cosine) < 1.0e-12:
            cosine = 0.0
        if abs(negative_sine) < 1.0e-12:
            negative_sine = 0.0
        expected_cos.append(np.float32(cosine).view(np.uint32).item())
        expected_neg_sin.append(np.float32(negative_sine).view(np.uint32).item())
    if tuple(expected_cos) != COS_BITS:
        raise AssertionError("baked cosine table does not match nearest f32 roots")
    if tuple(expected_neg_sin) != NEG_SIN_BITS:
        raise AssertionError("baked negative-sine table does not match nearest f32 roots")


def audit_pinned_model(model_path: Path) -> None:
    digest = hashlib.sha256(model_path.read_bytes()).hexdigest()
    if digest != PINNED_MODEL_SHA256:
        raise AssertionError(
            f"pinned model SHA-256 changed: expected {PINNED_MODEL_SHA256}, got {digest}"
        )

    model = onnx.load(model_path)
    stft_nodes = [(index, node) for index, node in enumerate(model.graph.node) if node.op_type == "STFT"]
    if len(stft_nodes) != 1:
        raise AssertionError(f"expected one STFT node, found {len(stft_nodes)}")
    node_index, node = stft_nodes[0]
    if node_index != PINNED_NODE_INDEX or node.name != PINNED_NODE_NAME:
        raise AssertionError(f"unexpected STFT identity: {node_index} {node.name!r}")
    attributes = {
        attribute.name: helper.get_attribute_value(attribute)
        for attribute in node.attribute
    }
    if attributes != {"onesided": 1}:
        raise AssertionError(f"unexpected STFT attributes: {attributes!r}")
    if len(node.input) != 4:
        raise AssertionError("pinned STFT must have four inputs")

    initializers = {initializer.name: initializer for initializer in model.graph.initializer}
    frame_step = numpy_helper.to_array(initializers[node.input[1]])
    window = numpy_helper.to_array(initializers[node.input[2]])
    frame_length = numpy_helper.to_array(initializers[node.input[3]])
    if frame_step.shape != () or frame_step.dtype != np.int64 or int(frame_step) != FRAME_STEP:
        raise AssertionError("pinned frame_step is not scalar INT64 5")
    if frame_length.shape != () or frame_length.dtype != np.int64 or int(frame_length) != FRAME_LENGTH:
        raise AssertionError("pinned frame_length is not scalar INT64 20")
    if window.shape != (FRAME_LENGTH,) or window.dtype != np.float32:
        raise AssertionError("pinned window is not FLOAT[20]")
    actual_window_bits = tuple(int(value) for value in window.view(np.uint32))
    if actual_window_bits != WINDOW_BITS:
        raise AssertionError("pinned Hann window bits changed")


def fixture_input(batch: int, length: int) -> np.ndarray:
    signal = np.empty((batch, length), dtype=np.float32)
    for batch_index in range(batch):
        for sample in range(length):
            raw = (batch_index * 37 + sample * 11 + 5) % 41 - 20
            signal[batch_index, sample] = np.float32(raw / 8.0)
    return signal


def run_ort(batch: int, length: int) -> np.ndarray:
    frames = 1 + (length - FRAME_LENGTH) // FRAME_STEP
    signal = fixture_input(batch, length)
    node = helper.make_node(
        "STFT",
        ["signal", "frame_step", "window", "frame_length"],
        ["output"],
        onesided=1,
    )
    graph = helper.make_graph(
        [node],
        "trueos_kokoro_stft_oracle",
        [helper.make_tensor_value_info("signal", TensorProto.FLOAT, [batch, length])],
        [
            helper.make_tensor_value_info(
                "output", TensorProto.FLOAT, [batch, frames, OUTPUT_BINS, 2]
            )
        ],
        [
            numpy_helper.from_array(np.asarray(FRAME_STEP, dtype=np.int64), "frame_step"),
            numpy_helper.from_array(floats_from_bits(WINDOW_BITS), "window"),
            numpy_helper.from_array(np.asarray(FRAME_LENGTH, dtype=np.int64), "frame_length"),
        ],
    )
    model = helper.make_model(
        graph,
        ir_version=10,
        opset_imports=[helper.make_opsetid("", 20)],
        producer_name="trueos-kokoro-stft-fixture",
    )
    onnx.checker.check_model(model)

    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_DISABLE_ALL
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1
    session = ort.InferenceSession(
        model.SerializeToString(),
        sess_options=options,
        providers=["CPUExecutionProvider"],
    )
    (output,) = session.run(None, {"signal": signal})
    expected_shape = (batch, frames, OUTPUT_BINS, 2)
    if output.shape != expected_shape:
        raise AssertionError(f"ORT output {output.shape} != {expected_shape}")
    return np.asarray(output, dtype="<f4").reshape(-1)


def encode_fixture(batch: int, length: int, output: np.ndarray) -> bytes:
    frames = 1 + (length - FRAME_LENGTH) // FRAME_STEP
    return HEADER.pack(
        MAGIC,
        FORMAT_VERSION,
        batch,
        length,
        frames,
        output.size,
    ) + output.tobytes()


def decode_fixture(blob: bytes) -> tuple[tuple[int, int, int], np.ndarray]:
    if len(blob) < HEADER.size:
        raise ValueError("fixture is shorter than its header")
    magic, version, batch, length, frames, output_len = HEADER.unpack_from(blob)
    if magic != MAGIC or version != FORMAT_VERSION:
        raise ValueError("unsupported fixture header")
    if len(blob) != HEADER.size + 4 * output_len:
        raise ValueError("fixture payload length mismatch")
    output = np.frombuffer(blob, dtype="<f4", count=output_len, offset=HEADER.size)
    return (batch, length, frames), output


def coefficient_digest() -> str:
    packed = b"".join(
        struct.pack("<I", value)
        for table in (WINDOW_BITS, COS_BITS, NEG_SIN_BITS)
        for value in table
    )
    return hashlib.sha256(packed).hexdigest()


def write_case(directory: Path, name: str, batch: int, length: int) -> None:
    output = run_ort(batch, length)
    blob = encode_fixture(batch, length, output)
    fixture_path = directory / f"{name}.bin"
    fixture_path.write_bytes(blob)
    frames = 1 + (length - FRAME_LENGTH) // FRAME_STEP
    metadata = {
        "schema": "trueos-kokoro-stft-ort-fixture-v1",
        "fixture": fixture_path.name,
        "sha256": hashlib.sha256(blob).hexdigest(),
        "onnxruntime_version": ort.__version__,
        "onnx_version": onnx.__version__,
        "execution_provider": "CPUExecutionProvider",
        "graph_optimization": "ORT_DISABLE_ALL",
        "intra_op_threads": 1,
        "inter_op_threads": 1,
        "opset": 20,
        "ir_version": 10,
        "pinned_model": {
            "sha256": PINNED_MODEL_SHA256,
            "node_index": PINNED_NODE_INDEX,
            "node_name": PINNED_NODE_NAME,
        },
        "node": {
            "op_type": "STFT",
            "input_shape": [batch, length],
            "frame_step": FRAME_STEP,
            "frame_length": FRAME_LENGTH,
            "window_bits": [f"0x{value:08x}" for value in WINDOW_BITS],
            "onesided": 1,
            "implicit_padding": False,
            "output_shape": [batch, frames, OUTPUT_BINS, 2],
            "component_order": ["real", "imaginary"],
            "forward_imaginary_sign": "negative sine",
        },
        "root_tables": {
            "coefficient_sha256": coefficient_digest(),
            "cos_bits": [f"0x{value:08x}" for value in COS_BITS],
            "negative_sin_bits": [f"0x{value:08x}" for value in NEG_SIN_BITS],
        },
        "payload": {
            "endianness": "little",
            "dtype": "float32",
            "array": "output",
        },
    }
    (directory / f"{name}.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(
        f"wrote {fixture_path}: ORT {ort.__version__}, "
        f"sha256={metadata['sha256']}"
    )


def verify_case(directory: Path, name: str, batch: int, length: int) -> None:
    fixture_path = directory / f"{name}.bin"
    shape, expected = decode_fixture(fixture_path.read_bytes())
    frames = 1 + (length - FRAME_LENGTH) // FRAME_STEP
    if shape != (batch, length, frames):
        raise AssertionError(f"fixture shape {shape} != {(batch, length, frames)}")
    actual = run_ort(batch, length)
    difference = np.abs(actual.astype(np.float64) - expected.astype(np.float64))
    maximum = float(difference.max(initial=0.0))
    if not np.allclose(actual, expected, rtol=2.0e-6, atol=2.0e-7):
        raise AssertionError(f"{name} max_abs={maximum}")
    exact = int(np.count_nonzero(actual.view(np.uint32) == expected.view(np.uint32)))
    print(
        f"verified {name}: ORT {ort.__version__}, max_abs={maximum:.9g}, "
        f"bit_exact={exact}/{actual.size}"
    )


def main() -> None:
    repo_root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--verify",
        action="store_true",
        help="compare this runtime against existing fixtures instead of writing",
    )
    parser.add_argument(
        "--model",
        type=Path,
        default=repo_root / "crates/ttstt/.ttstt/models/kokoro/kokoro-rten.onnx",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "tests" / "fixtures",
    )
    args = parser.parse_args()

    audit_root_tables()
    audit_pinned_model(args.model)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for name, batch, length in CASES:
        if args.verify:
            verify_case(args.output_dir, name, batch, length)
        else:
            write_case(args.output_dir, name, batch, length)


if __name__ == "__main__":
    main()
