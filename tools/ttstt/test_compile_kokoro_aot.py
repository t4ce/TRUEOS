#!/usr/bin/env python3
"""Focused tests for the Kokoro AOT analyzer and v1 artifact writer."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
TOOL_PATH = ROOT / "tools/ttstt/compile_kokoro_aot.py"
SPEC = importlib.util.spec_from_file_location("compile_kokoro_aot", TOOL_PATH)
assert SPEC is not None and SPEC.loader is not None
TOOL = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = TOOL
SPEC.loader.exec_module(TOOL)

MODEL = ROOT / "crates/ttstt/.ttstt/models/kokoro/kokoro-rten.onnx"
VOICES = ROOT / "crates/ttstt/.ttstt/models/kokoro/voices-v1.0.bin"
RUST_MANIFEST = ROOT / "crates/trueos-kokoro-aot/Cargo.toml"
RUST_INSPECTOR = ROOT / "crates/trueos-kokoro-aot/examples/inspect.rs"
HAS_ONNX = importlib.util.find_spec("onnx") is not None


class KokoroAotFixtureTests(unittest.TestCase):
    def test_fixture_is_canonical_and_deterministic(self) -> None:
        first = TOOL.synthetic_fixture_artifact()
        second = TOOL.synthetic_fixture_artifact()
        self.assertEqual(first, second)
        self.assertEqual(len(first), 1_651)
        self.assertEqual(
            TOOL.hashlib.sha256(first).hexdigest(),
            "dbe984a0f685a71bc70a06188abdee60254789c4a7890db8637d1ef8605fc485",
        )

        inspected = TOOL.inspect_aot(first)
        self.assertEqual(
            inspected["sections"],
            {
                "tensors": 7,
                "slots": 2,
                "ops": 3,
                "bindings": 8,
                "phases": 2,
                "data": 19,
            },
        )
        self.assertEqual(inspected["model_sha256"], TOOL.PINNED_MODEL_SHA256)
        self.assertEqual(inspected["voices_sha256"], TOOL.PINNED_VOICES_SHA256)
        self.assertEqual(
            inspected["artifact_sha256"],
            "0df8861b0d55f3a1d8587b0993a5588b098800c9c3006d080c19e6b90ad8df44",
        )

    def test_fixture_payload_tamper_is_rejected(self) -> None:
        artifact = bytearray(TOOL.synthetic_fixture_artifact())
        artifact[-1] ^= 1
        with self.assertRaisesRegex(TOOL.CompileError, "artifact seal"):
            TOOL.inspect_aot(bytes(artifact))

    def test_fixture_provenance_and_directory_tamper_are_sealed(self) -> None:
        for offset in (96, 128, 176):
            with self.subTest(offset=offset):
                artifact = bytearray(TOOL.synthetic_fixture_artifact())
                artifact[offset] ^= 1
                with self.assertRaisesRegex(TOOL.CompileError, "artifact seal"):
                    TOOL.inspect_aot(bytes(artifact))

    def test_fixture_reserved_header_tamper_is_rejected(self) -> None:
        artifact = bytearray(TOOL.synthetic_fixture_artifact())
        artifact[63] = 1
        with self.assertRaisesRegex(TOOL.CompileError, "reserved"):
            TOOL.inspect_aot(bytes(artifact))

    def test_atomic_writer_refuses_implicit_replacement(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.kkaot"
            TOOL.write_atomic(path, b"first", False)
            with self.assertRaisesRegex(TOOL.CompileError, "destination exists"):
                TOOL.write_atomic(path, b"second", False)
            self.assertEqual(path.read_bytes(), b"first")

    @unittest.skipUnless(
        shutil.which("cargo") and RUST_MANIFEST.is_file() and RUST_INSPECTOR.is_file(),
        "Rust cross-language inspector is not available",
    )
    def test_rust_parser_round_trip(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "fixture.kkaot"
            artifact.write_bytes(TOOL.synthetic_fixture_artifact())
            completed = subprocess.run(
                [
                    "cargo",
                    "run",
                    "--quiet",
                    "--manifest-path",
                    str(RUST_MANIFEST),
                    "--example",
                    "inspect",
                    "--",
                    str(artifact),
                    "16",
                ],
                # Running outside the checkout intentionally avoids the kernel
                # workspace's build-std/custom-target Cargo configuration.
                cwd="/tmp",
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn(
                "artifact_sha256=0df8861b0d55f3a1d8587b0993a5588b098800c9c3006d080c19e6b90ad8df44",
                completed.stdout,
            )
            self.assertIn("resolved_frame_count=16", completed.stdout)
            self.assertIn("resolved_arena_bytes=128", completed.stdout)


@unittest.skipUnless(
    HAS_ONNX and MODEL.is_file() and VOICES.is_file(),
    "pinned local model and optional ONNX tooling are not installed",
)
class KokoroPinnedGraphTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.analysis = TOOL.analyze(MODEL, VOICES)

    def test_real_graph_analysis_contract(self) -> None:
        report = self.analysis.report
        self.assertEqual(report["result"], "accepted")
        self.assertEqual(report["graph"]["nodes"], 3_615)
        self.assertEqual(report["tensor_contract"]["max_rank"], 4)
        self.assertEqual(report["quantized_lowering"]["dynamic_quantize_linear"], 139)
        self.assertEqual(report["quantized_lowering"]["matmul_recognized"], 148)
        self.assertEqual(report["quantized_lowering"]["conv_recognized"], 87)
        self.assertEqual(report["quantized_lowering"]["conv_int32_bias"], 80)
        self.assertEqual(report["quantized_lowering"]["conv_direct"], 7)
        self.assertEqual(report["phases"]["phase0_source_nodes"], [0, 1_747])
        self.assertEqual(report["phases"]["phase1_source_nodes"], [1_747, 3_615])

    def test_model_hash_change_is_rejected(self) -> None:
        with self.assertRaisesRegex(TOOL.CompileError, "SHA-256"):
            TOOL.validate_pinned_model(
                self.analysis.onnx,
                self.analysis.model,
                self.analysis.model_bytes,
                "00" * 32,
            )

    def test_quant_tuple_change_is_rejected(self) -> None:
        kernel = self.analysis.model.graph.node[11]
        original = kernel.input[2]
        kernel.input[2] = kernel.input[3]
        try:
            with self.assertRaisesRegex(TOOL.CompileError, "activation quant tuple"):
                TOOL.recognize_quant_fusions(
                    self.analysis.onnx,
                    self.analysis.model.graph,
                    self.analysis.producers,
                    self.analysis.consumers,
                    self.analysis.initializers,
                    self.analysis.dtypes,
                )
        finally:
            kernel.input[2] = original


if __name__ == "__main__":
    unittest.main()
