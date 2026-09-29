#!/usr/bin/env python3
"""Run the UI4 connection lifetime tests without booting the kernel."""

import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    source = (
        Path(__file__).resolve().parents[1]
        / "src/ui4/blueprint_text/display_registry.rs"
    )
    with tempfile.TemporaryDirectory(prefix="trueos-display-tests-") as directory:
        root = Path(directory)
        harness = root / "tests.rs"
        binary = root / "tests"
        harness.write_text(
            f'#[path = {json.dumps(str(source))}]\nmod display_registry;\n'
        )
        subprocess.run(
            [
                os.environ.get("RUSTC", "rustc"),
                "--edition=2024",
                "--test",
                str(harness),
                "-o",
                str(binary),
            ],
            check=True,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
