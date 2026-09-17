#!/usr/bin/env python3
"""Compile the private Font RCS tessellation preflight as a host fixture."""
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
MODULE = ROOT / "src/intel/gpgpu/rcs/font_tessellation.rs"


def main() -> None:
    source = f"""\
#![allow(dead_code)]
extern crate self as libm;
pub fn sqrtf(value: f32) -> f32 {{ value.sqrt() }}
pub fn ceilf(value: f32) -> f32 {{ value.ceil() }}
pub fn fmaxf(left: f32, right: f32) -> f32 {{ left.max(right) }}
mod intel {{
    pub mod shader {{
        pub mod font_patch {{
            pub const AVAILABLE: bool = false;
            pub const COMPILED_DEVICE_ID: u16 = 0;
        }}
    }}
}}
mod probe {{ include!(r#"{MODULE}"#); }}
"""
    with tempfile.TemporaryDirectory(prefix="trueos-font-tess-preflight-") as temporary:
        root = Path(temporary)
        path = root / "tests.rs"
        binary = root / "tests"
        path.write_text(source)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)],
            check=True,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
