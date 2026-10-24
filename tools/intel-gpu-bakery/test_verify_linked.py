#!/usr/bin/env python3
"""Regression tests for final-image Intel artifact presence proofs."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from verify_linked import LinkedArtifactError, verify_required_artifacts, verify_single_copy_artifacts


class RequiredLinkedArtifactTests(unittest.TestCase):
    def test_required_artifact_is_counted_in_linked_image(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifact = root / "cpp_demo.bin"
            image = root / "TRUEOS.elf"
            artifact.write_bytes(b"cpp-demo-zebin")
            image.write_bytes(b"prefix-cpp-demo-zebin-middle-cpp-demo-zebin-suffix")

            records = verify_required_artifacts(image, [artifact])

            self.assertEqual(len(records), 1)
            self.assertEqual(records[0][0], artifact)
            self.assertEqual(records[0][2], 2)

    def test_missing_required_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifact = root / "cpp_demo.bin"
            image = root / "TRUEOS.elf"
            artifact.write_bytes(b"cpp-demo-zebin")
            image.write_bytes(b"unrelated-linked-image")

            with self.assertRaisesRegex(LinkedArtifactError, "required artifact is absent"):
                verify_required_artifacts(image, [artifact])


class SingleCopyArtifactTests(unittest.TestCase):
    def check_image(self, image_bytes: bytes) -> int:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "kernel.bin").write_bytes(b"ZEBIN<SPIRV>END")
            (root / "kernel.spv").write_bytes(b"SPIRV")
            image = root / "TRUEOS.elf"
            image.write_bytes(image_bytes)
            return verify_single_copy_artifacts(image, root)

    def test_spirv_borrowed_from_single_binary(self) -> None:
        self.assertEqual(self.check_image(b"prefixZEBIN<SPIRV>ENDsuffix"), 1)

    def test_duplicate_binary_is_rejected(self) -> None:
        with self.assertRaisesRegex(LinkedArtifactError, "duplicate shader binary"):
            self.check_image(b"ZEBIN<SPIRV>ENDZEBIN<SPIRV>END")

    def test_separate_spirv_is_rejected(self) -> None:
        with self.assertRaisesRegex(LinkedArtifactError, "SPIR-V must occur only inside"):
            self.check_image(b"ZEBIN<SPIRV>ENDSPIRV")

    def test_duplicate_shared_graphics_stage_is_rejected(self) -> None:
        root = Path(__file__).resolve().parents[2]
        stage = (root / "crates/trueos-shader/host_shader_validation/cache_extract/triangle_vs.bin").read_bytes()
        with self.assertRaisesRegex(LinkedArtifactError, "duplicate shared graphics stage"):
            self.check_image(stage + b"padding" + stage)

    def test_optional_unlinked_shader_is_allowed(self) -> None:
        self.assertEqual(self.check_image(b"unrelated image"), 0)


if __name__ == "__main__":
    unittest.main()
