#!/usr/bin/env python3
"""Exercise production post-retirement R8/RGBA pixel checks on host memory."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

MOCKS = r'''
#![allow(dead_code)]
extern crate alloc;
mod phys { pub fn phys_to_virt(phys: usize) -> usize { phys } }
mod intel {
    pub fn dma_cache_flush_range(_: *const u8, _: usize) {}
    pub mod gpgpu {
        #[derive(Clone, Copy)]
        pub struct GpgpuMask8Surface {
            pub phys: u64, pub bytes: usize, pub width: u32, pub height: u32, pub pitch_bytes: u32,
        }
        #[derive(Clone, Copy)]
        pub struct GpgpuRgba8Surface {
            pub phys: u64, pub bytes: usize, pub width: u32, pub height: u32, pub pitch_bytes: u32,
        }
        impl GpgpuMask8Surface { pub fn is_valid(self) -> bool { self.width != 0 } }
        impl GpgpuRgba8Surface { pub fn is_valid(self) -> bool { self.width != 0 } }
    }
}
'''

TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    struct Targets { mask: Vec<u8>, rgba: Vec<u8>, mp: usize, rp: usize }
    impl Targets {
        fn new(padding: usize) -> Self {
            let mp = WIDTH as usize + padding;
            let rp = WIDTH as usize * 4 + padding;
            let mut s = Self { mask: vec![0; mp*HEIGHT as usize], rgba: vec![0; rp*HEIGHT as usize], mp, rp };
            for cell in 1..95 { s.paint_cell(cell, 255); }
            s
        }
        fn paint_cell(&mut self, cell: usize, value: u8) {
            let x = cell % COLUMNS * CELL_WIDTH + CELL_WIDTH/2;
            let y = cell / COLUMNS * CELL_HEIGHT + CELL_HEIGHT/2;
            self.mask[y*self.mp+x] = value;
            self.rgba[y*self.rp+x*4..y*self.rp+x*4+4].fill(value);
        }
        fn mask_surface(&self) -> GpgpuMask8Surface {
            GpgpuMask8Surface { phys: self.mask.as_ptr() as usize as u64, bytes: self.mask.len(),
                width: WIDTH, height: HEIGHT, pitch_bytes: self.mp as u32 }
        }
        fn rgba_surface(&self) -> GpgpuRgba8Surface {
            GpgpuRgba8Surface { phys: self.rgba.as_ptr() as usize as u64, bytes: self.rgba.len(),
                width: WIDTH, height: HEIGHT, pitch_bytes: self.rp as u32 }
        }
        fn audit(&self) -> Audit {
            let mask = unsafe { capture_r8_retired(self.mask_surface()).unwrap() };
            let rgba = unsafe { capture_rgba_retired(self.rgba_surface()).unwrap() };
            compare(&mask, &rgba)
        }
    }
    #[test]
    fn full_atlas_matches_and_row_padding_is_never_coverage() {
        let mut s = Targets::new(64);
        for y in 0..HEIGHT as usize {
            s.mask[y*s.mp+WIDTH as usize..(y+1)*s.mp].fill(127);
            s.rgba[y*s.rp+WIDTH as usize*4..(y+1)*s.rp].fill(127);
        }
        let a = s.audit();
        assert!(a.passed(), "{a:?}");
        assert_eq!(a.checked_pixels, 1536*1056);
        assert_eq!((a.nonzero_pixels, a.coverage_sum), (94, 94*255));
        assert_eq!(a.r8_hash, a.rgba_hash);
    }
    #[test]
    fn one_wrong_binary_pixel_fails_exact_comparison() {
        let mut s = Targets::new(0);
        s.mask[1] = 255;
        let a = s.audit();
        assert_eq!(a.mismatched_pixels, 1); assert!(!a.passed());
    }
    #[test]
    fn matching_nonbinary_images_still_fail_mask_contract() {
        let mut s = Targets::new(0);
        s.paint_cell(33, 128);
        let a = s.audit();
        assert_eq!(a.mismatched_pixels, 0);
        assert_eq!((a.nonbinary_pixels, a.rgba_nonbinary_pixels), (1, 1));
        assert!(!a.passed());
    }
    #[test]
    fn rgba_green_blue_and_alpha_are_checked() {
        for channel in 1..4 {
            let mut s = Targets::new(0);
            s.rgba[channel] = 255;
            let a = s.audit();
            assert_eq!(a.rgba_channel_mismatches, 1);
            assert_eq!(a.mismatched_pixels, 0); assert!(!a.passed());
        }
    }
    #[test]
    fn matching_empty_or_contaminated_cells_fail() {
        for cell in [0, 95] {
            let mut s = Targets::new(0);
            s.paint_cell(cell, 255);
            let a = s.audit();
            assert_eq!(a.mismatched_pixels, 0); assert!(!a.passed());
            assert_eq!(a.space_pixels + a.unused_pixels, 1);
        }
        let mut s = Targets::new(0); s.paint_cell(1, 0);
        let a = s.audit();
        assert_eq!((a.nonblank_glyphs, a.rgba_nonblank_glyphs), (93, 93));
        assert!(!a.passed());
    }
    #[test]
    fn malformed_extent_is_rejected_before_readback() {
        let s = Targets::new(0);
        for invalid in [
            GpgpuMask8Surface { phys: 0, ..s.mask_surface() },
            GpgpuMask8Surface { width: WIDTH-1, ..s.mask_surface() },
            GpgpuMask8Surface { height: HEIGHT-1, ..s.mask_surface() },
            GpgpuMask8Surface { pitch_bytes: WIDTH-1, ..s.mask_surface() },
            GpgpuMask8Surface { bytes: 1, ..s.mask_surface() },
        ] { assert!(unsafe { capture_r8_retired(invalid) }.is_err()); }
        let invalid = GpgpuRgba8Surface { bytes: 1, ..s.rgba_surface() };
        assert!(unsafe { capture_rgba_retired(invalid) }.is_err());
    }
    #[test]
    fn reversed_formats_cannot_be_reported_as_a_valid_comparison() {
        let s = Targets::new(0);
        let mask = unsafe { capture_r8_retired(s.mask_surface()).unwrap() };
        let rgba = unsafe { capture_rgba_retired(s.rgba_surface()).unwrap() };
        let a = compare(&rgba, &mask);
        assert_eq!(a.format_mismatches, 2); assert!(!a.passed());
    }
}
'''


def main():
    production = (ROOT / "src/r/services/microfont_dista_validate.rs").read_text()
    production = "\n".join(line for line in production.splitlines() if not line.startswith("//!"))
    source = MOCKS + "\nmod validation {\n" + production + TESTS + "\n}\n"
    with tempfile.TemporaryDirectory(prefix="trueos-dista-validation-test-") as temporary:
        directory = Path(temporary)
        rust = directory / "tests.rs"
        rust.write_text(source)
        executable = directory / "tests"
        subprocess.run(["rustc", "--test", "--edition=2024", "--target", "x86_64-unknown-linux-gnu",
                        "-O", str(rust), "-o", str(executable)], check=True)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
