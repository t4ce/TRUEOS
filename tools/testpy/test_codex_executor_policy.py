#!/usr/bin/env python3
"""Run the production topology/archive policy's Rust tests on the host."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    policy = ROOT / "src/workers/application_policy.rs"
    with tempfile.TemporaryDirectory(prefix="codex-executor-policy-") as work:
        source = Path(work) / "test.rs"
        source.write_text(f'#[path="{policy}"] mod application_policy;\n')
        binary = Path(work) / "test"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
