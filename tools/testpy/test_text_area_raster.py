#!/usr/bin/env python3
"""Host tests for production text-area async BCS ownership and fault handling.

The mock backend records admission, pending retirement, physical frees and
quarantines, and applies successful glyph/copy operations to independent pixels.
No GPU execution or hardware retirement is claimed by this host harness.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use std::{future::Future,pin::Pin,sync::{Arc,Mutex},task::{Context,Poll,Wake,Waker}};
pub struct Duration;impl Duration {pub fn from_millis(_:u64)->Self {Self}}
pub struct Timer;pub struct Yield(bool);
impl Timer {pub fn after(_:Duration)->Yield {Yield(false)}}
impl Future for Yield {type Output=();fn poll(mut self:Pin<&mut Self>,cx:&mut Context<'_>)->Poll<()> {if self.0 {Poll::Ready(())} else {self.0=true;cx.waker().wake_by_ref();Poll::Pending}}}
struct Nop;impl Wake for Nop {fn wake(self:Arc<Self>) {}}
fn context()->Context<'static> {Context::from_waker(Box::leak(Box::new(Waker::from(Arc::new(Nop)))))}
fn run<F:Future>(f:F)->F::Output {let mut f=std::pin::pin!(f);let mut cx=context();loop {if let Poll::Ready(v)=f.as_mut().poll(&mut cx) {return v;}}}
mod ui4 {
#[path="__TRUEOS_ROOT__/src/ui4/text_area.rs"] pub mod text_area;
#[path="__TRUEOS_ROOT__/src/ui4/text_area_raster.rs"] pub mod text_area_raster;
}
mod intel {
use super::*;use std::collections::{HashMap,VecDeque};
#[derive(Clone,Copy,Debug)] pub struct GucBcs0RgbaSurface {pub phys:u64,pub gpu:u64,pub bytes:usize,pub width:u32,pub height:u32,pub pitch_bytes:u32}
#[derive(Clone,Debug)] pub struct GucBcs0RgbaCopy {pub source:GucBcs0RgbaSurface,pub source_x:u32,pub source_y:u32,pub destination_x:u32,pub destination_y:u32,pub width:u32,pub height:u32}
#[derive(Clone,Debug)] pub struct GucBcs0MonoGlyph {pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub value:u32}
pub const GUC_BCS0_MONO_MAX_GLYPHS:usize=64;
#[derive(Clone,Copy,Debug)]pub struct GucBcs0CopySubmission(pub usize);
#[derive(Clone,Copy,Debug)]pub enum GucBcs0CopySubmitError {SubmitFailed,Busy}
#[derive(Clone,Copy,Debug,PartialEq)]pub enum GucBcs0CopyCompletion {Pending,Complete,Failed,InvalidSubmission}
#[derive(Clone,Copy,Debug)]pub enum Mode {Complete,Busy,SubmitFailed,Failed,Invalid}
#[derive(Clone,Debug)]pub enum Op {Mono(GucBcs0RgbaSurface,Vec<GucBcs0MonoGlyph>),Copy(GucBcs0RgbaSurface,Vec<GucBcs0RgbaCopy>)}
#[derive(Default)]pub struct State {pub modes:VecDeque<Mode>,pub events:Vec<String>,pub memory:HashMap<u64,Vec<u32>>,pending:HashMap<usize,(Op,Mode,bool)>,next:u64,pub fail_alloc:bool,pub size_mismatch:bool}
pub static S:Mutex<Option<State>>=Mutex::new(None);
pub fn reset() {*S.lock().unwrap()=Some(State::default());}
pub fn state<R>(f:impl FnOnce(&mut State)->R)->R {f(S.lock().unwrap().as_mut().unwrap())}
fn queue(op:Op)->Result<GucBcs0CopySubmission,GucBcs0CopySubmitError> {state(|s| {let mode=s.modes.pop_front().unwrap_or(Mode::Complete);s.events.push(format!("queue {mode:?}"));if matches!(mode,Mode::Busy) {return Err(GucBcs0CopySubmitError::Busy);}if matches!(mode,Mode::SubmitFailed) {return Err(GucBcs0CopySubmitError::SubmitFailed);}let id=s.events.len();s.pending.insert(id,(op,mode,false));Ok(GucBcs0CopySubmission(id))})}
pub fn queue_guc_bcs0_mono_glyphs(d:GucBcs0RgbaSurface,g:&[GucBcs0MonoGlyph])->Result<GucBcs0CopySubmission,GucBcs0CopySubmitError> {queue(Op::Mono(d,g.to_vec()))}
pub fn queue_guc_bcs0_rgba_copies(d:GucBcs0RgbaSurface,c:&[GucBcs0RgbaCopy])->Result<GucBcs0CopySubmission,GucBcs0CopySubmitError> {for c in c {assert_ne!(d.phys,c.source.phys);assert!(c.source_x+c.width<=c.source.width&&c.source_y+c.height<=c.source.height);assert!(c.destination_x+c.width<=d.width&&c.destination_y+c.height<=d.height);}queue(Op::Copy(d,c.to_vec()))}
pub fn poll_guc_bcs0_rgba_copies(id:GucBcs0CopySubmission)->GucBcs0CopyCompletion {state(|s| {let (op,mode,polled)=s.pending.get_mut(&id.0).unwrap();if !*polled {*polled=true;s.events.push("pending".into());return GucBcs0CopyCompletion::Pending;}let op=op.clone();let mode=*mode;if matches!(mode,Mode::Complete) {match op {Op::Mono(d,g)=>{let dst=s.memory.get_mut(&d.phys).unwrap();for g in g {for y in g.y..g.y+g.height {for x in g.x..g.x+g.width {dst[(y*d.width+x) as usize]=g.value;}}}},Op::Copy(d,copies)=>{for c in copies {let src=s.memory[&c.source.phys].clone();let dst=s.memory.get_mut(&d.phys).unwrap();for y in 0..c.height {for x in 0..c.width {dst[((c.destination_y+y)*d.width+c.destination_x+x) as usize]=src[((c.source_y+y)*c.source.width+c.source_x+x) as usize];}}}}}s.events.push("retired".into());GucBcs0CopyCompletion::Complete} else {s.events.push("uncertain".into());if matches!(mode,Mode::Failed) {GucBcs0CopyCompletion::Failed} else {GucBcs0CopyCompletion::InvalidSubmission}}})}
pub mod gpgpu {
use super::*;
pub struct GpgpuOwnedRgba8Surface {s:GucBcs0RgbaSurface,quarantined:bool}
impl GpgpuOwnedRgba8Surface {pub fn surface(&self)->GucBcs0RgbaSurface {self.s}pub fn quarantine_backing(&mut self) {if !self.quarantined {self.quarantined=true;state(|s|s.events.push(format!("quarantine {}",self.s.phys)));}}}
impl Drop for GpgpuOwnedRgba8Surface {fn drop(&mut self) {state(|s|s.events.push(format!("{} {}",if self.quarantined {"retained"} else {"free"},self.s.phys)));}}
pub fn allocate_font_instance_rgba8_surface(w:u32,h:u32)->Option<GpgpuOwnedRgba8Surface> {state(|s|{if s.fail_alloc {return None;}s.next+=1;let phys=s.next;let pitch=(w*4+63)&!63;let bytes=(pitch as usize*h as usize+4095)&!4095;let surf=GucBcs0RgbaSurface {phys,gpu:phys,bytes:bytes+usize::from(s.size_mismatch),width:w,height:h,pitch_bytes:pitch};s.memory.insert(phys,vec![0xdeadbeef;(w*h) as usize]);s.events.push(format!("alloc {phys}"));Some(GpgpuOwnedRgba8Surface {s:surf,quarantined:false})})}
}
}
use ui4::text_area::*;use ui4::text_area_raster::*;
fn rect(x:i64,y:i64,c:u32,r:u32)->CellRect {CellRect{x,y,columns:c,rows:r}}
fn layout(v:CellRect)->RasterLayout {RasterLayout::fit(v,(1,1),4,2*1024*1024).unwrap()}
fn glyph(w:CellWrite<u32>,l:RasterLayout)->intel::GucBcs0MonoGlyph {intel::GucBcs0MonoGlyph{x:w.x*l.cell_width,y:w.y*l.cell_height,width:l.cell_width,height:l.cell_height,value:w.cell}}
fn value(x:i64,y:i64)->u32 {((x+10000) as u32).wrapping_add(((y+10000) as u32)*100000)}
fn dest(w:u32,h:u32)->intel::gpgpu::GpgpuOwnedRgba8Surface {intel::gpgpu::allocate_font_instance_rgba8_surface(w,h).unwrap()}
#[test]fn cap_and_allocation_failures_release_reservations() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(l.bytes*2);let a=BcsTextArea::<u32>::new(l,&b).unwrap();let c=BcsTextArea::<u32>::new(l,&b).unwrap();assert!(BcsTextArea::<u32>::new(l,&b).is_none());assert_eq!(b.used(),l.bytes*2);drop(a);drop(c);assert_eq!(b.used(),0);intel::state(|s|s.fail_alloc=true);assert!(BcsTextArea::<u32>::new(l,&b).is_none());assert_eq!(b.used(),0);intel::state(|s|{s.fail_alloc=false;s.size_mismatch=true;});assert!(BcsTextArea::<u32>::new(l,&b).is_none());assert_eq!(b.used(),0);}
#[test]fn pan_copy_wrap_resize_and_pending_lifetime() {intel::reset();let v=rect(-7,-5,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;let work=run(a.render(v,1,value,glyph,&mut p)).unwrap();assert_eq!(work.produced,88*30);let same=run(a.render(v,1,|_,_|panic!("unexpected producer"),glyph,&mut p)).unwrap();assert_eq!(same.produced,0);let v2=rect(-7,-4,80,22);assert_eq!(run(a.render(v2,1,value,glyph,&mut p)).unwrap().produced,88);let d=dest(80,22);assert!(run(a.copy_view(v2,d.surface(),(0,0),&mut p)).unwrap()<=4);intel::state(|s|{for y in 0..22 {for x in 0..80 {assert_eq!(s.memory[&d.surface().phys][y*80+x],value(v2.x+x as i64,v2.y+y as i64));}}});let v3=rect(-9,-9,95,35);let l2=layout(v3);{let mut future=std::pin::pin!(a.resize(l2,v3,&b,&mut p));assert!(matches!(future.as_mut().poll(&mut context()),Poll::Pending));assert_eq!(b.used(),l.bytes+l2.bytes);assert!(!intel::state(|s|s.events.iter().any(|e|e=="free 1")));assert_eq!(run(future),Ok(true));}assert_eq!(b.used(),l2.bytes);assert!(intel::state(|s|s.events.iter().any(|e|e=="free 1")));run(a.render(v3,1,value,glyph,&mut p)).unwrap();let d2=dest(95,35);run(a.copy_view(v3,d2.surface(),(0,0),&mut p)).unwrap();intel::state(|s|{for y in 0..35 {for x in 0..95 {assert_eq!(s.memory[&d2.surface().phys][y*95+x],value(v3.x+x as i64,v3.y+y as i64));}}});drop(a);assert_eq!(b.used(),0);}
#[test]fn partial_admission_failure_invalidates_and_retries() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;intel::state(|s|s.modes.extend([intel::Mode::Complete,intel::Mode::Busy]));assert_eq!(run(a.render(v,1,value,glyph,&mut p)).err(),Some("text-area-admission"));assert!(!p);let work=run(a.render(v,1,value,glyph,&mut p)).unwrap();assert_eq!(work.produced,2640);assert_eq!(work.painted,2640);drop(a);assert_eq!(b.used(),0);}
#[test]fn ambiguous_render_quarantines_reservation() {for mode in [intel::Mode::SubmitFailed,intel::Mode::Failed,intel::Mode::Invalid] {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;intel::state(|s|s.modes.push_back(mode));assert!(run(a.render(v,1,value,glyph,&mut p)).is_err());assert!(p);drop(a);assert_eq!(b.used(),l.bytes);assert!(!intel::state(|s|s.events.iter().any(|e|e.starts_with("free"))));}}
#[test]fn ambiguous_resize_quarantines_both_allocations() {for mode in [intel::Mode::SubmitFailed,intel::Mode::Failed,intel::Mode::Invalid] {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;run(a.render(v,1,value,glyph,&mut p)).unwrap();let v2=rect(0,0,95,35);let l2=layout(v2);intel::state(|s|s.modes.push_back(mode));assert!(run(a.resize(l2,v2,&b,&mut p)).is_err());assert!(p);drop(a);assert_eq!(b.used(),l.bytes+l2.bytes);assert_eq!(intel::state(|s|s.events.iter().filter(|e|e.starts_with("quarantine")).count()),2);assert!(!intel::state(|s|s.events.iter().any(|e|e.starts_with("free"))));}}
#[test]fn resize_busy_releases_only_new_destination() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;run(a.render(v,1,value,glyph,&mut p)).unwrap();let v2=rect(0,0,95,35);let l2=layout(v2);intel::state(|s|s.modes.push_back(intel::Mode::Busy));assert!(run(a.resize(l2,v2,&b,&mut p)).is_err());assert!(!p);assert_eq!(a.layout(),l);assert_eq!(b.used(),l.bytes);assert!(intel::state(|s|s.events.iter().any(|e|e=="free 2")));drop(a);assert_eq!(b.used(),0);}
#[test]fn copy_uncertainty_retains_source_but_requires_destination_owner() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;run(a.render(v,1,value,glyph,&mut p)).unwrap();let mut d=dest(80,22);intel::state(|s|s.modes.push_back(intel::Mode::Failed));assert!(run(a.copy_view(v,d.surface(),(0,0),&mut p)).is_err());assert!(p);assert_eq!(intel::state(|s|s.events.iter().filter(|e|e.starts_with("quarantine")).count()),1);d.quarantine_backing();drop(d);drop(a);assert_eq!(b.used(),l.bytes);assert!(!intel::state(|s|s.events.iter().any(|e|e.starts_with("free"))));}
#[test]fn cancellation_render_must_not_free_inflight_source() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;{let mut future=Box::pin(a.render(v,1,value,glyph,&mut p));assert!(matches!(future.as_mut().poll(&mut context()),Poll::Pending));drop(future);}assert!(p);drop(a);let events=intel::state(|s|s.events.clone());println!("render cancellation events={events:?} budget_used={}",b.used());assert!(!events.iter().any(|e|e=="free 1"),"in-flight source freed on cancellation");assert_eq!(b.used(),l.bytes);}
#[test]fn cancellation_resize_must_not_free_inflight_destination() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;run(a.render(v,1,value,glyph,&mut p)).unwrap();let v2=rect(0,0,95,35);let l2=layout(v2);{let mut future=Box::pin(a.resize(l2,v2,&b,&mut p));assert!(matches!(future.as_mut().poll(&mut context()),Poll::Pending));drop(future);}assert!(p);let events=intel::state(|s|s.events.clone());println!("resize cancellation events_tail={:?} budget_used={}",&events[events.len().saturating_sub(5)..],b.used());assert!(!events.iter().any(|e|e=="free 2"),"in-flight destination freed on cancellation");assert_eq!(b.used(),l.bytes+l2.bytes);}
#[test]fn cancellation_copy_must_not_free_inflight_source() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;run(a.render(v,1,value,glyph,&mut p)).unwrap();let mut d=dest(80,22);{let mut future=Box::pin(a.copy_view(v,d.surface(),(0,0),&mut p));assert!(matches!(future.as_mut().poll(&mut context()),Poll::Pending));drop(future);}assert!(p);d.quarantine_backing();drop(d);drop(a);assert!(!intel::state(|s|s.events.iter().any(|e|e.starts_with("free"))));assert_eq!(b.used(),l.bytes);}
#[test]fn many_ring_positions_produce_exact_clipped_destination_pixels() {intel::reset();let base=rect(0,0,13,7);let l=layout(base);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;for (x,y) in [(-99,-21),(-14,-7),(0,0),(1,1),(20,14),(400,-93),(i64::MAX-100,i64::MAX-100)] {let v=rect(x,y,13,7);run(a.render(v,1,|x,y|x.wrapping_mul(31).wrapping_add(y) as u32,glyph,&mut p)).unwrap();let d=dest(12,8);let count=run(a.copy_view(v,d.surface(),(2,3),&mut p)).unwrap();assert!(count<=4);intel::state(|s|{let pixels=&s.memory[&d.surface().phys];for dy in 0..8 {for dx in 0..12 {let expected=if dx>=2&&dy>=3 {(v.x+(dx-2) as i64).wrapping_mul(31).wrapping_add(v.y+(dy-3) as i64) as u32} else {0xdeadbeef};assert_eq!(pixels[dy*12+dx],expected);}}});}drop(a);assert_eq!(b.used(),0);}

#[test]fn six_regions_share_exact_budget_and_reject_seventh() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(l.bytes*6);let mut areas=Vec::new();for _ in 0..6 {areas.push(BcsTextArea::<u32>::new(l,&b.clone()).unwrap());}assert_eq!(b.used(),l.bytes*6);assert!(BcsTextArea::<u32>::new(l,&b).is_none());areas.clear();assert_eq!(b.used(),0);let too_small=RasterBudget::new(l.bytes-1);assert!(BcsTextArea::<u32>::new(l,&too_small).is_none());assert_eq!(too_small.used(),0);}
#[test]fn overflowed_copy_origin_fails_before_admission() {intel::reset();let v=rect(20,20,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;run(a.render(v,1,value,glyph,&mut p)).unwrap();let d=dest(80,22);let before=intel::state(|s|s.events.iter().filter(|e|e.starts_with("queue")).count());assert_eq!(run(a.copy_view(v,d.surface(),(u32::MAX-1,u32::MAX-1),&mut p)),Err("text-area-copy-origin"));assert!(!p);assert_eq!(before,intel::state(|s|s.events.iter().filter(|e|e.starts_with("queue")).count()));}
#[test]fn font_resize_skips_overlap_transfer_and_repaints_all_cells() {intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;run(a.render(v,1,value,glyph,&mut p)).unwrap();let l2=RasterLayout::fit(v,(2,2),4,2*1024*1024).unwrap();let before=intel::state(|s|s.events.iter().filter(|e|e.starts_with("queue")).count());assert_eq!(run(a.resize(l2,v,&b,&mut p)),Ok(true));assert_eq!(before,intel::state(|s|s.events.iter().filter(|e|e.starts_with("queue")).count()));assert_eq!(b.used(),l2.bytes);let work=run(a.render(v,1,value,glyph,&mut p)).unwrap();assert_eq!(work.produced,2640);assert_eq!(work.painted,2640);drop(a);assert_eq!(b.used(),0);}

#[test]fn resize_rejects_foreign_budget_owner_before_admission() {
    intel::reset();let v=rect(0,0,80,22);let l=layout(v);
    let b=RasterBudget::new(2*1024*1024);let foreign=RasterBudget::new(2*1024*1024);
    let mut a=BcsTextArea::<u32>::new(l,&b).unwrap();let mut p=false;
    assert_eq!(run(a.resize(l,v,&foreign,&mut p)),Err("text-area-budget-owner"));
    assert_eq!(foreign.used(),0);assert_eq!(b.used(),l.bytes);assert!(!p);
    assert!(!intel::state(|s|s.events.iter().any(|e|e.starts_with("queue"))));
    drop(a);assert_eq!(b.used(),0);
}
#[test]fn cancelled_backing_cannot_be_reused_after_clearing_external_poison() {
    intel::reset();let v=rect(0,0,80,22);let l=layout(v);let b=RasterBudget::new(2*1024*1024);
    let mut a=BcsTextArea::new(l,&b).unwrap();let mut p=false;
    {let mut f=Box::pin(a.render(v,1,value,glyph,&mut p));
     assert!(matches!(f.as_mut().poll(&mut context()),Poll::Pending));drop(f);}
    p=false;
    assert_eq!(run(a.render(v,1,value,glyph,&mut p)).err(),Some("text-area-backing-pinned"));
    a.invalidate();assert_eq!(run(a.resize(l,v,&b,&mut p)),Err("text-area-backing-pinned"));
    drop(a);assert_eq!(b.used(),l.bytes);
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix='text-area-raster-') as directory:
        path = Path(directory)
        (path / 'tests.rs').write_text(SOURCE.replace('__TRUEOS_ROOT__', str(ROOT)))
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tests.rs'), '-o', str(path / 'tests')], check=True)
        subprocess.run([str(path / 'tests'), '--test-threads=1', '--nocapture'], check=True)

if __name__ == '__main__':
    main()
