#!/usr/bin/env python3
"""Host-check the actual capture lead-time policy, including release arithmetic."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-capture-timing-") as directory:
        directory = Path(directory)
        harness = directory / "timing.rs"
        harness.write_text(
            '#![allow(dead_code)]\n#[path="'
            + str(ROOT / "src/ui4/h264_capture_timing.rs")
            + '"] mod timing;\n'
        )
        for mode, flags in [("debug", []), ("optimized", ["-O"])]:
            binary = directory / mode
            subprocess.run(["rustup", "run", channel, "rustc", "--edition=2024", "--test",
                            *flags, str(harness), "-o", str(binary)], check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
