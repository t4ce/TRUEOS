#!/usr/bin/env python3
"""Exercise production collapse projection, lifecycle, and alpha on the host."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
item = extract.item
constant = extract.constant
upload_source = (ROOT / 'src/ui4/blueprint_ui4.rs').read_text()
collapse_api = (ROOT / 'src/ui4/blueprint_ui4/collapse_api.rs').read_text()
collapse = (ROOT / 'src/ui4/collapse.rs').read_text()
implementation = collapse[collapse.index('impl Collapse {'):collapse.index('/// Per-cursor click')]
harness = r'''
#![allow(dead_code)]
use std::sync::{Mutex as StdMutex,MutexGuard};
struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> { const fn new(v:T)->Self { Self(StdMutex::new(v)) } fn lock(&self)->MutexGuard<'_,T> { self.0.lock().unwrap() } }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowOwner(u8);
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowId(u32);
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowSessionId(u32);
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct FrameHandle(u64);
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum FrameBuffering { Double, Triple }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowPlacement { x:i32,y:i32,width:u32,height:u32,z:i32,opacity:u8,visible:bool }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct Interaction { movable:bool,primary_activation:bool,maximizable:bool,resize_on_maximize:bool }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct WindowSnapshot {
    frame:FrameHandle,arc:u16,background:Option<FrameHandle>,buffering:FrameBuffering,
    placement:WindowPlacement,presentation_placement:WindowPlacement,maximized:bool,dock_target:Option<u8>,interaction:Interaction,
}
mod collapse {
    use super::*;
'''
harness += item('src/ui4/collapse.rs', 'Collapse') + implementation
harness += item('src/ui4/collapse.rs', 'RestorePress')
harness += collapse[collapse.index('impl RestorePress {'):collapse.index('/// Copies a pitched')]
harness += constant('src/ui4/collapse.rs', 'COLLAPSED_SIZE')
harness += item('src/ui4/collapse.rs', 'collapse_source_pixel')
harness += item('src/ui4/collapse.rs', 'premultiply')
harness += r'''
    pub fn size()->u32 { COLLAPSED_SIZE }
    pub fn sample(x:usize,y:usize,w:u32,h:u32)->(usize,usize) { collapse_source_pixel(x,y,w,h) }
    pub fn pixel(input:[u8;4])->[u8;4] { let mut out=[0;4];premultiply(&input,&mut out);out }
}
#[derive(Debug)] enum WindowBrokerError { InvalidHandle,InteractionDenied,EmptyExtent }
struct DamageRegion;
impl DamageRegion { const FULL:Self=Self; }
impl WindowPlacement { fn valid(self)->bool { self.width>0 && self.height>0 } }
mod ui4 { pub fn output_dimensions()->(u32,u32) { (1920,1080) } }
fn translated_resize_presentation(p:WindowPlacement,_:i64,_:i64,_:(u32,u32))->WindowPlacement { p }
struct Window { interaction:Interaction,replacement_presentation:Option<WindowPlacement>,id:WindowId,owner:WindowOwner,session:WindowSessionId,collapse:Option<collapse::Collapse>,placement:WindowPlacement,revision:u64,damage:Option<DamageRegion> }
struct Broker { windows:Vec<Window>,revision:u64 }
impl Broker {
    fn checked_window_mut(&mut self,owner:WindowOwner,id:WindowId)->Result<&mut Window,WindowBrokerError> {
        self.windows.iter_mut().find(|w| w.id==id && w.owner==owner).ok_or(WindowBrokerError::InvalidHandle)
    }
    fn mark_composition_changed(&mut self) { self.revision+=1; }
}
static WINDOW_BROKER:Mutex<Broker> = Mutex::new(Broker { windows:Vec::new(),revision:0 });
static RETIRED:Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());
fn retire_frame_when_released(f:FrameHandle) { RETIRED.lock().push(f); }
fn retire_transferred_frames(fs:Vec<FrameHandle>) { RETIRED.lock().extend(fs); }
fn next_serial(v:u64)->u64 { v+1 }
mod cursor_frame_inout { pub fn selection_strip_stack_changed() {} }
mod broker {
    use super::*;
'''
for name in ['install_collapse', 'collapse_state', 'toggle_collapse', 'release_collapse', 'move_window']:
    harness += item('src/ui4/window_broker.rs', name)
harness += r'''
    pub fn release(owner:WindowOwner, session:Option<WindowSessionId>) { release_collapse(owner,session); }
}
fn placement()->WindowPlacement { WindowPlacement {x:100,y:150,width:800,height:600,z:2,opacity:199,visible:true} }
fn snapshot()->WindowSnapshot { WindowSnapshot { frame:FrameHandle(1),arc:8,background:Some(FrameHandle(2)),buffering:FrameBuffering::Triple,placement:placement(),presentation_placement:placement(),maximized:true,dock_target:Some(7),interaction:Interaction {movable:true,primary_activation:false,maximizable:true,resize_on_maximize:true} } }
fn icon()->collapse::Collapse { collapse::Collapse {frame:FrameHandle(10),width:256,height:128,tile:None} }
#[test] fn exact_non_square_icon_replaces_both_layers_and_disables_scalers() {
    let mut icon=icon(); icon.toggle(placement());
    let projected=icon.project(snapshot());
    assert_eq!(projected.frame,FrameHandle(10));
    assert_eq!((projected.placement.width,projected.placement.height),(256,128));
    assert_eq!(projected.placement.y,622);
    assert_eq!(projected.placement,projected.presentation_placement);
    assert_eq!(projected.placement.opacity,199);
    assert_eq!(projected.background,None);
    assert_eq!(projected.arc,0);
    assert!(!projected.maximized && !projected.interaction.maximizable && !projected.interaction.resize_on_maximize);
    assert!(!projected.interaction.primary_activation);
}
#[test] fn restore_uses_latest_producer_frame_layers_and_geometry() {
    let mut icon=icon(); icon.toggle(placement());
    let mut latest=snapshot(); latest.frame=FrameHandle(55);latest.placement.width=900;
    assert_eq!(icon.project(latest).frame,FrameHandle(10));
    icon.toggle(latest.placement);
    assert_eq!(icon.project(latest),latest);
}
#[test] fn restore_click_requires_same_window_and_never_revives_a_drag() {
    let mut press=collapse::RestorePress::new(20,30);
    press.update(24,34,false);assert!(press.clicked(true));assert!(!press.clicked(false));
    press.update(25,34,false);press.update(20,30,false);assert!(!press.clicked(true));
    let mut press=collapse::RestorePress::new(20,30);press.update(20,30,true);assert!(!press.clicked(true));
}
#[test] fn default_tile_size_and_resampling_stay_inside_source() {
    assert_eq!(collapse::size(),64);
    for (w,h) in [(1,1),(256,256),(64,128),(17,7)] {
        for y in 0..64 { for x in 0..64 {
            let (sx,sy)=collapse::sample(x,y,w,h);
            assert!(sx < w as usize && sy < h as usize);
        }}
    }
    assert_eq!(collapse::sample(0,0,256,256),(2,2));
    assert_eq!(collapse::sample(63,63,256,256),(254,254));
}
#[test] fn transparent_pixel_ownership_and_alpha_rounding() {
    assert_eq!(collapse::pixel([255,128,42,0]),[0,0,0,0]);
    assert_eq!(collapse::pixel([255,128,42,128]),[128,64,21,128]);
    assert_eq!(collapse::pixel([255,128,42,255]),[255,128,42,255]);
}
#[test] fn small_window_and_large_coordinates_do_not_underflow() {
    let mut p=placement();p.height=16;p.y=i32::MAX;
    let mut icon=icon();icon.toggle(p);
    assert_eq!(icon.tile.unwrap().y,i32::MAX);
}
#[test] fn owner_checks_update_while_compact_and_session_teardown() {
    let owner=WindowOwner(1);let id=WindowId(8);
    WINDOW_BROKER.lock().windows=vec![Window {interaction:snapshot().interaction,replacement_presentation:None,id,owner,session:WindowSessionId(3),collapse:None,placement:placement(),revision:1,damage:None}];
    assert!(broker::install_collapse(WindowOwner(2),id,icon()).is_err());
    broker::install_collapse(owner,id,icon()).unwrap();
    broker::toggle_collapse(owner,id).unwrap();
    assert_eq!(broker::collapse_state(owner,id),Some(true));
    let original=placement();
    let mut moved=WINDOW_BROKER.lock().windows[0].collapse.unwrap().tile.unwrap();
    moved.x+=40;moved.y-=20;
    broker::move_window(owner,id,moved).unwrap();
    assert_eq!(WINDOW_BROKER.lock().windows[0].collapse.unwrap().tile.unwrap(),moved);
    assert_eq!(WINDOW_BROKER.lock().windows[0].placement,original);
    let mut invalid=moved;invalid.width+=1;
    assert!(broker::move_window(owner,id,invalid).is_err());
    assert!(broker::move_window(WindowOwner(2),id,moved).is_err());
    let mut replacement=icon();replacement.frame=FrameHandle(11);replacement.width=64;
    broker::install_collapse(owner,id,replacement).unwrap();
    assert_eq!(*RETIRED.lock(),vec![FrameHandle(10)]);
    let b=WINDOW_BROKER.lock();let tile=b.windows[0].collapse.unwrap().tile.unwrap();
    assert_eq!((tile.width,tile.height),(64,128));assert_eq!(b.windows[0].placement,placement());drop(b);
    broker::release(owner,Some(WindowSessionId(4)));
    assert_eq!(broker::collapse_state(owner,id),Some(true));
    broker::release(owner,Some(WindowSessionId(3)));
    assert_eq!(broker::collapse_state(owner,id),None);
    assert_eq!(*RETIRED.lock(),vec![FrameHandle(10),FrameHandle(11)]);
}
'''
harness += re.search(r'^const TEXT_BACKBUFFER_SPRITE_ID:.*;$', upload_source, re.MULTILINE).group(0) + '\n'
harness += re.search(r'^const UPLOAD_ID:.*;$', collapse_api, re.MULTILINE).group(0) + '\n'
harness += item('src/ui4/blueprint_ui4.rs', 'sprite_rgba8_upload_id_valid')
harness += r'''
#[test] fn collapse_upload_is_admitted_without_opening_reserved_sprite_namespaces() {
    assert!(sprite_rgba8_upload_id_valid(UPLOAD_ID));
    assert!(sprite_rgba8_upload_id_valid(1));
    assert!(!sprite_rgba8_upload_id_valid(0));
    assert!(!sprite_rgba8_upload_id_valid(TEXT_BACKBUFFER_SPRITE_ID));
    assert!(!sprite_rgba8_upload_id_valid(0x8000_0001));
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-collapse-') as directory:
    directory = Path(directory)
    (directory / 'test.rs').write_text(harness)
    subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'), '-o', str(directory / 'test')], check=True)
    subprocess.run([str(directory / 'test'), '--test-threads=1'], check=True)

# Compile the actual public SDK method against a recording C ABI.
sdk_source = (ROOT.parent / 'TRUEOS-Blueprints/api/src/ui4_scene.rs').read_text()
sdk_method = sdk_source[sdk_source.index('impl Frame {'):]
sdk_harness = r'''
use std::sync::Mutex;
static CALLS:Mutex<Vec<(u32,u32,u32,Vec<u8>)>>=Mutex::new(Vec::new());
static STATUS:Mutex<i32>=Mutex::new(0);
mod v { pub mod bp_abi {
    pub unsafe fn trueos_cabi_ui4_scene_enable_collapse_v1(window:u32,w:u32,h:u32,p:*const u8,n:usize)->i32 {
        crate::CALLS.lock().unwrap().push((window,w,h,unsafe { std::slice::from_raw_parts(p,n) }.to_vec()));
        *crate::STATUS.lock().unwrap()
    }
} }
mod ui4_solara_text {
    pub fn status(rc:i32)->Result<(),crate::sdk::Error> { if rc==0 {Ok(())} else {Err(crate::sdk::Error::Busy)} }
}
mod sdk {
    use crate::v;
    pub const MAX_COLLAPSE_IMAGE_SIZE:u32=256;
    #[derive(Debug,PartialEq,Eq)] pub enum Error { Invalid,Busy }
    pub struct Frame;
    impl Frame { pub fn window_id(&self)->u32 { 42 } }
'''
sdk_harness += sdk_method + r'''
}
#[test] fn one_call_copies_full_256_image_without_scaling_or_alpha_changes() {
    let pixels=vec![127;256*256*4];
    sdk::Frame.enable_collapse(256,256,&pixels).unwrap();
    assert_eq!(crate::CALLS.lock().unwrap().pop().unwrap(),(42,256,256,pixels));
}
#[test] fn non_square_images_are_exact_and_invalid_inputs_never_cross_abi() {
    let mut frame=sdk::Frame;
    for (w,h,n) in [(0,1,0),(257,1,1028),(1,257,1028),(2,2,15),(2,2,17)] {
        assert_eq!(frame.enable_collapse(w,h,&vec![0;n]),Err(sdk::Error::Invalid));
    }
    assert!(crate::CALLS.lock().unwrap().is_empty());
    let image=vec![255;64*128*4];frame.enable_collapse(64,128,&image).unwrap();
    assert_eq!(crate::CALLS.lock().unwrap().pop().unwrap(),(42,64,128,image));
}
#[test] fn kernel_failure_is_returned_to_caller() {
    *crate::STATUS.lock().unwrap()=-7;
    assert_eq!(sdk::Frame.enable_collapse(1,1,&[255;4]),Err(sdk::Error::Busy));
    crate::CALLS.lock().unwrap().clear();*crate::STATUS.lock().unwrap()=0;
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-collapse-sdk-') as directory:
    directory = Path(directory)
    (directory / 'test.rs').write_text(sdk_harness)
    subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'), '-o', str(directory / 'test')], check=True)
    subprocess.run([str(directory / 'test'), '--test-threads=1'], check=True)
