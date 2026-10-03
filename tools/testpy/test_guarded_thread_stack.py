#!/usr/bin/env python3
"""Real Linux guard faults and kernel PMM/alias ownership regression tests."""
from pathlib import Path
import resource
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class GuardedStackTests(unittest.TestCase):
    def run_rust(self, source: str, extra_files: dict[str, str] | None = None):
        with tempfile.TemporaryDirectory(prefix="trueos-stack-") as directory:
            root = Path(directory)
            for name, contents in (extra_files or {}).items():
                (root / name).write_text(contents)
            path = root / "test.rs"
            path.write_text(source)
            binary = root / "tests"
            subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True,
                           preexec_fn=lambda: resource.setrlimit(resource.RLIMIT_CORE, (0, 0)))

    def test_real_zeroed_pages_and_both_guard_faults(self):
        source = '#![allow(dead_code)]\n#[path="' + str(ROOT / "src/r/threads/stack.rs") + '"] mod stack;\n'
        self.run_rust(source)

    def test_kernel_backing_and_carrier_ownership(self):
        stack = (ROOT / "src/r/threads/stack.rs").read_text()
        # Select the actual kernel backend on the Linux host; only cfg gates
        # change. PMM and page mapping are mocked by the fixture below.
        stack = stack.replace('#[cfg(not(target_os = "linux"))]', '#[cfg(test)]')
        stack = stack.replace('#[cfg(target_os = "linux")]', '#[cfg(not(test))]')
        stack = stack.replace('#[cfg(all(test, target_os = "linux"))]', '#[cfg(not(test))]')
        fixture = (ROOT / "tools/rust-std/tests/guarded_stack_kernel.rs").read_text()
        self.run_rust(fixture + '\n#[path="kernel_stack.rs"] mod stack;\n', {"kernel_stack.rs": stack})


if __name__ == "__main__":
    unittest.main()
