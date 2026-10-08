#!/usr/bin/env python3
"""Host-test production winit entry routing and paired frame resize policy.

The allocator/window hardware boundary is stubbed; ABI validation, cadence
routing, buffering selection, and resize staging are extracted from the kernel.
"""
from pathlib import Path
import subprocess
import re
import tempfile

from test_clip_position3_uv_texture import item, ROOT

SOURCE = "src/ui4/blueprint_text.rs"
HARNESS = r'''
#![allow(dead_code)]
use std::{cell::RefCell, sync::{Mutex, atomic::Ordering}};
#[macro_export] macro_rules! log_warn { (target: $target:expr; $($arg:tt)*) => { let _ = format_args!($($arg)*); }; }
#[macro_export] macro_rules! log_important { (target: $target:expr; $($arg:tt)*) => { let _ = format_args!($($arg)*); }; }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum FrameCadence { Immutable, Dirty, Streaming }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum FrameBuffering { Single, Double, Triple }
impl FrameBuffering { fn count(self)->u32 { match self {Self::Single=>1,Self::Double=>2,Self::Triple=>3} } }
type WindowOwner=u32;
type WindowSessionId=u32;
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowId(u32);
impl WindowId { fn raw(self)->u32 {self.0} }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct FrameHandle(u32);
impl FrameHandle { fn raw(self)->u32 {self.0} }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowPlacement { x:i32,y:i32,width:u32,height:u32 }
#[derive(Clone,Copy)] struct BlueprintPendingResize {previous_frame:FrameHandle,previous_width:u32,previous_height:u32,previous_placement:WindowPlacement,placement:WindowPlacement,resize_epoch:u64}
#[derive(Clone)] struct BlueprintSceneSurface {
    owner:WindowOwner,window:WindowId,session:WindowSessionId,render_target:u32,
    frame:FrameHandle,width:u32,height:u32,cadence:FrameCadence,placement:WindowPlacement,winit_dormant_background:bool,
    write_lease:Option<()>,gpu_submission_unretired:bool,vgpu_surface:Option<()>,
    pending_resize:Option<BlueprintPendingResize>,pending_resize_ready:bool,
    taken_resize_event:Option<()>,retained_text_backbuffer:Option<()>,retained_text_backbuffer_extent:Option<(u32,u32)>,
}
#[derive(Clone,Copy,Debug)] struct OutputId;
impl OutputId { fn from_slot(_:u32)->Option<Self> {Some(Self)} }
#[derive(Clone,Copy,Debug)] enum FrameContent { BlueprintScene }
#[derive(Clone,Copy,Debug)] enum ScanoutFormat { Rgba8888Premultiplied }
#[derive(Clone,Copy,Debug)] struct PremultipliedRgba8;
impl PremultipliedRgba8 { const TRANSPARENT:Self=Self; }
#[derive(Clone,Copy,Debug)] struct FrameSpec {output:OutputId,content:FrameContent,cadence:FrameCadence,buffering:FrameBuffering,format:ScanoutFormat,width:u32,height:u32,base_color:Option<PremultipliedRgba8>}
const ERROR_STATE:i32=-1;
const ERROR_BUSY:i32=-2;
const ERROR_UI4:i32=-3;
const MAX_FRAME_WIDTH:u32=4096;
const MAX_FRAME_HEIGHT:u32=2160;
const UI4_VISUAL_SOFT_CAP_HZ:u32=240;
struct SurfaceStore(Mutex<Vec<BlueprintSceneSurface>>);
impl SurfaceStore {fn lock(&self)->std::sync::MutexGuard<'_,Vec<BlueprintSceneSurface>> {self.0.lock().unwrap()} }
static SURFACES:SurfaceStore=SurfaceStore(Mutex::new(Vec::new()));
thread_local! {
    static OPENS:RefCell<Vec<(FrameCadence,FrameBuffering,Option<u32>,bool)>>=RefCell::new(Vec::new());
    static SPECS:RefCell<Vec<FrameSpec>>=RefCell::new(Vec::new());
    static DESTROYED:RefCell<Vec<FrameHandle>>=RefCell::new(Vec::new());
    static RETIRED:RefCell<Vec<FrameHandle>>=RefCell::new(Vec::new());
    static FAIL_OPEN:RefCell<bool>=RefCell::new(false);
    static FAIL_ALLOC:RefCell<Option<usize>>=RefCell::new(None);
    static CLOSES:RefCell<Vec<u32>>=RefCell::new(Vec::new());
    static COMMITS:RefCell<Vec<(FrameHandle,FrameHandle)>>=RefCell::new(Vec::new());
    static PUBLISHED:RefCell<Vec<FrameHandle>>=RefCell::new(Vec::new());
    static CANCELED:RefCell<Vec<FrameHandle>>=RefCell::new(Vec::new());
    static FAIL_PUBLISH:RefCell<bool>=RefCell::new(false);
    static BACKGROUND_PUBLISHES:RefCell<Vec<WindowId>>=RefCell::new(Vec::new());
}
mod hv {pub fn current_hull_guest_context_vm_id()->Option<u32> {None} }
mod trueos_vm {pub mod vmcall {
    pub const OP_BP_UI4_WINIT_FRAME_OPEN_V1:u64=99;
    pub const STATUS_OK:u64=0;
    pub fn call_with_payload(_:u64,_:u64,_:u64,_:&[u8],_:&mut[u8])->(u64,u64) {panic!("host entry must not vmcall")}
} }
fn pack_i32_pair(a:i32,b:i32)->u64 {pack_u32_pair(a as u32,b as u32)}
fn pack_u32_pair(a:u32,b:u32)->u64 {a as u64 | ((b as u64)<<32)}
fn blueprint_owner()->Option<u32> {Some(1)}
fn surface_mut(s:&mut[BlueprintSceneSurface],owner:u32,id:u32)->Option<&mut BlueprintSceneSurface> {s.iter_mut().find(|s|s.owner==owner&&s.render_target==id)}
fn trueos_cabi_ui4_solara_frame_close(id:u32)->i32 {CLOSES.with(|s|s.borrow_mut().push(id));SURFACES.lock().clear();0}
fn window_resize_state(_:u32,_:WindowId)->Result<(WindowPlacement,u64),()> {Ok((WindowPlacement{x:7,y:8,width:100,height:100},9))}
fn create_frame(spec:FrameSpec)->Result<FrameHandle,()> {SPECS.with(|s| {let mut s=s.borrow_mut();s.push(spec); if FAIL_ALLOC.with(|f|*f.borrow())==Some(s.len()) {Err(())}else{Ok(FrameHandle(20+s.len() as u32))}})}
#[derive(Clone,Copy,Debug)] enum FramePoolError { Busy }
#[derive(Clone,Copy,Debug)] struct FrameLease {frame:FrameHandle}
fn acquire_frame_buffer(frame:FrameHandle)->Result<FrameLease,FramePoolError> {Ok(FrameLease{frame})}
fn publish_frame_buffer(lease:FrameLease)->Result<(),FramePoolError> {if FAIL_PUBLISH.with(|s|*s.borrow()) {Err(FramePoolError::Busy)}else{PUBLISHED.with(|s|s.borrow_mut().push(lease.frame));Ok(())}}
fn cancel_frame_buffer(lease:FrameLease)->Result<(),FramePoolError> {CANCELED.with(|s|s.borrow_mut().push(lease.frame));Ok(())}
#[derive(Clone,Copy)] struct DamageRect;
impl DamageRect { const FULL:Self=Self; }
mod layer_contract {pub fn background_target(window:u32)->u32 {window|0x8000} }
fn destroy_frame(frame:FrameHandle)->Result<(),()> {DESTROYED.with(|s|s.borrow_mut().push(frame));Ok(())}
fn retire_frame_when_released(frame:FrameHandle) {RETIRED.with(|s|s.borrow_mut().push(frame));}
fn surface(cadence:FrameCadence,background:bool)->BlueprintSceneSurface {
    BlueprintSceneSurface {owner:1,window:WindowId(1),session:2,render_target:if background {0x8001}else{1},frame:FrameHandle(if background {11}else{10}),width:100,height:100,cadence,
    placement:WindowPlacement{x:5,y:6,width:100,height:100},winit_dormant_background:false,write_lease:None,gpu_submission_unretired:false,vgpu_surface:None,pending_resize:None,pending_resize_ready:false,taken_resize_event:Some(()),retained_text_backbuffer:Some(()),retained_text_backbuffer_extent:Some((100,100))}
}
mod producer {
    use super::*;
    ITEMS
    // Capture the hardware-bound surface allocation request. The production
    // buffering selector runs here, so the ABI and resize must agree on rings.
    fn open_blueprint_surface_with_cadence(_:i32,_:i32,_:u32,_:u32,cadence:FrameCadence,hz:Option<u32>,parent:Option<(WindowId,WindowSessionId)>)->u32 {
        OPENS.with(|o|o.borrow_mut().push((cadence,blueprint_frame_buffering(cadence),hz,parent.is_some())));
        if parent.is_some() && FAIL_OPEN.with(|f|*f.borrow()) {return 0;}
        SURFACES.lock().push(surface(cadence,parent.is_some()));
        1
    }
    fn open_blueprint_frame(x:i32,y:i32,w:u32,h:u32,c:FrameCadence,hz:Option<u32>)->u32 {open_blueprint_surface(x,y,w,h,c,hz,None)}
    pub(super) fn open(w:u32,h:u32,hz:u32)->u32 {trueos_cabi_ui4_winit_frame_open_v1(1,2,w,h,hz)}
    pub(super) fn legacy() {open_blueprint_surface(0,0,100,100,FrameCadence::Streaming,None,None);open_blueprint_surface(0,0,100,100,FrameCadence::Streaming,Some(60),Some((WindowId(1),2)));}
    pub(super) fn resize(s:&mut[BlueprintSceneSurface])->i32 {stage_layered_resize(s,1,WindowId(1),200,150)}
    pub(super) fn commit(s:&mut[BlueprintSceneSurface])->i32 {commit_layered_resize_if_ready(s,1,WindowId(1))}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum WindowBrokerError {StaleResize,InvalidHandle}
mod window_broker {
    use super::*;
    pub(super) fn publish_window_background(_:WindowOwner,window:WindowId,_:DamageRect)->Result<(),WindowBrokerError> {BACKGROUND_PUBLISHES.with(|s|s.borrow_mut().push(window));Ok(())}
    pub(super) fn commit_window_layered_replacement(_:WindowOwner,_:WindowId,foreground:FrameHandle,background:FrameHandle,_:WindowPlacement,_:u64)->Result<(),WindowBrokerError> {COMMITS.with(|s|s.borrow_mut().push((foreground,background)));Ok(())}
}
fn reset() {PUBLISHED.with(|s|s.borrow_mut().clear());CANCELED.with(|s|s.borrow_mut().clear());FAIL_PUBLISH.with(|s|*s.borrow_mut()=false);BACKGROUND_PUBLISHES.with(|s|s.borrow_mut().clear());COMMITS.with(|s|s.borrow_mut().clear());SURFACES.lock().clear();OPENS.with(|s|s.borrow_mut().clear());SPECS.with(|s|s.borrow_mut().clear());DESTROYED.with(|s|s.borrow_mut().clear());RETIRED.with(|s|s.borrow_mut().clear());CLOSES.with(|s|s.borrow_mut().clear());FAIL_OPEN.with(|s|*s.borrow_mut()=false);FAIL_ALLOC.with(|s|*s.borrow_mut()=None);}
#[test] fn winit_entry_routes_dirty_double_ui_and_streaming_triple_scene() {
    reset();assert_eq!(producer::open(100,100,60),1);
    OPENS.with(|s|assert_eq!(*s.borrow(),vec![(FrameCadence::Dirty,FrameBuffering::Double,None,false),(FrameCadence::Streaming,FrameBuffering::Triple,Some(60),true)]));
    assert!(SURFACES.lock()[1].winit_dormant_background);
    PUBLISHED.with(|s|assert_eq!(*s.borrow(),vec![FrameHandle(11)]));
    BACKGROUND_PUBLISHES.with(|s|assert_eq!(*s.borrow(),vec![WindowId(1)]));
}
#[test] fn invalid_winit_extents_or_cadence_allocate_nothing() {
    reset();for (w,h,hz) in [(0,100,60),(100,0,60),(4097,100,60),(100,2161,60),(100,100,0),(100,100,241)] {assert_eq!(producer::open(w,h,hz),0);}
    OPENS.with(|s|assert!(s.borrow().is_empty()));
}
#[test] fn failed_background_open_closes_the_new_foreground_window() {
    reset();FAIL_OPEN.with(|s|*s.borrow_mut()=true);assert_eq!(producer::open(100,100,60),0);
    CLOSES.with(|s|assert_eq!(*s.borrow(),vec![1]));assert!(SURFACES.lock().is_empty());
}
#[test] fn resize_preserves_winit_and_legacy_cadence_and_ring_sizes() {
    for legacy in [false,true] {
        reset();if legacy {producer::legacy()}else{assert_eq!(producer::open(100,100,60),1);}
        let mut pair=SURFACES.lock().clone();assert_eq!(producer::resize(&mut pair),0);
        SPECS.with(|s| {let s=s.borrow();let expected=if legacy {[(FrameCadence::Streaming,FrameBuffering::Triple),(FrameCadence::Dirty,FrameBuffering::Double)]}else{[(FrameCadence::Dirty,FrameBuffering::Double),(FrameCadence::Streaming,FrameBuffering::Triple)]};
            assert_eq!(s.iter().map(|s|(s.cadence,s.buffering)).collect::<Vec<_>>(),expected);assert!(s.iter().all(|s|s.width==200&&s.height==150));});
        for (i,s) in pair.iter().enumerate() {let pending=s.pending_resize.unwrap();assert_eq!(pending.previous_frame,FrameHandle(10+i as u32));assert_eq!(pending.resize_epoch,9);assert_eq!((s.width,s.height),(200,150));assert_eq!(s.pending_resize_ready,s.winit_dormant_background);assert!(s.taken_resize_event.is_none()&&s.retained_text_backbuffer.is_none());}
    }
}
#[test] fn dormant_background_resize_commits_after_only_ui_publication() {
    reset();assert_eq!(producer::open(100,100,60),1);let mut pair=SURFACES.lock().clone();
    assert_eq!(producer::resize(&mut pair),0);assert!(!pair[0].pending_resize_ready);assert!(pair[1].pending_resize_ready);
    PUBLISHED.with(|s|assert_eq!(*s.borrow(),vec![FrameHandle(11),FrameHandle(22)]));
    assert_eq!(producer::commit(&mut pair),0);COMMITS.with(|s|assert!(s.borrow().is_empty()));
    pair[0].pending_resize_ready=true;assert_eq!(producer::commit(&mut pair),0);
    COMMITS.with(|s|assert_eq!(*s.borrow(),vec![(FrameHandle(21),FrameHandle(22))]));
    assert!(pair.iter().all(|s|s.pending_resize.is_none()));
}
#[test] fn authored_background_resize_requires_both_producers_to_publish() {
    reset();let mut pair=vec![surface(FrameCadence::Dirty,false),surface(FrameCadence::Streaming,true)];
    assert_eq!(producer::resize(&mut pair),0);assert!(pair.iter().all(|s|!s.pending_resize_ready));
    PUBLISHED.with(|s|assert!(s.borrow().is_empty()));pair[0].pending_resize_ready=true;
    assert_eq!(producer::commit(&mut pair),0);COMMITS.with(|s|assert!(s.borrow().is_empty()));
    pair[1].pending_resize_ready=true;assert_eq!(producer::commit(&mut pair),0);
    COMMITS.with(|s|assert_eq!(s.borrow().len(),1));
}
#[test] fn failed_initial_background_publication_cancels_lease_and_closes_window() {
    reset();FAIL_PUBLISH.with(|s|*s.borrow_mut()=true);assert_eq!(producer::open(100,100,60),0);
    CANCELED.with(|s|assert_eq!(*s.borrow(),vec![FrameHandle(11)]));CLOSES.with(|s|assert_eq!(*s.borrow(),vec![1]));
    assert!(SURFACES.lock().is_empty());
}
#[test] fn failed_dormant_resize_publication_destroys_pair_and_preserves_old_frames() {
    reset();assert_eq!(producer::open(100,100,60),1);let mut pair=SURFACES.lock().clone();FAIL_PUBLISH.with(|s|*s.borrow_mut()=true);
    assert_eq!(producer::resize(&mut pair),ERROR_UI4);assert_eq!((pair[0].frame,pair[1].frame),(FrameHandle(10),FrameHandle(11)));assert!(pair.iter().all(|s|s.pending_resize.is_none()));
    CANCELED.with(|s|assert_eq!(*s.borrow(),vec![FrameHandle(22)]));DESTROYED.with(|s|assert_eq!(*s.borrow(),vec![FrameHandle(21),FrameHandle(22)]));
}
#[test] fn failed_second_resize_allocation_keeps_both_committed_frames() {
    reset();let mut pair=vec![surface(FrameCadence::Dirty,false),surface(FrameCadence::Streaming,true)];FAIL_ALLOC.with(|s|*s.borrow_mut()=Some(2));
    assert_eq!(producer::resize(&mut pair),ERROR_UI4);assert_eq!((pair[0].frame,pair[1].frame),(FrameHandle(10),FrameHandle(11)));assert!(pair.iter().all(|s|s.pending_resize.is_none()&&s.width==100));
    DESTROYED.with(|s|assert_eq!(*s.borrow(),vec![FrameHandle(21)]));
}
#[test] fn restaging_resize_retires_only_superseded_replacements() {
    reset();let mut pair=vec![surface(FrameCadence::Dirty,false),surface(FrameCadence::Streaming,true)];
    assert_eq!(producer::resize(&mut pair),0);pair[0].pending_resize_ready=true;
    assert_eq!(producer::resize(&mut pair),0);
    RETIRED.with(|s|assert_eq!(*s.borrow(),vec![FrameHandle(21),FrameHandle(22)]));
    for (i,s) in pair.iter().enumerate() {assert_eq!(s.frame,FrameHandle(23+i as u32));assert_eq!(s.pending_resize.unwrap().previous_frame,FrameHandle(10+i as u32));assert!(!s.pending_resize_ready);}
    SPECS.with(|s|assert_eq!(s.borrow().iter().map(|s|s.buffering).collect::<Vec<_>>(),vec![FrameBuffering::Double,FrameBuffering::Triple,FrameBuffering::Double,FrameBuffering::Triple]));
}
#[test] fn outstanding_producer_work_prevents_pair_allocation() {
    for kind in 0..3 {reset();let mut pair=vec![surface(FrameCadence::Dirty,false),surface(FrameCadence::Streaming,true)];match kind {0=>pair[0].write_lease=Some(()),1=>pair[1].gpu_submission_unretired=true,_=>pair[1].vgpu_surface=Some(())};assert_eq!(producer::resize(&mut pair),ERROR_BUSY);SPECS.with(|s|assert!(s.borrow().is_empty()));}
}
'''


def abi_item(name):
    source = (ROOT / SOURCE).read_text()
    declaration = re.search(rf'^pub extern "C" fn {re.escape(name)}\b', source, re.MULTILINE)
    if declaration is None:
        raise ValueError(f"missing ABI entry {name}")
    ending = re.search(r"^}\s*\n", source[declaration.end():], re.MULTILINE)
    if ending is None:
        raise ValueError(f"unterminated ABI entry {name}")
    return source[declaration.start():declaration.end() + ending.end()]


def main():
    functions = abi_item("trueos_cabi_ui4_winit_frame_open_v1") + "\n" + "\n".join(item(SOURCE, name) for name in (
        "open_blueprint_surface",
        "blueprint_frame_buffering", "publish_transparent_winit_background", "stage_layered_resize", "revert_blueprint_pending_resize", "commit_layered_resize_if_ready"))
    with tempfile.TemporaryDirectory(prefix="ui4-winit-frames-") as directory:
        source = Path(directory) / "tests.rs"
        source.write_text(HARNESS.replace("ITEMS", functions))
        binary = Path(directory) / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
