#!/usr/bin/env python3
"""Run production collapse capture, ownership and raster code on the host."""
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-collapse-animation-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-collapse-animation-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
spin = "0.10.0"
''')
        template = (ROOT / "tools/testpy/ui4_collapse_animation/harness.rs").read_text()
        (work / "src/lib.rs").write_text(template.replace("@TRUEOS_ROOT@", json.dumps(str(ROOT))[1:-1]))
        subprocess.run(["cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu", "--", "--test-threads=1"],
                       cwd=work, check=True, timeout=120)


if __name__ == "__main__":
    main()
