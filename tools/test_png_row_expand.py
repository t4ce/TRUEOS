#!/usr/bin/env python3
"""Run the kernel's real PNG color expansion functions on host, with timings."""
from pathlib import Path
import os
import subprocess
import tempfile
import tomllib
from test_clip_position3_uv_texture import ROOT, item

SOURCE = 'crates/trueos-graphics/decoder/png_decode_pool.rs'


def main():
    declarations = '\n'.join(item(SOURCE, name) for name in ['expand_rgb_rows', 'expand_gray_rows', 'expand_gray_alpha_rows'])
    tests = r'''
use std::ops::Range;
#[derive(Debug)] enum PngDecodeError { DecodeFailed }
#[test]
fn expansion_matches_reference_for_full_and_partial_rows() {
    for width in [0, 1, 2, 3, 7, 16, 17, 31, 64, 257] {
        for channels in 1..=3 {
            let pixels: Vec<u8> = (0..width * 7 * channels).map(|i| ((i * 37 + i / 7) % 256) as u8).collect();
            for rows in [0..7, 2..5, 0..0, 7..7] {
                let source = &pixels[rows.start * width * channels..rows.end * width * channels];
                let expected: Vec<u8> = source.chunks_exact(channels).flat_map(|p| match channels {
                    1 => [p[0], p[0], p[0], 255],
                    2 => [p[0], p[0], p[0], p[1]],
                    _ => [p[0], p[1], p[2], 255],
                }).collect();
                let output = match channels {
                    1 => expand_gray_rows(width, rows, &pixels),
                    2 => expand_gray_alpha_rows(width, rows, &pixels),
                    _ => expand_rgb_rows(width, rows, &pixels),
                }.unwrap();
                assert_eq!(output, expected);
            }
        }
    }
    assert!(expand_rgb_rows(2, 0..1, &[0; 5]).is_err());
    assert!(expand_gray_rows(2, 0..1, &[0]).is_err());
    assert!(expand_gray_alpha_rows(2, 0..1, &[0; 3]).is_err());
}
#[test]
fn report_4k_expansion() {
    for channels in 1..=3 {
        let pixels: Vec<u8> = (0..3840 * 2160 * channels).map(|i| (i * 37) as u8).collect();
        let mut samples = Vec::new();
        for iteration in 0..8 {
            let start = std::time::Instant::now();
            let output = match channels {
                1 => expand_gray_rows(3840, 0..2160, &pixels),
                2 => expand_gray_alpha_rows(3840, 0..2160, &pixels),
                _ => expand_rgb_rows(3840, 0..2160, &pixels),
            }.unwrap();
            if iteration > 0 { samples.push(start.elapsed().as_secs_f64() * 1000.0); }
            std::hint::black_box(output);
        }
        samples.sort_by(f64::total_cmp);
        println!("channels={channels} 4k_expand_median_ms={:.3}", samples[samples.len()/2]);
    }
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-png-expand-') as temporary:
        path = Path(temporary)
        (path / 'test.rs').write_text(tests + declarations)
        env = os.environ.copy()
        env['RUSTUP_TOOLCHAIN'] = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']
        subprocess.run(['rustc', '--edition=2024', '--test', '-C', 'opt-level=1', str(path / 'test.rs'), '-o', str(path / 'test')], cwd=path, env=env, check=True)
        subprocess.run([str(path / 'test'), '--nocapture', '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
