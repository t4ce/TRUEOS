#!/usr/bin/env python3
"""Run production CHROMATIC layout, cube geometry and output admission tests."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = re.search(r'channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-chroma-geometry-") as temporary:
        directory = Path(temporary)
        harness = directory / "tests.rs"
        harness.write_text(
            '#![allow(dead_code)]\n'
            + '#[path="' + str(ROOT / "src/ui4/chroma_layout.rs") + '"] mod layout;\n'
            + '#[path="' + str(ROOT / "src/ui4/chroma_geometry.rs") + '"] mod geometry;\n'
            + '#[path="' + str(ROOT / "src/intel/render/color_output_admission.rs")
            + '"] mod color_output_admission;\n'
        )
        for mode, flags in [("debug", []), ("optimized", ["-O"])]:
            binary = directory / mode
            subprocess.run(["rustup", "run", channel, "rustc", "--edition=2024", "--test",
                            *flags, str(harness), "-o", str(binary)], check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
