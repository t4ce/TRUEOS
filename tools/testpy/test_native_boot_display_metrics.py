#!/usr/bin/env python3
"""Exercise native boot EDID through the kernel's real font sizing policy.

Only the framebuffer response and active scanout are controlled by the harness.
The production EDID parser, physical conversion, UI4 policy, and Shell3 cell
authority run together, including the parser's embedded tests. No GPU or font
bundle is needed to verify the sizing gate before native rasterization.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code, unused_variables)]
extern crate self as microfont;
macro_rules! log_info {($($t:tt)*)=>{}}
macro_rules! log_warn {($($t:tt)*)=>{}}
pub(crate) use {log_info,log_warn};

mod limine {
    pub struct Framebuffer {pub width:u64,pub height:u64,edid:&'static [u8]}
    impl Framebuffer {pub fn edid(&self)->&[u8] {self.edid}}
    pub struct FramebufferResponse {buffers:Vec<&'static Framebuffer>}
    impl FramebufferResponse {pub fn framebuffers(&self)->&[&'static Framebuffer] {&self.buffers}}
    std::thread_local! {
        static RESPONSE:std::cell::Cell<Option<&'static FramebufferResponse>>=const {std::cell::Cell::new(None)};
    }
    pub fn framebuffer_response()->Option<&'static FramebufferResponse> {RESPONSE.get()}
    pub fn install(edid:&[u8]) {
        let edid=Box::leak(edid.to_vec().into_boxed_slice());
        let framebuffer=Box::leak(Box::new(Framebuffer {width:2560,height:1440,edid}));
        RESPONSE.set(Some(Box::leak(Box::new(FramebufferResponse {buffers:vec![framebuffer]}))));
    }
}

mod intel {
    #[derive(Clone,Copy)] pub struct Pipeline;
    impl Pipeline {pub fn name(self)->&'static str {"pipe-a"}}
    #[derive(Clone,Copy)] pub struct Ddi;
    impl Ddi {pub fn name(self)->&'static str {"ddi-b"}}
    #[derive(Clone,Copy)] pub struct Route {pub ddi:Ddi}
    #[derive(Clone,Copy)] pub struct DisplayPipelineTarget {
        pub width:u32,pub height:u32,pub pipeline:Pipeline,pub route:Route,
    }
    std::thread_local! {
        pub static MODE:std::cell::Cell<Option<(u32,u32)>>=const {std::cell::Cell::new(Some((2560,1440)))};
    }
    fn active_target()->Option<DisplayPipelineTarget> {
        let (width,height)=MODE.get()?;
        Some(DisplayPipelineTarget {width,height,pipeline:Pipeline,route:Route {ddi:Ddi}})
    }
    #[path=__NATIVE_METRICS__] mod display_metrics;
    pub fn physical_extent_pixels(width:u32,height:u32)->Option<(u32,u32)> {
        display_metrics::physical_extent_pixels(active_target()?,width,height)
    }
    pub fn physical_extent_pixels_fraction(width:u32,height:u32,denominator:u32)->Option<(u32,u32)> {
        display_metrics::physical_extent_pixels_fraction(active_target()?,width,height,denominator)
    }
}
mod virtio_gpu_logo {pub fn output_dimensions()->Option<(u32,u32)> {None}}
mod ui4 {
    #[path=__UI4_METRICS__] mod display_metrics;
    pub(crate) use display_metrics::{physical_extent_pixels,physical_extent_pixels_fraction};
}
'''

TESTS = r'''
fn checksum(edid:&mut [u8;128]) {
    edid[127]=0u8.wrapping_sub(edid[..127].iter().fold(0u8,|sum,&byte|sum.wrapping_add(byte)));
}
fn monitor_edid()->[u8;128] {
    // A 600 x 340 mm display whose first detailed timing is 2560 x 1440.
    let mut edid=[0u8;128];
    edid[..8].copy_from_slice(&[0,255,255,255,255,255,255,0]);
    edid[18]=1;edid[19]=4;edid[20]=0x80;
    edid[21]=60;edid[22]=34;
    edid[54]=1;edid[56]=0;edid[58]=0xa0;edid[59]=0xa0;edid[61]=0x50;
    edid[66]=0x58;edid[67]=0x54;edid[68]=0x21;
    checksum(&mut edid);edid
}
use r::services::shell3_font_service::cell_extent;

#[test] fn retained_boot_edid_enables_shell_text_and_home_glyph_sizing() {
    limine::install(&[]);
    assert_eq!(cell_extent(2),None,"the former loader handoff prevents atlas production");
    assert_eq!(ui4::physical_extent_pixels(START_GLYPH_EM_MM,START_GLYPH_EM_MM),None);
    limine::install(&monitor_edid());
    assert_eq!(cell_extent(2),Some((17,32)),"7.5 mm Shell3 em at the active 1440p mode");
    assert_eq!(ui4::physical_extent_pixels(START_GLYPH_EM_MM,START_GLYPH_EM_MM),Some((213,212)),
        "the private home glyph's 50 mm sizing gate is now available");
}

#[test] fn font_density_uses_current_scanout_not_the_edid_preferred_pixel_count() {
    limine::install(&monitor_edid());
    intel::MODE.set(Some((1280,720)));
    assert_eq!(cell_extent(2),Some((9,16)));
    intel::MODE.set(Some((2560,1440)));
    assert_eq!(cell_extent(2),Some((17,32)));
    intel::MODE.set(None);
    assert_eq!(cell_extent(2),None,"an EDID alone cannot make an absent display ready");
}

#[test] fn corrupt_or_size_free_edid_keeps_native_sizing_unavailable() {
    let mut edid=monitor_edid();edid[40]^=1;
    limine::install(&edid);
    assert_eq!(cell_extent(2),None,"a corrupt EDID must not supply physical dimensions");
    let mut edid=monitor_edid();
    edid[21]=0;edid[22]=0;edid[66..69].fill(0);checksum(&mut edid);
    limine::install(&edid);
    assert_eq!(cell_extent(2),None,"native displays retain the validated physical-size policy");
}

#[test] fn detailed_timing_and_centimeter_dimensions_both_restore_font_sizing() {
    let mut edid=monitor_edid();edid[21]=0;edid[22]=0;checksum(&mut edid);
    limine::install(&edid);
    assert_eq!(cell_extent(2),Some((17,32)));
    let mut edid=monitor_edid();edid[66..69].fill(0);checksum(&mut edid);
    limine::install(&edid);
    assert_eq!(cell_extent(2),Some((17,32)));
}
'''


def main():
    source = HARNESS.replace('__NATIVE_METRICS__', json.dumps(str(
        ROOT / 'src/intel/display/display_metrics.rs'))).replace(
        '__UI4_METRICS__', json.dumps(str(ROOT / 'src/ui4/display_metrics.rs')))
    source += ''.join(constant('vendor/microfont/src/lib.rs', name)
                      for name in ('FWIDTH', 'FHEIGHT'))
    source += constant('src/r/services/microfont_mask_service.rs', 'START_GLYPH_EM_MM')
    source += '\nmod r {pub mod services {pub mod shell3_font_service {\n'
    source += constant('src/r/services/shell3_font_service.rs', 'DEFAULT_UI4_FONT_SCALE')
    source += item('src/r/services/shell3_font_service.rs', 'cell_extent')
    source += '\n}}}\n' + TESTS
    with tempfile.TemporaryDirectory(prefix='trueos-native-boot-display-') as directory:
        work = Path(directory)
        harness = work / 'tests.rs'
        harness.write_text(source)
        binary = work / 'tests'
        subprocess.run([os.environ.get('RUSTC', 'rustc'), '--edition=2024', '--test',
                        str(harness), '-C', f'linker={ROOT / "tools/rust-host-ld"}',
                        '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
