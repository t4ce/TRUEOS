#!/usr/bin/env python3
"""Ordered local text plans: real CPU pixels plus modeled BCS admission/retirement."""
from pathlib import Path
import subprocess
import tempfile
from test_text_blit import SOURCE, ROOT

ADAPTER = r'''
extern crate self as trueos_time;
use std::future::Future;
pub struct Duration;impl Duration {pub fn from_millis(_:u64)->Self{Self}}
pub struct Timer;impl Timer {pub async fn after(_:Duration){}}
fn run<T>(future:impl Future<Output=T>)->T {
    let mut future=std::pin::pin!(future);
    loop {if let std::task::Poll::Ready(out)=future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())){return out;}}
}
static MODE:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
static MONO:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
static COPIES:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
static FILLS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
static FLUSHES:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
fn reset(){crate::virtio_gpu_logo::EMULATOR.with(|v|v.set(false));for counter in [&MODE,&MONO,&FILLS,&COPIES,&FLUSHES]{counter.store(0,std::sync::atomic::Ordering::SeqCst);}}
'''
GPU = r'''
pub const GUC_BCS0_MONO_MAX_GLYPHS:usize=64;
#[derive(Clone,Copy)]pub struct GucBcs0CopySubmission;
pub enum GucBcs0CopySubmitError {SubmitFailed,Busy}
pub enum GucBcs0CopyCompletion {Complete,Pending,Failed,InvalidSubmission}
pub fn dma_cache_flush_range(_: *const u8,_:usize){crate::FLUSHES.fetch_add(1,std::sync::atomic::Ordering::SeqCst);}
fn admitted()->Result<GucBcs0CopySubmission,GucBcs0CopySubmitError>{
    match crate::MODE.load(std::sync::atomic::Ordering::SeqCst){1=>Err(GucBcs0CopySubmitError::Busy),2=>Err(GucBcs0CopySubmitError::SubmitFailed),_=>Ok(GucBcs0CopySubmission)}
}
pub fn queue_guc_bcs0_mono_glyphs(s:GucBcs0RgbaSurface,g:&[GucBcs0MonoGlyph])->Result<GucBcs0CopySubmission,GucBcs0CopySubmitError>{
    let token=admitted()?;assert!(g.len()<=64);crate::MONO.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
    assert!(unsafe{crate::ui4::text_blit::mono(s,g)});Ok(token)
}
pub fn queue_guc_bcs0_legacy_rgba_copies(s:GucBcs0RgbaSurface,c:&[GucBcs0RgbaCopy])->Result<GucBcs0CopySubmission,GucBcs0CopySubmitError>{
    let token=admitted()?;assert!(c.len()<=128);crate::COPIES.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
    assert!(unsafe{crate::ui4::text_blit::copy(s,c)});Ok(token)
}
pub fn queue_guc_bcs0_rgba_fills(s:GucBcs0RgbaSurface,f:&[GucBcs0RgbaFill])->Result<GucBcs0CopySubmission,GucBcs0CopySubmitError>{
    let token=admitted()?;assert!(f.len()<=128);crate::FILLS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
    assert!(unsafe{crate::ui4::text_blit::fill(s,f)});Ok(token)
}
pub fn poll_guc_bcs0_rgba_copies(_:GucBcs0CopySubmission)->GucBcs0CopyCompletion{
    if crate::MODE.load(std::sync::atomic::Ordering::SeqCst)==3 {GucBcs0CopyCompletion::Failed} else {GucBcs0CopyCompletion::Complete}
}
'''
TESTS = r'''
// Convert the test's compact reference masks into owned immutable RGBA cells.
// Production uses only RgbaPaintPlan; these buffers retain every source address.
struct PaintPlan { plan: ui4::text_paint::RgbaPaintPlan, _sources: Vec<Vec<u8>> }
impl PaintPlan {
 fn new(glyphs:Vec<GucBcs0MonoGlyph>)->Self {let mut sources=Vec::new();let plan=rgba_plan(&glyphs,&mut sources);Self{plan,_sources:sources}}
 async fn paint(&self,s:GucBcs0RgbaSurface,p:&mut bool)->Result<ui4::text_paint::PaintStats,&'static str> {self.plan.paint(s,p).await}
}
fn reference(pixels:&mut [u8],s:GucBcs0RgbaSurface,glyphs:&[GucBcs0MonoGlyph]) {
    for g in glyphs {for y in 0..g.height as usize {for x in 0..g.width as usize {
        let color=if g.mask[y*2+x/8] & (0x80>>(x%8))!=0 {g.foreground}else{g.background};
        let offset=(g.y as usize+y)*s.pitch_bytes as usize+(g.x as usize+x)*4;
        pixels[offset..offset+4].copy_from_slice(&color.to_le_bytes());
    }}}
}
fn blank(x:u32,y:u32)->GucBcs0MonoGlyph {GucBcs0MonoGlyph{x,y,width:6,height:11,mask:[0;64],foreground:0xffeeeeee,background:0xff181818}}
#[test]fn whole_blank_row_is_one_fill_instead_of_eight_mono_batches(){
    reset();let mut pixels=vec![0x59;3072*44];let s=surface(&mut pixels,768,44,3072);
    let glyphs:Vec<_>=(0..512).map(|i|blank(i%128*6,i/128*11)).collect();
    let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);
    let mut poisoned=false;let stats=run(PaintPlan::new(glyphs).paint(s,&mut poisoned)).unwrap();
    assert_eq!(pixels,expected);assert!(!poisoned);assert_eq!(stats.copy_batches,0);assert_eq!(stats.fill_batches,1);
    assert_eq!(stats.gpu_pixels,512*6*11);assert_eq!(stats.cpu_pixels,0);
}
#[test]fn mixed_masks_clipping_and_overlaps_keep_exact_order(){
    for scale in [1,2] {for pattern in 0..7 {
        reset();let mut pixels=vec![0x59;257*44];let s=surface(&mut pixels,48,44,257);
        let mut glyphs=Vec::new();
        for i in 0..60 {let mut g=blank((i%6)*6,(i%3)*11);g.width=(6*scale).min(s.width-g.x);g.height=(11*scale).min(s.height-g.y);
            if (i+pattern)%3==0 {g.mask=[0xff;64];}else if (i+pattern)%3==1 {g.mask=glyph(g.width,g.height).mask;}
            if i%4==0 {g.background=0xff224488;}if i%5==0 {g.foreground=g.background;}
            glyphs.push(g);
        }
        let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);
        let mut poisoned=false;run(PaintPlan::new(glyphs).paint(s,&mut poisoned)).unwrap();
        assert_eq!(pixels,expected);assert!(!poisoned);
    }}
}
#[test]fn small_edits_stay_cpu_without_whole_frame_flush(){
    reset();let mut pixels=vec![0x59;768*44];let s=surface(&mut pixels,192,44,768);
    let glyphs=vec![glyph(6,11),blank(9,2),blank(15,2)];let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);
    let mut poisoned=false;let stats=run(PaintPlan::new(glyphs).paint(s,&mut poisoned)).unwrap();
    assert_eq!(pixels,expected);assert_eq!(stats.gpu_pixels,0);assert_eq!(stats.cpu_pixels,198);
    assert_eq!(FLUSHES.load(std::sync::atomic::Ordering::SeqCst),0);
}
#[test]fn large_clear_and_small_counters_use_one_fill_plus_bounded_cpu_work(){
    reset();let mut pixels=vec![0x59;3072*44];let s=surface(&mut pixels,768,44,3072);
    let mut glyphs=vec![glyph(6,11)];glyphs.extend((0..128).map(|i|blank(i*6,22)));glyphs.push(glyph(6,11));
    let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);
    let mut poisoned=false;let stats=run(PaintPlan::new(glyphs).paint(s,&mut poisoned)).unwrap();
    assert_eq!(pixels,expected);assert_eq!(stats.cpu_pixels,132);assert_eq!(stats.fill_batches,1);assert_eq!(stats.copy_batches,0);
}
#[test]fn invalid_later_cell_and_pinned_destination_write_nothing(){
    reset();let mut pixels=vec![0x59;128*16];let s=surface(&mut pixels,32,16,128);let before=pixels.clone();
    let mut bad=blank(0,0);bad.x=u32::MAX;
    let plan=PaintPlan::new(vec![blank(0,0),bad]);let mut poisoned=false;
    assert!(run(plan.paint(s,&mut poisoned)).is_err());assert_eq!(pixels,before);assert!(!poisoned);
    poisoned=true;assert!(run(PaintPlan::new(vec![blank(0,0)]).paint(s,&mut poisoned)).is_err());assert_eq!(pixels,before);
}
#[test]fn fragmented_solid_work_falls_back_without_extra_gpu_submissions(){
    reset();let mut pixels=vec![0x59;3072*22];let s=surface(&mut pixels,768,22,3072);
    let mut glyphs=Vec::new();for i in 0..64 {let mut g=blank(i*6,0);g.height=22;
        if i%2==0 {g.mask=glyph(6,22).mask;}glyphs.push(g);
    }
    let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);
    let mut poisoned=false;let stats=run(PaintPlan::new(glyphs).paint(s,&mut poisoned)).unwrap();
    assert_eq!(pixels,expected);assert_eq!(stats.copy_batches,1);assert_eq!(stats.fill_batches,0);assert_eq!(stats.cpu_pixels,0);
}
#[test]fn rejected_admission_is_retryable_but_uncertain_writes_pin_destination(){
    for mode in [1,2,3] {reset();MODE.store(mode,std::sync::atomic::Ordering::SeqCst);
        let mut pixels=vec![0x59;3072*22];let s=surface(&mut pixels,768,22,3072);let before=pixels.clone();
        let plan=PaintPlan::new((0..128).map(|i|blank(i*6,0)).collect());let mut poisoned=false;
        assert!(run(plan.paint(s,&mut poisoned)).is_err());assert_eq!(poisoned,mode!=1);
        if mode!=3 {assert_eq!(pixels,before);}
        if mode==1 {MODE.store(0,std::sync::atomic::Ordering::SeqCst);assert!(run(plan.paint(s,&mut poisoned)).is_ok());}
    }
}
fn rgba_plan(glyphs:&[GucBcs0MonoGlyph], sources:&mut Vec<Vec<u8>>)->ui4::text_paint::RgbaPaintPlan {
 let mut colors=std::collections::BTreeMap::new();let mut copies=Vec::new();
 for g in glyphs {
  let mut pixels=vec![0;(g.width*g.height*4) as usize];
  for y in 0..g.height {for x in 0..g.width {let color=if g.mask[(y*2+x/8) as usize]&(0x80>>(x%8))!=0 {g.foreground}else{g.background};let offset=((y*g.width+x)*4) as usize;pixels[offset..offset+4].copy_from_slice(&color.to_le_bytes());}}
  let first=u32::from_le_bytes(pixels[..4].try_into().unwrap());let uniform=pixels.chunks_exact(4).all(|p|p==first.to_le_bytes());
  let source=surface(&mut pixels,g.width,g.height,g.width*4);if uniform {colors.insert(source.phys,first);}sources.push(pixels);
  copies.push(GucBcs0RgbaCopy {source,source_x:0,source_y:0,destination_x:g.x,destination_y:g.y,width:g.width,height:g.height});
 }
 ui4::text_paint::RgbaPaintPlan::new(copies,|copy|colors.get(&copy.source.phys).copied())
}
#[test]fn rgba_blank_rows_keep_one_fill_and_no_glyph_copy_batches() {
 reset();let mut pixels=vec![0x59;3072*44];let s=surface(&mut pixels,768,44,3072);let glyphs:Vec<_>=(0..512).map(|i|blank(i%128*6,i/128*11)).collect();
 let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);let mut sources=Vec::new();let plan=rgba_plan(&glyphs,&mut sources);let mut p=false;let stats=run(plan.paint(s,&mut p)).unwrap();assert_eq!(pixels,expected);assert_eq!(stats.fill_batches,1);assert_eq!(stats.copy_batches,0);assert_eq!(stats.cpu_pixels,0);assert!(!p);
}
#[test]fn rgba_large_clear_and_small_counters_keep_shared_cpu_allowance() {
 reset();let mut pixels=vec![0x59;3072*44];let s=surface(&mut pixels,768,44,3072);let mut glyphs=vec![glyph(6,11)];glyphs.extend((0..128).map(|i|blank(i*6,22)));glyphs.push(glyph(6,11));
 let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);let mut sources=Vec::new();let plan=rgba_plan(&glyphs,&mut sources);let mut p=false;let stats=run(plan.paint(s,&mut p)).unwrap();assert_eq!(pixels,expected);assert_eq!(stats.cpu_pixels,132);assert_eq!(stats.fill_batches,1);assert_eq!(stats.copy_batches,0);
}
#[test]fn rgba_fragmented_fills_fall_back_to_one_legacy_copy_batch() {
 reset();let mut pixels=vec![0x59;3072*22];let s=surface(&mut pixels,768,22,3072);let mut glyphs=Vec::new();for i in 0..64 {let mut g=blank(i*6,0);g.height=22;if i%2==0 {g.mask=glyph(6,22).mask;}glyphs.push(g);}
 let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);let mut sources=Vec::new();let plan=rgba_plan(&glyphs,&mut sources);let mut p=false;let stats=run(plan.paint(s,&mut p)).unwrap();assert_eq!(pixels,expected);assert_eq!(stats.copy_batches,1);assert_eq!(stats.fill_batches,0);assert_eq!(stats.cpu_pixels,0);assert_eq!(MONO.load(std::sync::atomic::Ordering::SeqCst),0);
}
#[test]fn rgba_fractional_coverage_copies_survive_fill_boundaries_and_overlays() {
 reset();let mut pixels=vec![0x59;768*22];let s=surface(&mut pixels,192,22,768);let glyphs=[glyph(6,11),blank(9,2),glyph(6,11)];let mut sources=Vec::new();let plan=rgba_plan(&glyphs,&mut sources);
 // Alter only an immutable test source before submission; 128 is real
 // fractional coverage already tinted by the provider, never a mono mask.
 sources[0][..4].copy_from_slice(&[64,80,120,128]);sources[2][..4].copy_from_slice(&[64,80,120,128]);
 let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);let offset=2*768+3*4;expected[offset..offset+4].copy_from_slice(&[64,80,120,128]);
 let mut p=false;let stats=run(plan.paint(s,&mut p)).unwrap();assert_eq!(pixels,expected);assert_eq!(stats.cpu_pixels,198);assert_eq!(stats.gpu_pixels,0);assert_eq!(FLUSHES.load(std::sync::atomic::Ordering::SeqCst),0);
}
#[test]fn rgba_split_runs_share_one_cpu_budget_instead_of_multiplying_it() {
 reset();let mut pixels=vec![0x59;3072*22];let s=surface(&mut pixels,768,22,3072);let mut glyphs=Vec::new();for i in 0..64 {let mut g=blank(i*6,0);if i%2==0 {g.mask=glyph(6,11).mask;}glyphs.push(g);}
 let mut sources=Vec::new();let plan=rgba_plan(&glyphs,&mut sources);let mut p=false;let stats=run(plan.paint(s,&mut p)).unwrap();assert!(stats.cpu_pixels<=1024);assert!(stats.gpu_pixels>0);
}
#[test]fn rgba_paint_admission_and_uncertainty_preserve_original_ownership_rules() {
 for mode in [1,2,3] {reset();MODE.store(mode,std::sync::atomic::Ordering::SeqCst);let mut pixels=vec![0x59;3072*22];let s=surface(&mut pixels,768,22,3072);let glyphs:Vec<_>=(0..128).map(|i|{let mut g=glyph(6,11);g.x=i*6;g}).collect();let mut sources=Vec::new();let plan=rgba_plan(&glyphs,&mut sources);let mut p=false;assert!(run(plan.paint(s,&mut p)).is_err());assert_eq!(p,mode!=1);if mode==1 {MODE.store(0,std::sync::atomic::Ordering::SeqCst);assert!(run(plan.paint(s,&mut p)).is_ok());}}
}

#[test]fn emulator_large_terminal_paint_is_local_pixel_exact_and_never_submits() {
 reset();virtio_gpu_logo::EMULATOR.with(|v|v.set(true));
 let mut pixels=vec![0x59;3072*22];let s=surface(&mut pixels,768,22,3072);
 let glyphs:Vec<_>=(0..256).map(|i|{let mut g=if i%3==0 {blank((i%128)*6,(i/128)*11)} else {let mut g=glyph(6,11);g.x=(i%128)*6;g.y=(i/128)*11;g};g.foreground=0xff806040;g}).collect();
 let mut expected=pixels.clone();reference(&mut expected,s,&glyphs);
 let mut sources=Vec::new();let plan=rgba_plan(&glyphs,&mut sources);let mut p=false;
 MODE.store(2,std::sync::atomic::Ordering::SeqCst); // Any GPU admission would fail.
 let stats=run(plan.paint(s,&mut p)).unwrap();assert_eq!(pixels,expected);assert!(!p);
 assert_eq!(stats.gpu_pixels,0);assert_eq!(stats.copy_batches,0);assert_eq!(stats.fill_batches,0);assert_eq!(stats.cpu_pixels,256*66);
 for counter in [&COPIES,&FILLS,&FLUSHES] {assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst),0);}
 virtio_gpu_logo::EMULATOR.with(|v|v.set(false));
}
'''

def main():
    source=SOURCE[:SOURCE.index('#[test]')]
    source=source.replace('extern crate alloc;','extern crate alloc;\n'+ADAPTER,1)
    source=source.replace('mod intel {','mod intel {\n'+GPU,1)
    source=source.replace('mod text_blit_bench {', '#[path="__ROOT__/src/ui4/text_paint.rs"]pub mod text_paint;\nmod text_blit_bench {',1)
    source+=TESTS
    with tempfile.TemporaryDirectory(prefix='text-paint-') as directory:
        path=Path(directory);(path/'tests.rs').write_text(source.replace('__ROOT__',str(ROOT)))
        subprocess.run(['rustc','--edition=2024','--test',str(path/'tests.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests'),'--test-threads=1'],check=True)

if __name__=='__main__':main()
