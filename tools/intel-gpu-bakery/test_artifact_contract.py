#!/usr/bin/env python3
"""Host-only regression tests for the Intel GPU bakery contract parser."""

from __future__ import annotations

import json
import unittest
from pathlib import Path

from artifact_contract import (
    abi_projection,
    analyze_zebin,
    parse_ze_info_yaml,
    verify_manifest,
)


TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent
ARTIFACT_ROOT = (
    REPO_ROOT
    / "crates"
    / "trueos-shader"
    / "gpgpu"
    / "kernels"
    / "artifacts"
    / "adls"
)


class ArtifactContractTests(unittest.TestCase):
    def test_every_legacy_zebin_is_unambiguously_parseable(self) -> None:
        binaries = sorted(ARTIFACT_ROOT.glob("*.bin"))
        self.assertGreater(len(binaries), 10)
        for binary in binaries:
            with self.subTest(binary=binary.name):
                analysis = analyze_zebin(binary)
                self.assertGreaterEqual(len(analysis["kernels"]), 1)

    def test_copy_contract_distinguishes_section_and_entry_ranges(self) -> None:
        analysis = analyze_zebin(ARTIFACT_ROOT / "copy_rect_rgba8.bin")
        kernel = analysis["kernels"][0]
        self.assertEqual(kernel["kernel_name"], "copy_rect_rgba8")
        self.assertEqual(kernel["text"]["section_offset"], 64)
        self.assertEqual(kernel["text"]["section_size"], 896)
        self.assertEqual(kernel["text"]["entry_offset"], 64)
        self.assertEqual(kernel["text"]["entry_size"], 712)
        self.assertEqual(kernel["simd_width"], 16)
        self.assertEqual(kernel["cross_thread_data_bytes"], 96)
        self.assertEqual(kernel["per_thread_data_bytes"], 96)

    def test_cpp_and_legacy_copy_abis_are_exactly_equal(self) -> None:
        legacy = analyze_zebin(ARTIFACT_ROOT / "copy_rect_rgba8.bin")
        cpp = analyze_zebin(
            ARTIFACT_ROOT / "cpp" / "copy_rect_rgba8.bin",
            ARTIFACT_ROOT / "cpp" / "copy_rect_rgba8.spv",
        )
        self.assertEqual(abi_projection(cpp), abi_projection(legacy))

    def test_pointer_qualifiers_survive_cpp_translation(self) -> None:
        cpp = analyze_zebin(ARTIFACT_ROOT / "cpp" / "copy_rect_rgba8.bin")
        pointers = [
            arg
            for arg in cpp["kernels"][0]["payload_args"]
            if arg["kind"] == "by_pointer"
        ]
        self.assertEqual(
            [
                (arg["arg_index"], arg["access"], arg["address_mode"])
                for arg in pointers
            ],
            [(0, "readonly", "stateful"), (1, "readwrite", "stateful")],
        )

    def test_hybrid_pointer_keeps_stateful_and_stateless_facts(self) -> None:
        mesh = analyze_zebin(ARTIFACT_ROOT / "font_outline_mesh.bin")
        arg = next(
            item
            for item in mesh["kernels"][0]["payload_args"]
            if item["arg_index"] == 1
        )
        self.assertEqual(arg["address_mode"], "stateless")
        self.assertEqual(
            [item["address_mode"] for item in arg["representations"]],
            ["stateful", "stateless"],
        )

    def test_minor_version_is_data_not_a_parser_gate(self) -> None:
        parsed = parse_ze_info_yaml(
            "---\nversion: '1.999'\nkernels:\n  - name: probe\n...\n"
        )
        self.assertEqual(parsed["version"], "1.999")

    def test_published_manifest_and_generated_rust_are_current(self) -> None:
        root = ARTIFACT_ROOT / "cpp"
        verify_manifest(
            root / "copy_rect_rgba8.manifest.json",
            root / "copy_rect_rgba8.bin",
            root / "copy_rect_rgba8.spv",
            root / "copy_rect_rgba8.contract.rs",
            REPO_ROOT,
        )
        manifest = json.loads(
            (root / "copy_rect_rgba8.manifest.json").read_text(encoding="utf-8")
        )
        input_paths = {record["path"] for record in manifest["source"]["inputs"]}
        self.assertIn(
            "crates/trueos-shader/gpgpu/kernels/include/trueos_clcpp.hpp",
            input_paths,
        )


if __name__ == "__main__":
    unittest.main()
