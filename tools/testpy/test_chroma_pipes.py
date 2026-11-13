#!/usr/bin/env python3
"""Run production CHROMATIC per-pipe selection and grading tests on the host."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = re.search(r'channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-chroma-pipes-") as temporary:
        directory = Path(temporary)
        harness = directory / "tests.rs"
        harness.write_text(
            '#![allow(dead_code)]\n'
            'extern crate self as libm;\n'
            'pub fn expf(x: f32) -> f32 { x.exp() }\n'
            'pub fn sqrtf(x: f32) -> f32 { x.sqrt() }\n'
            + '#[path="' + str(ROOT / "src/ui4/chroma_grade.rs") + '"] mod grade;\n'
            + 'mod chroma_color { pub(crate) use crate::grade::Settings; }\n'
            + '#[path="' + str(ROOT / "src/ui4/chroma_pipes.rs") + '"] mod pipes;\n'
        )
        for mode, flags in [("debug", []), ("optimized", ["-O"])]:
            binary = directory / mode
            subprocess.run(["rustup", "run", channel, "rustc", "--edition=2024", "--test",
                            *flags, str(harness), "-o", str(binary)], check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
