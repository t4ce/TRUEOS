#!/usr/bin/env python3
"""Compare vendored PNG scalar/SIMD decoding, validating every decoded byte.

Generate assets with png_path_fixtures.py first. Runs the real vendored codec
in an isolated host crate, with the kernel's opt-level=2 and pinned toolchain.
Host timings are comparative evidence, not TRUEOS end-to-end timings.
"""
from pathlib import Path
import json
import os
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def main():
    assets = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / 'bld/png-path/assets').resolve()
    records = json.loads((assets / 'manifest.json').read_text())
    with tempfile.TemporaryDirectory(prefix='trueos-png-bench-') as temporary:
        directory = Path(temporary)
        (directory / 'src').mkdir()
        (directory / 'Cargo.toml').write_text(f'''[package]
name = "trueos-png-bench"
version = "0.0.0"
edition = "2024"
[workspace]
[features]
simd = ["png/unstable"]
[dependencies]
png = {{ path = "{ROOT / 'vendor/png-0.18.1'}", default-features = false }}
core3 = {{ version = "0.1.2", default-features = false, features = ["alloc"] }}
sha2 = "0.10"
[patch.crates-io]
fdeflate = {{ path = "{ROOT / 'vendor/fdeflate-0.3.7'}" }}
simd-adler32 = {{ path = "{ROOT / 'vendor/simd-adler32-0.3.8'}" }}
crc32fast = {{ path = "{ROOT / 'vendor/crc32fast-1.5.0'}" }}
[profile.release]
opt-level = 2
lto = "fat"
codegen-units = 1
''')
        cases = ',\n'.join(f'({json.dumps(str(assets / r["file"]))}, {json.dumps(r["raw_sha256"])})' for r in records)
        (directory / 'src/main.rs').write_text('''use sha2::{Digest, Sha256};
fn main() {
    for (path, expected) in [''' + cases + '''] {
        let encoded = std::fs::read(path).unwrap();
        let mut samples = Vec::new();
        for iteration in 0..8 {
            let start = std::time::Instant::now();
            let decoder = png::Decoder::new(core3::io::Cursor::new(encoded.as_slice()));
            let mut reader = decoder.read_info().unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let frame = reader.next_frame(&mut pixels).unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            pixels.truncate(frame.buffer_size());
            assert_eq!(format!("{:x}", Sha256::digest(&pixels)), expected, "{path}");
            if iteration > 0 { samples.push(elapsed); }
            std::hint::black_box(pixels);
        }
        samples.sort_by(f64::total_cmp);
        println!("{} median_ms={:.3} simd={}", std::path::Path::new(path).file_name().unwrap().to_str().unwrap(), samples[samples.len()/2], cfg!(feature="simd"));
    }
}
''')
        env = os.environ.copy()
        env['RUSTUP_TOOLCHAIN'] = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']
        env['CARGO_TARGET_DIR'] = str(ROOT / 'bld/png-path/host-target')
        for features in [[], ['--features', 'simd']]:
            subprocess.run(['cargo', 'run', '--offline', '--quiet', '--release', '--target', 'x86_64-unknown-linux-gnu', *features], cwd=directory, env=env, check=True)


if __name__ == '__main__':
    main()
