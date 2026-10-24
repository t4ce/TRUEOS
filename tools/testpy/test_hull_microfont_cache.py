#!/usr/bin/env python3
"""Run the production Hull font cache tests with real vendored microfont geometry.

Only display metrics and the monotonic clock are supplied by this host harness.
The Rust module owns the raster, concurrency, budget, fallback and paint tests.
Run: python3 tools/testpy/test_hull_microfont_cache.py
"""
from pathlib import Path
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

SOURCE = r'''
#![allow(dead_code)]
extern crate alloc;
#[macro_export]
macro_rules! log_important {
    (target: $target:literal; $($args:tt)*) => { let _ = format_args!($($args)*); };
}
mod intel {
    pub fn physical_extent_pixels_fraction(_: u32, height: u32, denominator: u32)
        -> Option<(u32, u32)>
    {
        Some((40, height * 4 / denominator))
    }
}
mod ui4 {
    pub fn output_dimensions() -> Option<(u32, u32)> { Some((1920, 1080)) }
    pub fn physical_extent_pixels(_:u32,h:u32)->Option<(u32,u32)> { Some((40,h*4)) }
    pub mod text_blit {
        pub fn stamp() -> u64 {
            static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64
        }
        pub fn nanos(ticks: u64) -> u64 { ticks }
    }
}
#[path = __MODULE__]
mod hull_font_service;
'''


def main():
    version = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(line.split(": ", 1)[1] for line in version.splitlines()
                if line.startswith("host: "))
    with tempfile.TemporaryDirectory(prefix="hull-microfont-tests-") as directory:
        path = Path(directory)
        (path / "src").mkdir()
        module = ROOT / "src/r/services/hull_font_service.rs"
        (path / "src/lib.rs").write_text(SOURCE.replace("__MODULE__", json.dumps(str(module))))
        manifest = '''[package]
name = "hull-microfont-host-tests"
version = "0.1.0"
edition = "2024"
[dependencies]
spin = "0.10"
libm = "0.2"
microfont = { path = __MICROFONT__, features = ["raster"] }
'''
        (path / "Cargo.toml").write_text(manifest.replace(
            "__MICROFONT__", json.dumps(str(ROOT / "vendor/microfont"))))
        # A temporary working directory avoids the kernel's custom-target and
        # build-std configuration. Offline mode never downloads dependencies.
        subprocess.run(["cargo", "test", "--offline", "--target", host,
                        "--manifest-path", str(path / "Cargo.toml")],
                       cwd=path, check=True)


if __name__ == "__main__":
    main()
