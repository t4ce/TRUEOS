#!/usr/bin/env python3
"""Generate the checked Kokoro LSTM semantic fixtures with ONNX Runtime.

The production fixture provenance is ONNX Runtime 1.27.0 using only the CPU
execution provider. `--verify` regenerates results in memory and compares them
with an existing fixture, which is also useful for checking another runtime.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
from onnx import TensorProto, helper, numpy_helper


HIDDEN = 256
DIRECTIONS = 2
GATES = 4
GATE_ELEMENTS = GATES * HIDDEN
MAGIC = b"KORLSTM1"
FORMAT_VERSION = 1
HEADER = struct.Struct("<8s6I")
CASES = (
    ("text512_t3", 3, 512),
    ("prosody640_t2", 2, 640),
)


def fixture_tensors(sequence: int, width: int) -> tuple[np.ndarray, ...]:
    """Create exact-power-of-two test values shared with the Rust tests."""
    x = np.zeros((sequence, 1, width), dtype=np.float32)
    for time in range(sequence):
        for channel in range(width):
            raw = (time * 19 + channel * 7 + 3) % 29 - 14
            x[time, 0, channel] = np.float32(raw / 16.0)

    w = np.zeros((DIRECTIONS, GATE_ELEMENTS, width), dtype=np.float32)
    r = np.zeros((DIRECTIONS, GATE_ELEMENTS, HIDDEN), dtype=np.float32)
    b = np.zeros((DIRECTIONS, 2 * GATE_ELEMENTS), dtype=np.float32)

    for direction in range(DIRECTIONS):
        for gate in range(GATES):
            for hidden in range(HIDDEN):
                row = gate * HIDDEN + hidden

                input_a = (hidden * 17 + gate * 29 + direction * 31) % width
                input_b = (hidden * 43 + gate * 11 + direction * 7 + 5) % width
                if input_b == input_a:
                    input_b = (input_b + 1) % width
                raw_a = (hidden * 3 + gate * 5 + direction * 7) % 9 - 4
                raw_b = (hidden * 5 + gate * 7 + direction * 2 + 1) % 7 - 3
                w[direction, row, input_a] = np.float32(raw_a / 16.0)
                w[direction, row, input_b] = np.float32(raw_b / 32.0)

                recurrent_a = hidden
                recurrent_b = (hidden + 1 + gate * 13 + direction * 3) % HIDDEN
                if recurrent_b == recurrent_a:
                    recurrent_b = (recurrent_b + 1) % HIDDEN
                recurrent_raw_a = (
                    hidden * 11 + gate * 3 + direction * 5 + 2
                ) % 7 - 3
                recurrent_raw_b = (
                    hidden * 7 + gate * 5 + direction * 11 + 1
                ) % 5 - 2
                r[direction, row, recurrent_a] = np.float32(
                    recurrent_raw_a / 32.0
                )
                r[direction, row, recurrent_b] = np.float32(
                    recurrent_raw_b / 64.0
                )

                wb_raw = (hidden * 13 + gate * 17 + direction * 19 + 4) % 17 - 8
                rb_raw = (hidden * 23 + gate * 7 + direction * 3 + 6) % 13 - 6
                b[direction, row] = np.float32(wb_raw / 64.0)
                b[direction, GATE_ELEMENTS + row] = np.float32(rb_raw / 64.0)

    return x, w, r, b


def run_ort(sequence: int, width: int) -> tuple[np.ndarray, ...]:
    x, w, r, b = fixture_tensors(sequence, width)
    node = helper.make_node(
        "LSTM",
        ["X", "W", "R", "B"],
        ["Y", "Y_h", "Y_c"],
        direction="bidirectional",
        hidden_size=HIDDEN,
        input_forget=0,
    )
    graph = helper.make_graph(
        [node],
        "trueos_kokoro_lstm_oracle",
        [helper.make_tensor_value_info("X", TensorProto.FLOAT, [sequence, 1, width])],
        [
            helper.make_tensor_value_info(
                "Y", TensorProto.FLOAT, [sequence, DIRECTIONS, 1, HIDDEN]
            ),
            helper.make_tensor_value_info(
                "Y_h", TensorProto.FLOAT, [DIRECTIONS, 1, HIDDEN]
            ),
            helper.make_tensor_value_info(
                "Y_c", TensorProto.FLOAT, [DIRECTIONS, 1, HIDDEN]
            ),
        ],
        [
            numpy_helper.from_array(w, "W"),
            numpy_helper.from_array(r, "R"),
            numpy_helper.from_array(b, "B"),
        ],
    )
    model = helper.make_model(
        graph,
        ir_version=10,
        opset_imports=[helper.make_opsetid("", 20)],
        producer_name="trueos-kokoro-lstm-fixture",
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
    y, y_h, y_c = session.run(None, {"X": x})
    return (
        np.asarray(y, dtype="<f4").reshape(-1),
        np.asarray(y_h, dtype="<f4").reshape(-1),
        np.asarray(y_c, dtype="<f4").reshape(-1),
    )


def encode_fixture(
    sequence: int, width: int, outputs: tuple[np.ndarray, ...]
) -> bytes:
    y, y_h, y_c = outputs
    header = HEADER.pack(
        MAGIC,
        FORMAT_VERSION,
        sequence,
        width,
        y.size,
        y_h.size,
        y_c.size,
    )
    return header + y.tobytes() + y_h.tobytes() + y_c.tobytes()


def decode_fixture(blob: bytes) -> tuple[tuple[int, int], tuple[np.ndarray, ...]]:
    if len(blob) < HEADER.size:
        raise ValueError("fixture is shorter than its header")
    magic, version, sequence, width, y_len, yh_len, yc_len = HEADER.unpack_from(blob)
    if magic != MAGIC or version != FORMAT_VERSION:
        raise ValueError("unsupported fixture header")
    counts = (y_len, yh_len, yc_len)
    expected_bytes = HEADER.size + 4 * sum(counts)
    if len(blob) != expected_bytes:
        raise ValueError("fixture payload length mismatch")
    arrays = []
    offset = HEADER.size
    for count in counts:
        arrays.append(np.frombuffer(blob, dtype="<f4", count=count, offset=offset))
        offset += 4 * count
    return (sequence, width), tuple(arrays)


def write_case(directory: Path, name: str, sequence: int, width: int) -> None:
    outputs = run_ort(sequence, width)
    blob = encode_fixture(sequence, width, outputs)
    fixture_path = directory / f"{name}.bin"
    fixture_path.write_bytes(blob)
    metadata = {
        "schema": "trueos-kokoro-lstm-ort-fixture-v1",
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
        "node": {
            "op_type": "LSTM",
            "direction": "bidirectional",
            "hidden_size": HIDDEN,
            "input_forget": 0,
            "layout": 0,
            "activations": ["Sigmoid", "Tanh", "Tanh"],
            "batch": 1,
            "sequence": sequence,
            "input_width": width,
            "initial_h_c": "implicit zero",
            "peepholes": "absent",
            "sequence_lens": "absent",
            "gate_order": "IOFC",
        },
        "payload": {
            "endianness": "little",
            "dtype": "float32",
            "arrays": ["Y", "Y_h", "Y_c"],
        },
    }
    (directory / f"{name}.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(
        f"wrote {fixture_path}: ORT {ort.__version__}, "
        f"sha256={metadata['sha256']}"
    )


def verify_case(directory: Path, name: str, sequence: int, width: int) -> None:
    fixture_path = directory / f"{name}.bin"
    shape, expected = decode_fixture(fixture_path.read_bytes())
    if shape != (sequence, width):
        raise AssertionError(f"fixture shape {shape} != {(sequence, width)}")
    actual = run_ort(sequence, width)
    for array_name, candidate, reference in zip(("Y", "Y_h", "Y_c"), actual, expected):
        difference = np.abs(candidate.astype(np.float64) - reference.astype(np.float64))
        maximum = float(difference.max(initial=0.0))
        if not np.allclose(candidate, reference, rtol=2e-6, atol=2e-7):
            raise AssertionError(f"{name} {array_name} max_abs={maximum}")
        exact = int(np.count_nonzero(candidate.view(np.uint32) == reference.view(np.uint32)))
        print(
            f"verified {name} {array_name}: ORT {ort.__version__}, "
            f"max_abs={maximum:.9g}, bit_exact={exact}/{candidate.size}"
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--verify",
        action="store_true",
        help="compare this runtime against existing fixtures instead of writing",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "tests" / "fixtures",
    )
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    for name, sequence, width in CASES:
        if args.verify:
            verify_case(args.output_dir, name, sequence, width)
        else:
            write_case(args.output_dir, name, sequence, width)


if __name__ == "__main__":
    main()
