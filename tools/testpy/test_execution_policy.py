#!/usr/bin/env python3
"""Test the production VM execution policy without running privileged VMX code."""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
source = root / "src/hv/execution_policy.rs"
with tempfile.TemporaryDirectory(prefix="trueos-execution-policy-") as directory:
    work = Path(directory)
    harness = work / "test.rs"
    harness.write_text(f'#[path = "{source}"] mod execution_policy;\n')
    binary = work / "test"
    subprocess.run(["rustc", "--edition=2024", "--test", str(harness), "-o", str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
