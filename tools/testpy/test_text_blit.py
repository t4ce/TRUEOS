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
mod allcaps {pub mod text_blit {pub const CPU_MONO_MAX_PIXELS:usize=1024;pub const CPU_COPY_MAX_PIXELS:usize=2048;pub const DIAGNOSTICS:bool=false;}}
mod virtio_gpu_logo {
 pub async fn copy_rgba(_:crate::intel::GucBcs0RgbaSurface,_:&[crate::intel::GucBcs0RgbaCopy])->Result<(), &'static str> {panic!("unexpected emulator copy")}
 pub async fn fill_rgba(_:crate::intel::GucBcs0RgbaSurface,_:&[crate::intel::GucBcs0RgbaFill])->Result<(), &'static str> {panic!("unexpected emulator fill")}
 std::thread_local! {pub static EMULATOR: std::cell::Cell<bool> = const {std::cell::Cell::new(false)};}
 pub fn output_dimensions()->Option<(u32,u32)> {EMULATOR.with(|v|v.get().then_some((1280,720)))}
}
mod time {pub fn tsc_hz()->u64 {1_000_000_000}}
mod phys {pub fn phys_to_virt(v:usize)->usize {v}}
mod percpu {pub fn current_slot()->u32 {0}}
mod intel {
#[derive(Clone,Copy)]pub struct GucBcs0RgbaSurface {pub phys:u64,pub gpu:u64,pub bytes:usize,pub width:u32,pub height:u32,pub pitch_bytes:u32}
#[derive(Clone,Copy)]pub struct GucBcs0MonoGlyph {pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub mask:[u8;64],pub foreground:u32,pub background:u32}
#[derive(Clone,Copy)]pub struct GucBcs0RgbaFill {pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub color:u32}
#[derive(Clone,Copy)]pub struct GucBcs0RgbaCopy {pub source:GucBcs0RgbaSurface,pub source_x:u32,pub source_y:u32,pub destination_x:u32,pub destination_y:u32,pub width:u32,pub height:u32}
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
    let mut source=vec![0;768*44];let src=surface(&mut source,192,44,768);
    let c=GucBcs0RgbaCopy {source:src,source_x:0,source_y:0,destination_x:0,destination_y:0,width:192,height:11};
    assert!(!unsafe{try_copy(wide,&[c],11)});
}
#[test]fn rgba_glyphs_keep_mono_budget_while_viewport_copies_keep_copy_budget() {
 let mut pixels=vec![0;1024*44];let dst=surface(&mut pixels,256,44,1024);
 let mut source=vec![0x80;1024*44];let src=surface(&mut source,256,44,1024);
 let copies:Vec<_>=(0..16).map(|i|GucBcs0RgbaCopy {source:src,source_x:0,source_y:0,destination_x:i*6,destination_y:0,width:6,height:11}).collect();
 // 1056 pixels fit the independent 2048px copy allowance, but exceed the
 // 1024px glyph allowance. This remains true when their caps are tuned apart.
 assert!(unsafe{try_copy(dst,&copies,11)});let before=pixels.clone();assert!(!unsafe{try_glyph_copy(dst,&copies)});assert_eq!(before,pixels);
 assert!(unsafe{try_glyph_copy(dst,&copies[..15])});
 let narrow=GucBcs0RgbaSurface {width:24,..dst};assert!(!unsafe{try_glyph_copy(narrow,&copies[..3])});
 // Use the first clipped glyph height, as mono_on_cpu does; a declared full
 // row height must not grant extra CPU work for a clipped bottom row.
 let clipped:Vec<_>=(0..13).map(|_|GucBcs0RgbaCopy {source:src,source_x:0,source_y:0,destination_x:0,destination_y:0,width:6,height:1}).collect();
 assert!(!unsafe{try_glyph_copy(GucBcs0RgbaSurface {width:24,..dst},&clipped)});
}
#[test]fn fills_and_uniform_mono_rows_match_reference_with_unaligned_storage() {
    for width in [1,3,6,12,16] {
        let mut allocation=vec![0x59;257*40+1];let mut expected=allocation.clone();
        let s=surface(&mut allocation[1..],48,40,257);
        let f=GucBcs0RgbaFill{x:3,y:2,width,height:11,color:0x44332211};
        for y in 2..13 {for x in 3..3+width as usize {let offset=1+y*257+x*4;expected[offset..offset+4].copy_from_slice(&f.color.to_le_bytes());}}
        assert!(unsafe{fill(s,&[f])});assert_eq!(allocation,expected);
        for mask in [[0;64],[255;64]] {
            let mut pixels=vec![0x59;257*40];let s=surface(&mut pixels,48,40,257);
            let mut g=glyph(width,11);g.mask=mask;
            let mut expected=pixels.clone();let color=if mask[0]==0 {g.background}else{g.foreground};
            for y in 2..13 {for x in 3..3+width as usize {let offset=y*257+x*4;expected[offset..offset+4].copy_from_slice(&color.to_le_bytes());}}
            assert!(unsafe{mono(s,&[g])});assert_eq!(pixels,expected);
        }
    }
}
#[test]fn fills_validate_every_rectangle_before_writing_and_keep_cpu_cutoff() {
    let mut pixels=vec![0x59;128*16];let s=surface(&mut pixels,32,16,128);
    let f=GucBcs0RgbaFill{x:0,y:0,width:6,height:11,color:42};let mut invalid=f;invalid.y=u32::MAX;
    let expected=pixels.clone();assert!(!unsafe{fill(s,&[f,invalid])});assert_eq!(pixels,expected);
    assert!(unsafe{try_fill(s,&[f])});
    let full=GucBcs0RgbaFill{width:32,..f};assert!(!unsafe{try_fill(s,&[full])});
}

#[test]fn emulator_copies_above_intel_limits_without_host_roundtrips() {
 virtio_gpu_logo::EMULATOR.with(|v|v.set(true));
 let mut pixels=vec![0x59;3072*44];let dst=surface(&mut pixels,768,44,3072);
 let mut source=vec![0x80;3072*44];let src=surface(&mut source,768,44,3072);
 let c=GucBcs0RgbaCopy {source:src,source_x:0,source_y:0,destination_x:0,destination_y:0,width:768,height:44};
 assert!(unsafe{try_glyph_copy(dst,&[c])});assert_eq!(pixels,source);
 pixels.fill(0x59);assert!(unsafe{try_copy(dst,&[c],11)});assert_eq!(pixels,source);
 let invalid=GucBcs0RgbaCopy{width:769,..c};let before=pixels.clone();assert!(!unsafe{try_glyph_copy(dst,&[invalid])});assert_eq!(pixels,before);
 virtio_gpu_logo::EMULATOR.with(|v|v.set(false));
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix='text-blit-') as directory:
        path=Path(directory)
        (path/'tests.rs').write_text(SOURCE.replace('__ROOT__',str(ROOT)))
        subprocess.run(['rustc','--edition=2024','--test',str(path/'tests.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests')],check=True)

if __name__=='__main__':main()
