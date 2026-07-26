#!/usr/bin/env python3
"""Host-only regression tests for the Intel GPU bakery contract parser."""

from __future__ import annotations

import copy
import hashlib
import json
import os
import re
import tempfile
import unittest
from unittest import mock
from pathlib import Path

from artifact_contract import (
    ContractError,
    abi_projection,
    analyze_zebin,
    parse_ze_info_yaml,
    validate_constraints,
    verify_manifest,
)
from bake import _environment


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
KERNEL_CATALOG = REPO_ROOT / "src" / "intel" / "gpgpu" / "kernel_catalog.rs"


class ArtifactContractTests(unittest.TestCase):
    def test_bake_environment_drops_unmodeled_compiler_inputs(self) -> None:
        hostile = {
            "CCC_OVERRIDE_OPTIONS": "+-DTRUEOS_ENV_OVERRIDE=1",
            "CPATH": "/unreviewed/include",
            "CPLUS_INCLUDE_PATH": "/unreviewed/cpp",
            "COMPILER_PATH": "/unreviewed/bin",
            "GCC_EXEC_PREFIX": "/unreviewed/gcc",
            "LD_AUDIT": "/unreviewed/audit.so",
            "LD_LIBRARY_PATH": "/unreviewed/lib",
            "LD_PRELOAD": "/unreviewed/preload.so",
            "LIBRARY_PATH": "/unreviewed/archive",
        }
        with mock.patch.dict(os.environ, hostile, clear=False):
            environment = _environment([])
        for name in hostile:
            self.assertNotIn(name, environment)
        self.assertEqual(environment["SOURCE_DATE_EPOCH"], "0")
        self.assertEqual(environment["LC_ALL"], "C")

    def test_every_legacy_zebin_is_unambiguously_parseable(self) -> None:
        binaries = sorted(ARTIFACT_ROOT.glob("*.bin"))
        self.assertGreater(len(binaries), 10)
        for binary in binaries:
            with self.subTest(binary=binary.name):
                analysis = analyze_zebin(binary)
                self.assertGreaterEqual(len(analysis["kernels"]), 1)

    def test_every_legacy_catalog_hash_matches_its_zebin(self) -> None:
        catalog = KERNEL_CATALOG.read_text(encoding="utf-8")
        declarations = re.findall(
            r"pub\(crate\) const ([A-Z0-9_]+)_ADLS_BIN_SHA256:"
            r"\s*\[u8;\s*32\]\s*=\s*\[(.*?)\];",
            catalog,
            flags=re.DOTALL,
        )
        # Spirit keeps its two legacy binaries solely as ABI references for
        # the unconditionally selected C++ repass. They are no longer embedded
        # through literal legacy catalog hash declarations.
        abi_reference_only = {
            "spirit_vfx_background_rgba8.bin",
            "spirit_vfx_sprite_rgba8.bin",
        }
        binaries = sorted(
            binary
            for binary in ARTIFACT_ROOT.glob("*.bin")
            if binary.name not in abi_reference_only
        )
        self.assertEqual(len(declarations), len(binaries))
        observed_names = set()
        for stem, byte_source in declarations:
            binary = ARTIFACT_ROOT / f"{stem.lower()}.bin"
            observed_names.add(binary.name)
            with self.subTest(binary=binary.name):
                self.assertTrue(binary.is_file())
                expected = bytes(
                    int(value, 16)
                    for value in re.findall(r"0x([0-9A-Fa-f]{2})", byte_source)
                )
                self.assertEqual(len(expected), 32)
                self.assertEqual(hashlib.sha256(binary.read_bytes()).digest(), expected)
        self.assertEqual(observed_names, {binary.name for binary in binaries})

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
        self.assertEqual(
            kernel["implicit_payload_args"],
            [
                {"arg_type": "global_id_offset", "offset": 0, "size": 12},
                {"arg_type": "local_size", "offset": 12, "size": 12},
                {"arg_type": "enqueued_local_size", "offset": 32, "size": 12},
            ],
        )
        self.assertEqual(
            kernel["per_thread_payload_args"],
            [{"arg_type": "local_id", "offset": 0, "size": 96}],
        )

    def test_cpp_and_legacy_copy_abis_are_exactly_equal(self) -> None:
        legacy = analyze_zebin(ARTIFACT_ROOT / "copy_rect_rgba8.bin")
        cpp = analyze_zebin(
            ARTIFACT_ROOT / "cpp" / "copy_rect_rgba8.bin",
            ARTIFACT_ROOT / "cpp" / "copy_rect_rgba8.spv",
        )
        self.assertEqual(abi_projection(cpp), abi_projection(legacy))

    def test_cpp_and_legacy_spirit_abis_are_exactly_equal(self) -> None:
        for stem in (
            "spirit_vfx_background_rgba8",
            "spirit_vfx_sprite_rgba8",
        ):
            with self.subTest(artifact=stem):
                legacy = analyze_zebin(ARTIFACT_ROOT / f"{stem}.bin")
                cpp = analyze_zebin(
                    ARTIFACT_ROOT / "cpp" / f"{stem}.bin",
                    ARTIFACT_ROOT / "cpp" / f"{stem}.spv",
                )
                self.assertEqual(abi_projection(cpp), abi_projection(legacy))

    def test_abi_projection_detects_implicit_payload_drift(self) -> None:
        cpp = analyze_zebin(ARTIFACT_ROOT / "cpp" / "copy_rect_rgba8.bin")
        changed = copy.deepcopy(cpp)
        changed["kernels"][0]["implicit_payload_args"][1]["offset"] += 4
        self.assertNotEqual(abi_projection(changed), abi_projection(cpp))

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

    def test_constraint_gate_rejects_unreviewed_ze_info_major(self) -> None:
        analysis = analyze_zebin(ARTIFACT_ROOT / "copy_rect_rgba8.bin")
        analysis["kernels"][0]["ze_info_major"] = 2
        with self.assertRaisesRegex(ContractError, r"unsupported \.ze_info major 2"):
            validate_constraints(analysis, 16, 0, 0)

    def test_sibling_spirv_must_match_embedded_zebin_section(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            mismatched = Path(directory) / "copy_rect_rgba8.spv"
            mismatched.write_bytes(b"not the embedded SPIR-V")
            with self.assertRaisesRegex(ContractError, "does not match"):
                analyze_zebin(
                    ARTIFACT_ROOT / "cpp" / "copy_rect_rgba8.bin",
                    mismatched,
                )

    def test_published_manifest_and_generated_rust_are_current(self) -> None:
        root = ARTIFACT_ROOT / "cpp"
        manifests = sorted(root.glob("*.manifest.json"))
        self.assertEqual(
            [path.name for path in manifests],
            [
                "copy_rect_rgba8.manifest.json",
                "cpp_audio_visualizer_rgba8.manifest.json",
                "cpp_demo_rgba8.manifest.json",
                "font_instance_rgba8.manifest.json",
                "particle_craft.manifest.json",
                "spirit_vfx_background_rgba8.manifest.json",
                "spirit_vfx_sprite_rgba8.manifest.json",
            ],
        )
        for manifest_path in manifests:
            stem = manifest_path.name.removesuffix(".manifest.json")
            with self.subTest(artifact=stem):
                verify_manifest(
                    manifest_path,
                    root / f"{stem}.bin",
                    root / f"{stem}.spv",
                    root / f"{stem}.contract.rs",
                    REPO_ROOT,
                )
        manifest = json.loads(
            (root / "copy_rect_rgba8.manifest.json").read_text(encoding="utf-8")
        )
        self.assertEqual(manifest["target"]["pci_device_ids"], [0x4680])
        self.assertEqual(manifest["target"]["revision_min"], 0x0C)
        self.assertEqual(manifest["target"]["revision_max"], 0x0C)
        self.assertEqual(
            manifest["provenance"]["profile"]["path"],
            "tools/intel-gpu-bakery/profiles/adls-4680-r0c-cpp.json",
        )
        input_paths = {record["path"] for record in manifest["source"]["inputs"]}
        self.assertIn(
            "crates/trueos-shader/gpgpu/kernels/include/trueos_clcpp.hpp",
            input_paths,
        )
        for tool in manifest["provenance"]["toolchain"]["tools"].values():
            self.assertNotIn("path", tool)
        clang_dependencies = {
            record["soname"]
            for record in manifest["provenance"]["toolchain"]["tools"]["clang"][
                "dynamic_compiler_libraries"
            ]
        }
        self.assertTrue(
            any(name.startswith("libclang-cpp") for name in clang_dependencies)
        )
        self.assertTrue(any(name.startswith("libLLVM") for name in clang_dependencies))
        resource_tree = manifest["provenance"]["toolchain"]["tools"]["clang"][
            "resource_tree"
        ]
        self.assertGreater(resource_tree["file_count"], 0)
        self.assertGreater(resource_tree["size_bytes"], 0)
        self.assertRegex(resource_tree["tree_sha256"], r"^[0-9a-f]{64}$")
        serialized = json.dumps(manifest, sort_keys=True)
        self.assertNotIn(str(REPO_ROOT.parent), serialized)

    def test_particle_craft_is_one_exact_two_entry_artifact(self) -> None:
        analysis = analyze_zebin(
            ARTIFACT_ROOT / "cpp" / "particle_craft.bin",
            ARTIFACT_ROOT / "cpp" / "particle_craft.spv",
        )
        kernels = {kernel["kernel_name"]: kernel for kernel in analysis["kernels"]}
        self.assertEqual(
            set(kernels),
            {"particle_craft_step", "particle_craft_render_rgba8"},
        )
        self.assertEqual(kernels["particle_craft_step"]["simd_width"], 16)
        self.assertEqual(kernels["particle_craft_step"]["cross_thread_data_bytes"], 64)
        self.assertEqual(
            kernels["particle_craft_render_rgba8"]["cross_thread_data_bytes"],
            96,
        )
        self.assertTrue(
            all(kernel["scratch_bytes"] == 0 for kernel in kernels.values())
        )
        self.assertTrue(all(kernel["slm_bytes"] == 0 for kernel in kernels.values()))

    def test_spirit_cpp_sources_are_self_contained_native_artifacts(self) -> None:
        root = ARTIFACT_ROOT / "cpp"
        expected = {
            "spirit_vfx_background_rgba8": (2, 64),
            "spirit_vfx_sprite_rgba8": (3, 96),
        }
        for stem, (binding_count, cross_thread_bytes) in expected.items():
            with self.subTest(artifact=stem):
                manifest = json.loads(
                    (root / f"{stem}.manifest.json").read_text(encoding="utf-8")
                )
                self.assertEqual(manifest["variant"], "cpp-native")
                self.assertIsNone(manifest["abi_reference"])
                self.assertEqual(
                    manifest["provenance"]["publication_policy"],
                    {
                        "name": "cpp-native-aot-v1",
                        "expected_kernels": [stem],
                    },
                )
                input_paths = {
                    record["path"] for record in manifest["source"]["inputs"]
                }
                self.assertIn(
                    f"crates/trueos-shader/gpgpu/kernels/{stem}.clcpp",
                    input_paths,
                )
                self.assertNotIn(
                    f"crates/trueos-shader/gpgpu/kernels/{stem}.cl",
                    input_paths,
                )
                self.assertIn(
                    "crates/trueos-shader/gpgpu/kernels/include/trueos_clcpp.hpp",
                    input_paths,
                )
                analysis = analyze_zebin(
                    root / f"{stem}.bin",
                    root / f"{stem}.spv",
                )
                kernel = analysis["kernels"][0]
                self.assertEqual(len(kernel["bindings"]), binding_count)
                self.assertEqual(
                    kernel["cross_thread_data_bytes"],
                    cross_thread_bytes,
                )
                self.assertEqual(kernel["per_thread_data_bytes"], 96)
                self.assertEqual(kernel["scratch_bytes"], 0)
                self.assertEqual(kernel["slm_bytes"], 0)

    def test_cpp_native_demo_has_reviewed_standalone_policy(self) -> None:
        root = ARTIFACT_ROOT / "cpp"
        manifest = json.loads(
            (root / "cpp_demo_rgba8.manifest.json").read_text(encoding="utf-8")
        )
        self.assertEqual(manifest["variant"], "cpp-native")
        self.assertIsNone(manifest["abi_reference"])
        self.assertEqual(
            manifest["provenance"]["publication_policy"],
            {
                "name": "cpp-native-aot-v1",
                "expected_kernels": ["cpp_demo_rgba8"],
            },
        )
        self.assertEqual(manifest["target"]["pci_device_ids"], [0x4680])
        self.assertEqual(manifest["target"]["revision_min"], 0x0C)
        self.assertEqual(manifest["target"]["revision_max"], 0x0C)
        analysis = analyze_zebin(
            root / "cpp_demo_rgba8.bin", root / "cpp_demo_rgba8.spv"
        )
        self.assertEqual(
            [kernel["kernel_name"] for kernel in analysis["kernels"]],
            ["cpp_demo_rgba8"],
        )
        kernel = analysis["kernels"][0]
        self.assertEqual(kernel["simd_width"], 16)
        self.assertEqual(kernel["cross_thread_data_bytes"], 128)
        self.assertEqual(kernel["per_thread_data_bytes"], 96)
        self.assertEqual(kernel["scratch_bytes"], 0)
        self.assertEqual(kernel["slm_bytes"], 0)

    def test_cpp_audio_visualizer_has_reviewed_single_kernel_policy(self) -> None:
        root = ARTIFACT_ROOT / "cpp"
        manifest = json.loads(
            (root / "cpp_audio_visualizer_rgba8.manifest.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(manifest["variant"], "cpp-native")
        self.assertIsNone(manifest["abi_reference"])
        self.assertEqual(
            manifest["provenance"]["publication_policy"],
            {
                "name": "cpp-native-aot-v1",
                "expected_kernels": ["cpp_audio_visualizer_rgba8"],
            },
        )
        self.assertEqual(manifest["target"]["pci_device_ids"], [0x4680])
        self.assertEqual(manifest["target"]["revision_min"], 0x0C)
        self.assertEqual(manifest["target"]["revision_max"], 0x0C)
        analysis = analyze_zebin(
            root / "cpp_audio_visualizer_rgba8.bin",
            root / "cpp_audio_visualizer_rgba8.spv",
        )
        self.assertEqual(
            [kernel["kernel_name"] for kernel in analysis["kernels"]],
            ["cpp_audio_visualizer_rgba8"],
        )
        kernel = analysis["kernels"][0]
        self.assertEqual(kernel["simd_width"], 16)
        self.assertEqual(kernel["cross_thread_data_bytes"], 96)
        self.assertEqual(kernel["per_thread_data_bytes"], 96)
        self.assertEqual(kernel["scratch_bytes"], 0)
        self.assertEqual(kernel["slm_bytes"], 0)
        self.assertEqual(
            kernel["bindings"],
            [{"arg_index": 0, "bti": 0}, {"arg_index": 1, "bti": 1}],
        )
        self.assertEqual(
            [
                (arg["arg_index"], arg["kind"], arg["offset_bytes"], arg["size_bytes"])
                for arg in kernel["payload_args"]
            ],
            [
                (0, "by_pointer", 48, 8),
                (1, "by_pointer", 56, 8),
                (2, "by_value", 64, 4),
                (3, "by_value", 68, 4),
                (4, "by_value", 72, 4),
                (5, "by_value", 76, 4),
                (6, "by_value", 80, 4),
                (7, "by_value", 84, 4),
            ],
        )

    def test_cpp_publication_gates_cannot_be_removed_from_manifest(self) -> None:
        root = ARTIFACT_ROOT / "cpp"
        manifest_path = root / "copy_rect_rgba8.manifest.json"
        original = json.loads(manifest_path.read_text(encoding="utf-8"))
        mutations = {
            "source inputs": lambda value: value["source"].update(inputs=[]),
            "ABI reference": lambda value: value.update(abi_reference=None),
            "reproducibility": lambda value: value["provenance"].update(
                reproducibility_check="not-requested"
            ),
            "toolchain": lambda value: value["provenance"].update(
                toolchain={"status": "unavailable"}, toolchain_lock=None
            ),
            "expected kernels": lambda value: value["provenance"][
                "publication_policy"
            ].update(expected_kernels=[]),
        }
        with tempfile.TemporaryDirectory() as directory:
            for label, mutate in mutations.items():
                with self.subTest(gate=label):
                    changed = copy.deepcopy(original)
                    mutate(changed)
                    candidate = Path(directory) / f"{label.replace(' ', '-')}.json"
                    candidate.write_text(
                        json.dumps(changed, indent=2, sort_keys=True) + "\n",
                        encoding="utf-8",
                    )
                    with self.assertRaises(ContractError):
                        verify_manifest(
                            candidate,
                            root / "copy_rect_rgba8.bin",
                            root / "copy_rect_rgba8.spv",
                            root / "copy_rect_rgba8.contract.rs",
                            REPO_ROOT,
                        )

    def test_cpp_native_publication_gates_cannot_be_removed(self) -> None:
        root = ARTIFACT_ROOT / "cpp"
        manifest_path = root / "cpp_demo_rgba8.manifest.json"
        original = json.loads(manifest_path.read_text(encoding="utf-8"))
        mutations = {
            "source inputs": lambda value: value["source"].update(inputs=[]),
            "unexpected ABI reference": lambda value: value.update(
                abi_reference={"result": "exact-match"}
            ),
            "reproducibility": lambda value: value["provenance"].update(
                reproducibility_check="not-requested"
            ),
            "toolchain": lambda value: value["provenance"].update(
                toolchain={"status": "unavailable"}, toolchain_lock=None
            ),
            "expected kernels": lambda value: value["provenance"][
                "publication_policy"
            ].update(expected_kernels=[]),
            "policy": lambda value: value["provenance"][
                "publication_policy"
            ].update(name="unreviewed"),
        }
        with tempfile.TemporaryDirectory() as directory:
            for label, mutate in mutations.items():
                with self.subTest(gate=label):
                    changed = copy.deepcopy(original)
                    mutate(changed)
                    candidate = Path(directory) / f"{label.replace(' ', '-')}.json"
                    candidate.write_text(
                        json.dumps(changed, indent=2, sort_keys=True) + "\n",
                        encoding="utf-8",
                    )
                    with self.assertRaises(ContractError):
                        verify_manifest(
                            candidate,
                            root / "cpp_demo_rgba8.bin",
                            root / "cpp_demo_rgba8.spv",
                            root / "cpp_demo_rgba8.contract.rs",
                            REPO_ROOT,
                        )


if __name__ == "__main__":
    unittest.main()
