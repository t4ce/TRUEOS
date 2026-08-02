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

MODEL = ROOT / "tools/ttstt/models/kokoro/kokoro-rten.onnx"
VOICES = ROOT / "tools/ttstt/models/kokoro/voices-v1.0.bin"
RUST_MANIFEST = ROOT / "crates/trueos-kokoro-aot/Cargo.toml"
RUST_INSPECTOR = ROOT / "crates/trueos-kokoro-aot/examples/inspect.rs"
RUST_TARGET_DIR = ROOT / "target/kokoro-aot-host-test"
TEST_SCRATCH_ROOT = ROOT / "target/kokoro-aot-host-test-scratch"
HAS_ONNX = importlib.util.find_spec("onnx") is not None


def test_scratch_directory() -> tempfile.TemporaryDirectory[str]:
    """Keep large real-model fixtures off small or quota-limited /tmp mounts."""

    TEST_SCRATCH_ROOT.mkdir(parents=True, exist_ok=True)
    return tempfile.TemporaryDirectory(dir=TEST_SCRATCH_ROOT)


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

    def test_attribute_fixture_is_canonical_and_deterministic(self) -> None:
        first = TOOL.synthetic_attribute_fixture_artifact()
        second = TOOL.synthetic_attribute_fixture_artifact()
        self.assertEqual(first, second)
        self.assertEqual(len(first), 27_500)
        self.assertEqual(
            TOOL.hashlib.sha256(first).hexdigest(),
            "76b3b6f833b7cdbf4933b0985ad081ebe7b543b21d6d7c68ca9ff3ad19b603e9",
        )
        inspected = TOOL.inspect_aot(first)
        self.assertEqual(
            inspected["sections"],
            {
                "tensors": 178,
                "slots": 0,
                "ops": 56,
                "bindings": 178,
                "phases": 2,
                "data": 1_308,
            },
        )
        self.assertEqual(inspected["attribute_abi"]["version"], 1)
        self.assertEqual(inspected["attribute_abi"]["records"], 56)
        self.assertEqual(
            inspected["artifact_sha256"],
            "52b9d668b82cf015259ae8ecbf3001d6719048ffdce15b04f642b4f009480a0d",
        )
        kind_counts = inspected["attribute_abi"]["kind_counts"]
        for op_type in (
            "ResolveDecoderShape",
            "DynamicQuantizedGemm",
            "DynamicQuantizedConv1d",
            "BiLstm256",
            "FloatConv1d",
            "FloatConvTranspose1d",
            "FixedStft20",
        ):
            self.assertEqual(kind_counts[f"0x{TOOL.AOT_OPCODES[op_type]:04x}"], 1)

    def test_attribute_records_fail_closed(self) -> None:
        record = TOOL.binary_attribute("Add", 3, 0, 3)
        decoded = TOOL.inspect_attribute_record(record, TOOL.AOT_OPCODES["Add"])
        self.assertEqual(decoded["version"], 1)
        self.assertEqual(decoded["bytes"], 16)

        corruptions = {
            "version": (0, 2, "version"),
            "byte count": (4, 12, "byte count"),
            "reserved": (15, 1, "reserved"),
        }
        for name, (offset, value, message) in corruptions.items():
            with self.subTest(name=name):
                corrupt = bytearray(record)
                corrupt[offset] = value
                with self.assertRaisesRegex(TOOL.CompileError, message):
                    TOOL.inspect_attribute_record(bytes(corrupt), TOOL.AOT_OPCODES["Add"])
        with self.assertRaisesRegex(TOOL.CompileError, "kind"):
            TOOL.inspect_attribute_record(record, TOOL.AOT_OPCODES["Mul"])
        with self.assertRaisesRegex(TOOL.CompileError, "Unsqueeze axes"):
            TOOL.inspect_attribute_record(
                TOOL.view_attribute(
                    "Unsqueeze",
                    2,
                    4,
                    1,
                    static_control=True,
                    parameters=(1, 1),
                )
            )
        with self.assertRaisesRegex(TOOL.CompileError, "Reshape parameters"):
            TOOL.inspect_attribute_record(
                TOOL.view_attribute(
                    "Reshape",
                    2,
                    2,
                    1,
                    static_control=True,
                    parameters=(-1, -1),
                )
            )
        with self.assertRaisesRegex(TOOL.CompileError, "Transpose contract"):
            TOOL.inspect_attribute_record(
                TOOL.transpose_attribute((0, 0), 2, 1)
            )
        with self.assertRaisesRegex(TOOL.CompileError, "Slice step"):
            TOOL.inspect_attribute_record(
                TOOL.slice_attribute(
                    3,
                    3,
                    1,
                    4,
                    True,
                    True,
                    (
                        TOOL.ATTRIBUTE_CONTROL_INITIALIZER,
                        TOOL.ATTRIBUTE_CONTROL_INITIALIZER,
                        TOOL.ATTRIBUTE_CONTROL_INITIALIZER,
                        TOOL.ATTRIBUTE_CONTROL_INITIALIZER,
                    ),
                    (0, 2, 1, -1),
                    (0, 0, 0, 0),
                )
            )
        scatter = bytearray(TOOL.scatter_nd_attribute(2, 3, 2, 2, 1, 2))
        scatter[15] = 0
        with self.assertRaisesRegex(TOOL.CompileError, "ScatterND contract"):
            TOOL.inspect_attribute_record(bytes(scatter))
        with self.assertRaisesRegex(TOOL.CompileError, "Cast contract"):
            TOOL.inspect_attribute_record(TOOL.cast_attribute(1, 1, 1, 7, 1))
        with self.assertRaisesRegex(TOOL.CompileError, "Pow contract"):
            TOOL.inspect_attribute_record(TOOL.pow_attribute(TOOL.f32_bits(3.0), 3, 3, 1, 0))
        with self.assertRaisesRegex(TOOL.CompileError, "DynamicQuantizedGemm"):
            TOOL.inspect_attribute_record(
                TOOL.quant_gemm_attribute(1, 3, 3, TOOL.ATTRIBUTE_BIAS_QUANTIZED_INT32, 128, 256, 6)
            )

    def test_lowering_source_ownership_is_canonical(self) -> None:
        record = TOOL.LoweringRecord(
            3,
            "Add",
            TOOL.AOT_OPCODES["Add"],
            0,
            (1, 2),
            (3,),
            TOOL.binary_attribute("Add", 1, 1, 1),
            "fixture",
            (1, 2, 3),
        )
        encoded = record.canonical_bytes()
        self.assertEqual(encoded[7], 3)
        self.assertTrue(TOOL.lowering_plan_bytes((record,)).startswith(b"KKLOWER2"))
        with self.assertRaisesRegex(TOOL.CompileError, "source ownership"):
            TOOL.LoweringRecord(
                3,
                "Add",
                TOOL.AOT_OPCODES["Add"],
                0,
                (1, 2),
                (3,),
                TOOL.binary_attribute("Add", 1, 1, 1),
                "fixture",
                (1, 3, 3),
            ).canonical_bytes()

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
        with test_scratch_directory() as directory:
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
        with test_scratch_directory() as directory:
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
        shutil.which("cargo") and RUST_MANIFEST.is_file() and RUST_INSPECTOR.is_file(),
        "Rust cross-language inspector is not available",
    )
    def test_rust_parser_accepts_attribute_fixture(self) -> None:
        with test_scratch_directory() as directory:
            artifact = Path(directory) / "attributes.kkaot"
            artifact.write_bytes(TOOL.synthetic_attribute_fixture_artifact())
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
                ],
                cwd="/tmp",
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
            self.assertEqual(completed.returncode, 0, completed.stdout)
            self.assertIn("section[2]=Ops", completed.stdout)
            self.assertIn("count=56", completed.stdout)
            self.assertIn(
                "artifact_sha256=52b9d668b82cf015259ae8ecbf3001d6719048ffdce15b04f642b4f009480a0d",
                completed.stdout,
            )


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
        self.assertEqual(
            report["tensor_contract"]["descriptor_sha256"],
            "8bad04023c1aa2d2810646ea4558942e23e8aab1bd901c6cb8d292845db2c653",
        )
        self.assertEqual(
            report["quantized_lowering"]["plan_sha256"],
            "a949c04bfce049d0c26be6b8ad322d3b1a108b714c479ffe41fe1c094cdefd13",
        )
        self.assertEqual(report["phases"]["phase0_source_nodes"], [0, 1_747])
        self.assertEqual(report["phases"]["phase1_source_nodes"], [1_747, 3_615])
        lowering = report["cpu_attribute_lowering"]
        self.assertEqual(lowering["records"], 2_227)
        self.assertEqual(lowering["raw_admitted_records_before_fusion"], 2_696)
        self.assertEqual(lowering["raw_surviving_records"], 1_845)
        self.assertEqual(lowering["direct_residual_records"], 146)
        self.assertEqual(lowering["native_quant_records"], 235)
        self.assertEqual(lowering["resolve_decoder_shape_records"], 1)
        self.assertEqual(lowering["f32_core_records"], 943)
        self.assertEqual(lowering["f32_unary_records"], 174)
        self.assertEqual(lowering["f32_total_records"], 1_117)
        self.assertEqual(lowering["layout_material_records"], 471)
        self.assertEqual(lowering["layout_view_records"], 258)
        self.assertEqual(lowering["layout_total_records"], 729)
        self.assertEqual(lowering["view_alias_records"], 258)
        self.assertEqual(lowering["view_static_controllers"], 207)
        self.assertEqual(lowering["view_dynamic_controllers"], 51)
        self.assertEqual(lowering["excluded_non_f32_add"], 81)
        self.assertEqual(lowering["operator_counts"]["DynamicQuantizedGemm"], 148)
        self.assertEqual(lowering["operator_counts"]["DynamicQuantizedConv1d"], 87)
        self.assertEqual(lowering["operator_counts"]["MatMul"], 27)
        self.assertEqual(lowering["operator_counts"]["Pow"], 50)
        self.assertEqual(lowering["operator_counts"]["BiLstm256"], 6)
        self.assertEqual(lowering["raw_plan_sha256"], TOOL.PINNED_LOWERING_SHA256)
        self.assertEqual(
            lowering["plan_sha256"],
            TOOL.PINNED_COMPLETE_LOWERING_SHA256,
        )
        self.assertEqual(
            lowering["source_ownership"],
            {
                "graph_source_nodes": 3_615,
                "owned_source_nodes": 3_615,
                "unowned_source_nodes": 0,
                "duplicate_source_nodes": 0,
                "quant_component_source_nodes": 1_615,
                "duration_component_source_nodes": 9,
                "sha256": TOOL.PINNED_SOURCE_OWNERSHIP_SHA256,
            },
        )
        self.assertEqual(report["phases"]["phase0_lowered_ops"], [0, 1_079])
        self.assertEqual(report["phases"]["phase1_lowered_ops"], [1_079, 2_227])
        self.assertEqual(
            report["phases"]["resolve_decoder_shape"]["output_bindings"],
            ["/encoder/CumSum_output_0", TOOL.FRAME_COUNT_TENSOR],
        )
        self.assertTrue(lowering["structural_program_emitted"])
        self.assertFalse(lowering["executable_graph_emitted"])
        structural = report["structural_program_plan"]
        self.assertIsNotNone(structural)
        assert structural is not None
        capacities = structural["tensor_capacities"]
        self.assertEqual(capacities["token_max"], 512)
        self.assertEqual(capacities["frame_max"], 2_560)
        self.assertEqual(capacities["descriptors"], 4_744)
        self.assertEqual(
            capacities["shape_sha256"],
            TOOL.PINNED_CAPACITY_SHAPES_SHA256[2_560],
        )
        self.assertEqual(
            capacities["runtime_shape_dependencies"],
            {"static": 2_922, "n_only": 656, "f_only": 1_162, "n_and_f": 4},
        )
        self.assertEqual(capacities["dynamic_affine_descriptors"], 815)
        self.assertEqual(
            structural["arenas"],
            {
                "phase0_bytes": 33_229_952,
                "phase1_min_bytes": 12_475_392,
                "phase1_f2560_bytes": 1_572_883_968,
                "phase1_max_bytes": 1_572_883_968,
                "phase1_live_byte_lower_bound": 1_572_878_336,
                "sizing_comparison": {
                    "f2560": {
                        "frame_max": 2_560,
                        "operational": True,
                        "live_byte_lower_bound": 1_572_878_336,
                        "packed_bytes": 1_572_883_968,
                        "packing_overhead_bytes": 5_632,
                        "packing_overhead_ppb": 3_581,
                        "packed_to_lower_bound_ratio": "1.000003580697",
                    },
                    "f3072": {
                        "frame_max": 3_072,
                        "operational": False,
                        "live_byte_lower_bound": 1_887_451_136,
                        "packed_bytes": 1_887_456_768,
                        "packing_overhead_bytes": 5_632,
                        "packing_overhead_ppb": 2_984,
                        "packed_to_lower_bound_ratio": "1.000002983918",
                    },
                },
                "alignment": 64,
            },
        )
        self.assertEqual(structural["program"]["ops"], 2_227)
        self.assertEqual(structural["program"]["bindings"], 7_314)
        self.assertEqual(structural["program"]["slots"], 2_055)
        self.assertEqual(
            structural["program"]["slot_phase_counts"],
            {"phase0": 788, "phase1": 1_123, "shared": 144},
        )
        self.assertEqual(structural["program"]["constant_tensors"], 762)
        self.assertEqual(structural["program"]["constant_payload_bytes"], 123_168_704)
        self.assertEqual(
            structural["program"]["work_unit_contract"],
            {
                "record_counts": {
                    "atomic_whole_op": 2_214,
                    "float_conv_channel_time_tiles": 7,
                    "resize_output_elements": 6,
                },
                "emitted_units": {
                    "atomic_whole_op": 2_214,
                    "float_conv_channel_time_tiles": 71_546_922,
                    "resize_output_elements": 26_229_760,
                },
                "partial_slice_families": [
                    "float_conv_channel_time_tiles",
                    "resize_output_elements",
                ],
                "atomic_records_have_one_unit": True,
            },
        )
        truth = structural["truth"]
        self.assertTrue(truth["structural_program_emitted"])
        self.assertTrue(truth["rust_program_parse_verified"])
        self.assertFalse(truth["executable_graph_emitted"])
        self.assertEqual(truth["artifact_bytes"], TOOL.PINNED_ARTIFACT_BYTES)
        self.assertEqual(
            truth["runtime_blockers"],
            {
                "dynamic_no_copy_view_records": 14,
                "memory_alias_identity_view_records": 258,
                "quant_adapter_records": 235,
                "atomic_bilstm_records": 6,
                "atomic_stft_records": 1,
            },
        )
        self.assertEqual(
            self.analysis.ranks[
                "/decoder/decoder/generator/istft/stft/Reshape_1_output_0"
            ],
            2,
        )

    def test_real_structural_artifact_is_canonical(self) -> None:
        plan = self.analysis.executable_plan
        self.assertIsNotNone(plan)
        assert plan is not None
        artifact = TOOL.emit_pinned_program(self.analysis)
        inspected = TOOL.inspect_aot(artifact)
        self.assertEqual(len(artifact), TOOL.PINNED_ARTIFACT_BYTES)
        self.assertEqual(
            inspected["artifact_sha256"],
            TOOL.PINNED_ARTIFACT_SEAL_SHA256,
        )
        self.assertEqual(
            inspected["file_sha256"],
            TOOL.PINNED_ARTIFACT_FILE_SHA256,
        )
        self.assertEqual(
            inspected["sections"],
            {
                "tensors": 4_744,
                "slots": 2_055,
                "ops": 2_227,
                "bindings": 7_314,
                "phases": 2,
                "data": 123_223_824,
            },
        )

    @unittest.skipUnless(
        shutil.which("cargo") and RUST_MANIFEST.is_file() and RUST_INSPECTOR.is_file(),
        "Rust cross-language inspector is not available",
    )
    def test_rust_parser_accepts_real_structural_program(self) -> None:
        plan = self.analysis.executable_plan
        self.assertIsNotNone(plan)
        assert plan is not None
        artifact = TOOL.emit_pinned_program(self.analysis)
        with test_scratch_directory() as directory:
            path = Path(directory) / "kokoro.kkaot"
            path.write_bytes(artifact)
            completed = subprocess.run(
                [
                    "cargo",
                    "run",
                    "--quiet",
                    "--manifest-path",
                    str(RUST_MANIFEST),
                    "--target-dir",
                    str(RUST_TARGET_DIR),
                    "--example",
                    "inspect",
                    "--",
                    str(path),
                    "2560",
                ],
                cwd="/tmp",
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
        self.assertEqual(completed.returncode, 0, completed.stdout)
        self.assertIn("section[0]=Tensors offset=352 count=4744", completed.stdout)
        self.assertIn("phase=Phase0 ops=0..1079 arena=33229952..33229952", completed.stdout)
        self.assertIn(
            "phase=Phase1 ops=1079..2227 arena=12475392..1572883968",
            completed.stdout,
        )
        self.assertIn("resolved_arena_bytes=1572883968", completed.stdout)

    def test_complete_attributes_and_source_ownership(self) -> None:
        owners: list[int] = []
        native_quant = 0
        resolver = None
        for record in self.analysis.lowerings:
            TOOL.inspect_attribute_record(record.attributes, record.opcode)
            owners.extend(record.owned_sources or (record.source_index,))
            native_quant += record.op_type in {
                "DynamicQuantizedGemm",
                "DynamicQuantizedConv1d",
            }
            if record.op_type == "ResolveDecoderShape":
                resolver = record
        self.assertEqual(native_quant, 235)
        self.assertEqual(sorted(owners), list(range(3_615)))
        self.assertEqual(len(owners), len(set(owners)))
        self.assertIsNotNone(resolver)
        assert resolver is not None
        self.assertEqual(resolver.owned_sources, tuple(range(1_738, 1_747)))
        self.assertEqual(len(resolver.inputs), 2)
        self.assertEqual(len(resolver.outputs), 2)

    def test_structural_program_is_byte_exact_and_rust_parseable(self) -> None:
        artifact = TOOL.emit_pinned_program(self.analysis)
        self.assertEqual(len(artifact), TOOL.PINNED_ARTIFACT_BYTES)
        self.assertEqual(
            TOOL.hashlib.sha256(artifact).hexdigest(),
            TOOL.PINNED_ARTIFACT_FILE_SHA256,
        )
        inspected = TOOL.inspect_aot(artifact)
        self.assertEqual(
            inspected["artifact_sha256"], TOOL.PINNED_ARTIFACT_SEAL_SHA256
        )
        self.assertEqual(inspected["sections"]["tensors"], 4_744)
        self.assertEqual(inspected["sections"]["slots"], 2_055)
        self.assertEqual(inspected["sections"]["ops"], 2_227)

        if shutil.which("cargo") and RUST_MANIFEST.is_file() and RUST_INSPECTOR.is_file():
            with test_scratch_directory() as directory:
                path = Path(directory) / "kokoro.kkaot"
                path.write_bytes(artifact)
                completed = subprocess.run(
                    [
                        "cargo",
                        "run",
                        "--quiet",
                        "--manifest-path",
                        str(RUST_MANIFEST),
                        "--target-dir",
                        str(RUST_TARGET_DIR),
                        "--example",
                        "inspect",
                        "--",
                        str(path),
                    ],
                    cwd="/tmp",
                    check=False,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    text=True,
                )
                self.assertEqual(completed.returncode, 0, completed.stdout)
                self.assertIn(
                    f"artifact_sha256={TOOL.PINNED_ARTIFACT_SEAL_SHA256}",
                    completed.stdout,
                )

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

    def test_cpu_attribute_change_is_rejected(self) -> None:
        node = next(
            node
            for node in self.analysis.model.graph.node
            if node.op_type == "ReduceMean"
        )
        keepdims = next(attribute for attribute in node.attribute if attribute.name == "keepdims")
        original = keepdims.i
        keepdims.i = 0
        try:
            with self.assertRaisesRegex(TOOL.CompileError, "ReduceMean attributes"):
                TOOL.build_supported_lowerings(self.analysis)
        finally:
            keepdims.i = original

    def test_view_attribute_change_is_rejected(self) -> None:
        node = next(
            node
            for node in self.analysis.model.graph.node
            if node.op_type == "Reshape" and node.attribute
        )
        allowzero = next(attribute for attribute in node.attribute if attribute.name == "allowzero")
        original = allowzero.i
        allowzero.i = 1
        try:
            with self.assertRaisesRegex(TOOL.CompileError, "Reshape attributes"):
                TOOL.build_supported_lowerings(self.analysis)
        finally:
            allowzero.i = original

    def test_layout_permutation_change_is_rejected(self) -> None:
        node = next(
            node for node in self.analysis.model.graph.node if node.op_type == "Transpose"
        )
        permutation = next(
            attribute for attribute in node.attribute if attribute.name == "perm"
        )
        original = tuple(permutation.ints)
        del permutation.ints[:]
        permutation.ints.extend([0] * len(original))
        try:
            with self.assertRaisesRegex(TOOL.CompileError, "Transpose.*rejected"):
                TOOL.build_supported_lowerings(self.analysis)
        finally:
            del permutation.ints[:]
            permutation.ints.extend(original)

    def test_negative_slice_step_is_rejected(self) -> None:
        node = next(
            node
            for node in self.analysis.model.graph.node
            if node.op_type == "Slice" and len(node.input) == 5
        )
        negative_one = next(
            name
            for name, tensor in self.analysis.initializers.items()
            if int(tensor.data_type) == 7
            and tuple(int(dim) for dim in tensor.dims) == (1,)
            and TOOL.initializer_values(self.analysis.onnx, tensor) == (-1,)
        )
        original = node.input[4]
        node.input[4] = negative_one
        try:
            with self.assertRaisesRegex(TOOL.CompileError, "negative/non-unit Slice step"):
                TOOL.build_supported_lowerings(self.analysis)
        finally:
            node.input[4] = original

    def test_non_reflect_pad_is_rejected(self) -> None:
        node = next(node for node in self.analysis.model.graph.node if node.op_type == "Pad")
        mode = next(attribute for attribute in node.attribute if attribute.name == "mode")
        original = mode.s
        mode.s = b"constant"
        try:
            with self.assertRaisesRegex(TOOL.CompileError, "non-reflect Pad"):
                TOOL.build_supported_lowerings(self.analysis)
        finally:
            mode.s = original

    def test_scatter_reduction_change_is_rejected(self) -> None:
        node = next(
            node for node in self.analysis.model.graph.node if node.op_type == "ScatterND"
        )
        reduction = next(
            attribute for attribute in node.attribute if attribute.name == "reduction"
        )
        original = reduction.s
        reduction.s = b"add"
        try:
            with self.assertRaisesRegex(TOOL.CompileError, "reduction contract"):
                TOOL.build_supported_lowerings(self.analysis)
        finally:
            reduction.s = original


if __name__ == "__main__":
    unittest.main()
