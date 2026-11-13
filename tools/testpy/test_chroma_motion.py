#!/usr/bin/env python3
"""Run production color-lattice motion and per-pipe animation scheduling tests."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = re.search(r'channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-chroma-motion-") as temporary:
        directory = Path(temporary)
        harness = directory / "tests.rs"
        harness.write_text(
            '#![allow(dead_code)]\n'
            'extern crate alloc;\n'
            'extern crate self as libm;\n'
            'pub fn sinf(x: f32) -> f32 { x.sin() }\n'
            'pub fn cosf(x: f32) -> f32 { x.cos() }\n'
            'pub fn sqrtf(x: f32) -> f32 { x.sqrt() }\n'
            'pub fn expf(x: f32) -> f32 { x.exp() }\n'
            + '#[path="' + str(ROOT / "src/ui4/chroma_grade.rs") + '"] mod grade;\n'
            + 'mod chroma_color { pub(crate) use crate::grade::{Settings,LUT_EDGE,LUT_ENTRIES}; }\n'
            + '#[path="' + str(ROOT / "src/ui4/chroma_motion.rs") + '"] mod chroma_motion;\n'
            + '#[path="' + str(ROOT / "src/ui4/chroma_pipes.rs") + '"] mod chroma_pipes;\n'
            + '#[path="' + str(ROOT / "src/ui4/chroma_animation.rs") + '"] mod animation;\n'
        )
        for mode, flags in [("debug", []), ("optimized", ["-O"])]:
            binary = directory / mode
            subprocess.run(["rustup", "run", channel, "rustc", "--edition=2024", "--test",
                            *flags, str(harness), "-o", str(binary)], check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
