#!/usr/bin/env python3
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("check_rebuilt_artifacts.py").resolve()


class RebuiltArtifactTests(unittest.TestCase):
    def test_comparison_preserves_artifact_checks(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            def git(*args):
                subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)

            def check(expected):
                result = subprocess.run(["python3", str(SCRIPT), "artifacts"], cwd=root, capture_output=True)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)

            git("init", "-q")
            directory = root / "artifacts"
            directory.mkdir()
            contract = directory / "kernel.contract.rs"
            contract.write_text('// source: /old/kernel.clcpp\n// normalized commands: old\nconst ABI: u32 = 1;\n')
            binary = directory / "kernel.bin"
            binary.write_bytes(b"original")
            git("add", ".")
            git("-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-qm", "fixture")
            check(0)
            contract.write_text('// source: /new/kernel.clcpp\n// normalized commands: new\nconst ABI: u32 = 1;\n')
            check(0)
            binary.write_bytes(b"changed")
            check(1)
            binary.write_bytes(b"original")
            contract.write_text('const ABI: u32 = 2;\n')
            check(1)
            git("add", ".")  # Staging must not hide an ABI change.
            check(1)
            contract.unlink()
            check(1)


if __name__ == "__main__":
    unittest.main()
