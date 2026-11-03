#!/usr/bin/env python3
"""Run the production installer/formatter scratch-buffer tests on the host."""

import json
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    source = ROOT / "src/disc/aligned_buf.rs"
    with tempfile.TemporaryDirectory(prefix="trueos-aligned-disk-buffer-") as directory:
        work = Path(directory)
        harness = work / "harness.rs"
        harness.write_text(
            "extern crate alloc;\n"
            f"#[path = {json.dumps(str(source))}] mod aligned_buf;\n"
        )
        binary = work / "tests"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(harness), "-o", str(binary)],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
