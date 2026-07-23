#!/usr/bin/env python3
"""Regression tests for final-image Intel artifact presence proofs."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from verify_linked import LinkedArtifactError, verify_required_artifacts


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


if __name__ == "__main__":
    unittest.main()
