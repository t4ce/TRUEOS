#!/usr/bin/env python3
"""Test native font metrics against the embedded TTF without booting TRUEOS."""
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    source = Path(__file__).resolve().parents[2] / "crates/trueos-graphics/font_metrics.rs"
    with tempfile.TemporaryDirectory(prefix="trueos-font-metrics-") as directory:
        root = Path(directory)
        (root / "Cargo.toml").write_text(
            '[package]\nname = "trueos-font-metrics-tests"\nversion = "0.1.0"\nedition = "2024"\n'
            '[dependencies]\nskrifa = { version = "=0.44.0", default-features = false, features = ["libm"] }\n'
            '[lib]\npath = ' + json.dumps(str(source)) + '\n'
        )
        # Run outside the kernel's build-std/target configuration.
        subprocess.run([os.environ.get("CARGO", "cargo"), "test"], cwd=root, check=True)


if __name__ == "__main__":
    main()
