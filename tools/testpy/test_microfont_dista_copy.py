#!/usr/bin/env python3
"""Run the production one-copy Dista mask sink against guarded host buffers."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

MOCKS = r'''
#![allow(dead_code)]
mod phys { pub fn phys_to_virt(phys: usize) -> usize { phys } }
mod ui4 {
    #[derive(Clone, Copy)]
    pub struct FrameRgbaView {
        pub phys: u64, pub virt: *mut u8, pub byte_len: usize,
        pub width: u32, pub height: u32, pub pitch: u32,
    }
}
mod intel {
    use std::sync::Mutex;
    pub static FLUSHES: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    pub struct DmaFlushRows;
    impl DmaFlushRows {
        pub fn new(_: *mut u8, _: usize, _: usize, _: usize) -> Self { Self }
    }
    pub fn dma_flush_strided_row_spans(spans: &[DmaFlushRows]) -> bool {
        FLUSHES.lock().unwrap().push(spans.len()); true
    }
    pub mod gpgpu {
        #[derive(Clone, Copy)]
        pub struct GpgpuMask8Surface {
            pub phys: u64, pub bytes: usize, pub width: u32, pub height: u32, pub pitch_bytes: u32,
        }
        impl GpgpuMask8Surface {
            pub fn is_valid(self) -> bool {
                self.width != 0 && self.height != 0 && self.pitch_bytes >= self.width
                    && (self.height as usize - 1).checked_mul(self.pitch_bytes as usize)
                    .and_then(|n| n.checked_add(self.width as usize)).is_some_and(|n| n <= self.bytes)
            }
        }
    }
}
'''

TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    const GUARD: usize = 64;
    struct Buffers {
        source: Vec<u8>, target: Vec<u8>, source_pitch: usize, target_pitch: usize,
        width: u32, height: u32, factor: u32,
    }
    impl Buffers {
        fn new(width: u32, height: u32, factor: u32, source_padding: usize, target_padding: usize) -> Self {
            let source_pitch = (width * factor) as usize + source_padding;
            let target_pitch = width as usize * 4 + target_padding;
            Self {
                source: vec![0xA7; source_pitch * (height * factor) as usize + GUARD*2],
                target: vec![0xD3; target_pitch * height as usize + GUARD*2],
                source_pitch, target_pitch, width, height, factor,
            }
        }
        fn set(&mut self, x: usize, y: usize, coverage: u8) {
            self.source[GUARD + y*self.source_pitch + x] = coverage;
        }
        fn mask(&self) -> GpgpuMask8Surface {
            GpgpuMask8Surface { phys: unsafe { self.source.as_ptr().add(GUARD) } as usize as u64,
                bytes: self.source.len()-GUARD*2, width: self.width*self.factor,
                height: self.height*self.factor, pitch_bytes: self.source_pitch as u32 }
        }
        fn view(&mut self) -> FrameRgbaView {
            let virt = unsafe { self.target.as_mut_ptr().add(GUARD) };
            FrameRgbaView { phys: virt as usize as u64, virt, byte_len: self.target.len()-GUARD*2,
                width: self.width, height: self.height, pitch: self.target_pitch as u32 }
        }
        fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
            self.target[GUARD+y*self.target_pitch+x*4..GUARD+y*self.target_pitch+x*4+4].try_into().unwrap()
        }
        fn copy(&mut self) -> Result<CopyStats, &'static str> {
            unsafe { copy_retired_mask_to_rgba(self.mask(), self.view(), self.factor) }
        }
        fn guards_intact(&self) -> bool {
            self.target[..GUARD].iter().chain(&self.target[self.target.len()-GUARD..]).all(|&b| b==0xD3)
                && (0..self.height as usize).all(|y| self.target[GUARD+y*self.target_pitch+self.width as usize*4
                    ..GUARD+(y+1)*self.target_pitch].iter().all(|&b| b==0xD3))
        }
    }

    #[test]
    fn one_copy_blends_retired_coverage_and_preserves_padding() {
        let mut b = Buffers::new(5,2,1,7,12);
        for y in 0..2 { for (x,c) in [0,64,128,191,255].into_iter().enumerate() { b.set(x,y,c); } }
        let source = b.source.clone();
        let stats = b.copy().unwrap();
        let expected = [[255,255,255,255],[203,216,245,255],[151,177,235,255],
                        [100,139,226,255],[48,100,216,255]];
        for y in 0..2 { for (x,rgba) in expected.into_iter().enumerate() { assert_eq!(b.pixel(x,y),rgba); } }
        assert_eq!(stats, CopyStats { output_pixels:10, source_samples:10, partial_coverage_pixels:6 });
        assert!(b.guards_intact()); assert_eq!(b.source,source);
    }
    #[test]
    fn two_by_two_resolve_has_expected_five_coverage_levels() {
        let mut b = Buffers::new(5,1,2,9,8);
        for x in 0..10 { for y in 0..2 { b.set(x,y,0); } }
        // Output boxes contain respectively 0,1,2,3,4 covered source samples.
        for pixel in 0..5 { for sample in 0..pixel {
            b.set(pixel*2+sample%2,sample/2,255);
        } }
        let stats=b.copy().unwrap();
        for (x,c) in [0,64,128,191,255].into_iter().enumerate() {
            assert_eq!(b.pixel(x,0), ink_over_white(c));
        }
        assert_eq!(stats,CopyStats { output_pixels:5,source_samples:20,partial_coverage_pixels:3 });
        assert!(b.guards_intact());
    }
    #[test]
    fn supersample_rows_and_columns_remain_separate() {
        let mut b=Buffers::new(2,2,2,5,4);
        for y in 0..4 { for x in 0..4 { b.set(x,y,if (x<2)==(y<2) {255} else {0}); } }
        b.copy().unwrap();
        assert_eq!(b.pixel(0,0),[48,100,216,255]); assert_eq!(b.pixel(1,0),[255;4]);
        assert_eq!(b.pixel(0,1),[255;4]); assert_eq!(b.pixel(1,1),[48,100,216,255]);
        assert!(b.guards_intact());
    }
    #[test]
    fn malformed_requests_never_flush_or_write_destination() {
        let mut b=Buffers::new(2,2,1,0,0);
        let mask=b.mask(); let view=b.view(); let before=b.target.clone();
        let flush_count=crate::intel::FLUSHES.lock().unwrap().len();
        for (m,v,scale) in [
            (mask,view,0),(mask,view,3),
            (GpgpuMask8Surface {width:1,..mask},view,1),
            (GpgpuMask8Surface {bytes:1,..mask},view,1),
            (GpgpuMask8Surface {phys:0,..mask},view,1),
            (mask,FrameRgbaView {phys:0,..view},1),
            (mask,FrameRgbaView {virt:core::ptr::null_mut(),..view},1),
            (mask,FrameRgbaView {pitch:7,..view},1),
            (mask,FrameRgbaView {byte_len:1,..view},1),
            (mask,FrameRgbaView {phys:u64::MAX-1,..view},1),
            (mask,FrameRgbaView {virt:(usize::MAX-1) as *mut u8,..view},1),
            (GpgpuMask8Surface {phys:u64::MAX-1,..mask},view,1),
        ] { assert!(unsafe {copy_retired_mask_to_rgba(m,v,scale)}.is_err()); }
        assert_eq!(b.target,before);
        assert_eq!(crate::intel::FLUSHES.lock().unwrap().len(),flush_count);
    }
    #[test]
    fn overlapping_physical_allocations_are_rejected_before_write() {
        let mut b=Buffers::new(2,2,1,0,0);
        let mask=b.mask(); let view=b.view(); let before=b.target.clone();
        for phys in [mask.phys,mask.phys+1,mask.phys-1] {
            assert_eq!(unsafe {copy_retired_mask_to_rgba(mask,FrameRgbaView {phys,..view},1)},Err("dista-copy-alias"));
        }
        assert_eq!(b.target,before);
    }
    #[test]
    fn cache_preparation_includes_both_buffers_and_publication_only_target() {
        crate::intel::FLUSHES.lock().unwrap().clear();
        let mut b=Buffers::new(1,1,1,0,0); b.set(0,0,255); b.copy().unwrap();
        assert_eq!(*crate::intel::FLUSHES.lock().unwrap(),vec![2,1]);
    }
}
'''


def main():
    production = (ROOT / "src/r/services/microfont_dista_copy.rs").read_text()
    production = "\n".join(line for line in production.splitlines() if not line.startswith("//!"))
    source = MOCKS + "\nmod sink {\n" + production + TESTS + "\n}\n"
    with tempfile.TemporaryDirectory(prefix="trueos-dista-copy-test-") as temporary:
        directory = Path(temporary)
        rust = directory / "tests.rs"
        rust.write_text(source)
        executable = directory / "tests"
        subprocess.run(["rustc", "--test", "--edition=2024", "--target", "x86_64-unknown-linux-gnu",
                        "-O", str(rust), "-o", str(executable)], check=True)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
