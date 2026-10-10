#!/usr/bin/env python3
"""Production CPU text blits: pixel equivalence, bounds, alias and cutoff checks."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = r'''
#![allow(dead_code)]
extern crate alloc;
macro_rules! log_info {($($t:tt)*)=>{}}
pub(crate) use log_info;
mod allcaps {pub mod text_blit {pub const CPU_MONO_MAX_PIXELS:usize=1024;pub const CPU_COPY_MAX_PIXELS:usize=1024;pub const DIAGNOSTICS:bool=false;}}
mod time {pub fn tsc_hz()->u64 {1_000_000_000}}
mod phys {pub fn phys_to_virt(v:usize)->usize {v}}
mod percpu {pub fn current_slot()->u32 {0}}
mod intel {
#[derive(Clone,Copy)]pub struct GucBcs0RgbaSurface {pub phys:u64,pub gpu:u64,pub bytes:usize,pub width:u32,pub height:u32,pub pitch_bytes:u32}
pub struct GucBcs0MonoGlyph {pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub mask:[u8;64],pub foreground:u32,pub background:u32}
pub struct GucBcs0RgbaCopy {pub source:GucBcs0RgbaSurface,pub source_x:u32,pub source_y:u32,pub destination_x:u32,pub destination_y:u32,pub width:u32,pub height:u32}
pub struct DmaFlushRows;
impl DmaFlushRows {pub fn new(_: *mut u8,_:usize,_:usize,_:usize)->Self {Self}}
pub fn dma_flush_strided_row_spans(_: &[DmaFlushRows])->bool {true}
}
mod ui4 {
#[path="__ROOT__/src/ui4/text_blit.rs"]pub mod text_blit;
mod text_blit_bench {pub async fn run_once()->Result<(),&'static str>{Ok(())}}
}
use intel::*;use ui4::text_blit::*;
fn surface(pixels:&mut [u8],width:u32,height:u32,pitch:u32)->GucBcs0RgbaSurface {GucBcs0RgbaSurface {phys:pixels.as_mut_ptr() as u64,gpu:0,bytes:pixels.len(),width,height,pitch_bytes:pitch}}
fn glyph(width:u32,height:u32)->GucBcs0MonoGlyph {let mut mask=[0u8;64];for (i,b) in mask.iter_mut().enumerate(){*b=(i as u8).wrapping_mul(19)^0xa5;}GucBcs0MonoGlyph{x:3,y:2,width,height,mask,foreground:0xff332211,background:0xa0000000}}
#[test]fn pixels_match_mono_expansion_and_leave_padding_untouched() {
    for (w,h) in [(6,11),(12,22),(16,32)] {
        let mut pixels=vec![0x59;256*40];let s=surface(&mut pixels,48,40,256);let g=glyph(w,h);
        let mut expected=pixels.clone();for y in 0..h as usize {for x in 0..w as usize {
            let value=if g.mask[y*2+x/8]&(0x80>>(x%8))!=0 {g.foreground} else {g.background};
            let offset=(y+2)*256+(x+3)*4;expected[offset..offset+4].copy_from_slice(&value.to_le_bytes());
        }}
        assert!(unsafe{mono(s,&[g])});assert_eq!(pixels,expected);
    }
}
#[test]fn invalid_later_glyph_rejects_entire_operation_before_writing() {
    let mut pixels=vec![0x59;128*16];let s=surface(&mut pixels,32,16,128);let good=glyph(6,11);let mut bad=glyph(6,11);bad.x=u32::MAX;
    let before=pixels.clone();assert!(!unsafe{mono(s,&[good,bad])});assert_eq!(pixels,before);
    let mut broken=s;broken.bytes-=1;assert!(!unsafe{mono(broken,&[glyph(6,11)])});assert_eq!(pixels,before);
}
#[test]fn copy_handles_four_rectangles_with_independent_pitches() {
    let mut source:Vec<_>=(0..256*16).map(|i|i as u8).collect();let src=surface(&mut source,48,16,256);
    let mut pixels=vec![0x59;320*20];let dst=surface(&mut pixels,64,20,320);let mut expected=pixels.clone();
    let copies:Vec<_>=(0..4).map(|i|GucBcs0RgbaCopy {source:src,source_x:3+i*4,source_y:1+i,destination_x:2+i*8,destination_y:3+i,width:7,height:4}).collect();
    for c in &copies {for y in 0..4 {let s=(c.source_y as usize+y)*256+c.source_x as usize*4;let d=(c.destination_y as usize+y)*320+c.destination_x as usize*4;expected[d..d+28].copy_from_slice(&source[s..s+28]);}}
    assert!(unsafe{copy(dst,&copies)});assert_eq!(pixels,expected);
}
#[test]fn copy_rejects_alias_and_out_of_bounds_without_writes() {
    let mut pixels=vec![0x59;128*16];let dst=surface(&mut pixels,32,16,128);let before=pixels.clone();
    let c=GucBcs0RgbaCopy {source:dst,source_x:0,source_y:0,destination_x:0,destination_y:0,width:6,height:11};
    assert!(!unsafe{copy(dst,&[c])});assert_eq!(pixels,before);
    let mut source=vec![0;128*16];let src=surface(&mut source,32,16,128);
    let c=GucBcs0RgbaCopy {source:src,source_x:31,source_y:0,destination_x:0,destination_y:0,width:6,height:11};
    assert!(!unsafe{copy(dst,&[c])});assert_eq!(pixels,before);
}
#[test]fn selector_respects_measured_limit_and_half_row_ceiling() {
    let mut pixels=vec![0;768*44];let wide=surface(&mut pixels,192,44,768);
    let small:Vec<_>=(0..15).map(|_|glyph(6,11)).collect();assert!(unsafe{try_mono(wide,&small)});
    let large:Vec<_>=(0..16).map(|_|glyph(6,11)).collect();assert!(!unsafe{try_mono(wide,&large)});
    let narrow=GucBcs0RgbaSurface {width:24,..wide};let small:Vec<_>=(0..3).map(|_|glyph(6,11)).collect();assert!(!unsafe{try_mono(narrow,&small)});
    let mut source=vec![0;768*44];let src=surface(&mut source,192,44,768);
    let c=GucBcs0RgbaCopy {source:src,source_x:0,source_y:0,destination_x:0,destination_y:0,width:192,height:11};
    assert!(!unsafe{try_copy(wide,&[c],11)});
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix='text-blit-') as directory:
        path=Path(directory)
        (path/'tests.rs').write_text(SOURCE.replace('__ROOT__',str(ROOT)))
        subprocess.run(['rustc','--edition=2024','--test',str(path/'tests.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests')],check=True)

if __name__=='__main__':main()
