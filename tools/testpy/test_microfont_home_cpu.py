#!/usr/bin/env python3
"""Run the CPU home renderer against the actual private MicroFont line asset."""
import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    mask_source = (ROOT / "src/r/services/microfont_mask_service.rs").read_text()
    # Retain the production geometry, sizing and output type. Only the libm
    # host transport is adapted; the renderer and asset decoder are unmodified.
    config = mask_source[mask_source.index("pub(crate) const ATLAS_COLUMNS"):
                         mask_source.index("pub(crate) fn glyph_slot")]
    glyph = re.search(r"pub\(crate\) struct OwnedGlyphMask \{.*?^\}",
                      mask_source, re.MULTILINE | re.DOTALL)
    assert glyph
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as libm;
pub fn floorf(value: f32) -> f32 { value.floor() }
pub fn ceilf(value: f32) -> f32 { value.ceil() }
pub fn roundf(value: f32) -> f32 { value.round() }
pub fn sqrtf(value: f32) -> f32 { value.sqrt() }
mod microfont_mask_service {
    use alloc::vec::Vec;
    @CONFIG@
    @GLYPH@
}
#[path = "@SOURCE@"] mod home;

#[test]
fn accepts_native_sizes_and_keeps_the_retained_mask_bounded() {
    for em in [11, 22, 55, 100, 253, 1000, 4000] {
        let mask = home::build(em).unwrap();
        let config = microfont_mask_service::AtlasConfig {
            glyph_height_px: Some(em), line_width_px: 6.0,
            ..microfont_mask_service::AtlasConfig::default()
        };
        assert_eq!((mask.width, mask.height), config.cell_extent());
        assert_eq!(mask.em_height_px, em);
        assert_eq!(mask.pixels.len(), mask.width as usize * mask.height as usize);
        assert!(mask.pixels.iter().any(|&alpha| alpha == 255));
        assert!(mask.pixels.iter().any(|&alpha| alpha > 0 && alpha < 255));
        let width = mask.width as usize;
        assert!(mask.pixels[..width].iter().all(|&alpha| alpha == 0));
        assert!(mask.pixels[mask.pixels.len()-width..].iter().all(|&alpha| alpha == 0));
        for row in mask.pixels.chunks_exact(width) {
            assert_eq!(row[0], 0);
            assert_eq!(row[width - 1], 0);
        }
    }
}

#[test]
fn rejects_invalid_sizes_before_allocating() {
    for em in [0, 1, 10, 4001, u32::MAX] {
        assert!(matches!(home::build(em), Err("microfont-single-glyph-size")));
    }
}

#[test]
fn every_pixel_matches_the_private_asset_and_stroke_origin() {
    // An independent f64 all-segment oracle checks clipping, gutter placement,
    // fixed six-pixel stroke width, endpoint caps and shared-segment overlap.
    // In particular the source is the private asset, not blank U+2302.
    for em in [11, 22, 55] {
        let mask = home::build(em).unwrap();
        let scale = f64::from(em) / 11.0;
        let gutter = f64::from(mask.height - em) * 0.5;
        let mut segments = Vec::new();
        for strip in microfont::raster_line::home_button_strips().unwrap() {
            for index in 0..strip.segments() {
                let point = |index| {
                    let p = strip.point(index).unwrap();
                    [(f64::from(p.x()) + 0.5) * scale + gutter,
                     (f64::from(p.y()) + 0.5) * scale + gutter]
                };
                segments.push((point(index), point(index + 1)));
            }
        }
        assert!(!segments.is_empty());
        for y in 0..mask.height {
            for x in 0..mask.width {
                let px = f64::from(x) + 0.5;
                let py = f64::from(y) + 0.5;
                let distance = segments.iter().map(|(a, b)| {
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let length = dx * dx + dy * dy;
                    let t = if length == 0.0 { 0.0 }
                        else { (((px-a[0])*dx + (py-a[1])*dy) / length).clamp(0.0, 1.0) };
                    ((px-a[0]-t*dx).powi(2) + (py-a[1]-t*dy).powi(2)).sqrt()
                }).fold(f64::INFINITY, f64::min);
                let expected = ((3.5-distance).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let actual = mask.pixels[(y * mask.width + x) as usize];
                assert!(actual.abs_diff(expected) <= 1,
                    "em={em}, ({x},{y}), expected={expected}, actual={actual}");
            }
        }
    }
}
'''.replace("@CONFIG@", config).replace("@GLYPH@", glyph.group()) \
        .replace("@SOURCE@", str(ROOT / "src/r/services/microfont_home_cpu.rs"))
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    rustc = ["rustc", "+" + channel.group(1), "--edition=2024",
             "--target=x86_64-unknown-linux-gnu", "-Adead_code",
             "-C", "linker=" + str(ROOT / "tools/rust-host-ld")]
    with tempfile.TemporaryDirectory(prefix="trueos-home-cpu-") as directory:
        work = Path(directory)
        vendor = ROOT / "vendor/microfont"
        subprocess.run(rustc + [str(vendor / "build.rs"), "-o", str(work / "pack")],
                       cwd=ROOT, check=True)
        env = os.environ.copy()
        env.pop("CARGO_FEATURE_HEX", None)
        env.pop("MICROFONT_SOURCE_DIR", None)
        env.update(CARGO_MANIFEST_DIR=str(vendor), OUT_DIR=str(work / "font"),
                   CARGO_FEATURE_RASTER="1", CARGO_CFG_TARGET_OS="linux")
        subprocess.run([str(work / "pack")], env=env, cwd=ROOT,
                       check=True, stdout=subprocess.DEVNULL)
        library = work / "libmicrofont.rlib"
        subprocess.run(rustc + ["--crate-type=rlib", "--crate-name=microfont",
                               "--cfg", 'feature="raster"', str(vendor / "src/lib.rs"),
                               "-o", str(library)], env=env, cwd=ROOT, check=True)
        fixture = work / "tests.rs"
        fixture.write_text(harness)
        binary = work / "tests"
        subprocess.run(rustc + ["--test", "-O", str(fixture),
                               "--extern", "microfont=" + str(library),
                               "-o", str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
