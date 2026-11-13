#!/usr/bin/env python3
"""Run the real CHROMATIC grading/transport code against independent modeled A/B MMIO ports."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = re.search(r'channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text())[1]
    fixture = (ROOT / "tools/testpy/fixtures/chroma_color.rs").read_text()
    fixture = fixture.replace("@CHROMA_COLOR_PATH@", str(ROOT / "src/ui4/chroma_color.rs"))
    with tempfile.TemporaryDirectory(prefix="trueos-chroma-color-") as temporary:
        directory = Path(temporary)
        harness = directory / "tests.rs"
        harness.write_text(fixture)
        for name, flags in [("debug", []), ("optimized", ["-O"])]:
            binary = directory / name
            subprocess.run(["rustup", "run", channel, "rustc", "--edition=2024",
                            "--test", *flags, str(harness), "-o", str(binary)],
                           check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
