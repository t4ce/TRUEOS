#!/usr/bin/env python3
"""Exercise the production Blueprint bottom-color ABI with a mocked register writer."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item, constant

ROOT = Path(__file__).resolve().parents[1]
SOURCE = 'src/ui4/blueprint_text.rs'
declarations = '\n'.join(constant(SOURCE, name) for name in
    ('ERROR_INVALID', 'ERROR_CONTEXT', 'ERROR_NOT_FOUND', 'ERROR_UI4'))
declarations += '\n' + item(SOURCE, 'surface_mut')
production = (ROOT / SOURCE).read_text()
start = production.index('pub extern "C" fn trueos_cabi_ui4_scene_set_display_bottom_color(')
end = production.index('{', start) + 1
depth = 1
while depth:
    if production[end] == '{': depth += 1
    elif production[end] == '}': depth -= 1
    end += 1
declarations += '\n' + production[start:end]
opcode = constant('crates/trueos-vm/src/vmcall.rs', 'OP_BP_UI4_SCENE_SET_DISPLAY_BOTTOM_COLOR')
host_opcode = constant('src/hv/vmcall.rs', 'OP_BP_UI4_SCENE_SET_DISPLAY_BOTTOM_COLOR')
host_source = (ROOT / 'src/hv/vmcall.rs').read_text()
start = host_source.index('OP_BP_UI4_SCENE_SET_DISPLAY_BOTTOM_COLOR => {')
end = host_source.index('{', start) + 1
depth = 1
while depth:
    if host_source[end] == '{': depth += 1
    elif host_source[end] == '}': depth -= 1
    end += 1
host_arm = host_source[start:end]
source = r'''
#![allow(dead_code)]
#![deny(unreachable_patterns)]
extern crate self as trueos_vm;
use std::cell::RefCell;
type WindowOwner = u32;
struct BlueprintSceneSurface { owner:u32, render_target:u32 }
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
static SURFACES:Mutex<Vec<BlueprintSceneSurface>> = Mutex::new(Vec::new());
#[derive(Default)]
struct Trace {owner:Option<u32>,guest:bool,programmed:bool,writes:Vec<[u8;3]>,forwarded:Vec<(u32,u64,u64)>,responses:Vec<u64>}
thread_local! {static TRACE:RefCell<Trace> = RefCell::new(Trace::default());}
mod hv {pub fn current_hull_guest_context_vm_id()->Option<u32> {super::TRACE.with_borrow(|t|t.guest.then_some(1))}}
mod intel {
    pub fn set_pipe_a_bottom_color_rgb8(r:u8,g:u8,b:u8)->bool {
        super::TRACE.with_borrow_mut(|t| {t.writes.push([r,g,b]);t.programmed})
    }
}
fn blueprint_owner()->Option<u32> {TRACE.with_borrow(|t|t.owner)}
#[derive(Debug,PartialEq)] enum DispatchOutcome {Resume,Unhandled}
const STATUS_OK:u32 = 0;
fn write_response(_:u8,_:u32,_:u32,data:u64,_:u64) {TRACE.with_borrow_mut(|t|t.responses.push(data));}
mod ui4 {pub mod blueprint_text {pub use crate::trueos_cabi_ui4_scene_set_display_bottom_color;}}
fn guest_status(op:u32,a:u64,b:u64,payload:&[u8])->i32 {
    assert!(payload.is_empty());TRACE.with_borrow_mut(|t|t.forwarded.push((op,a,b)));-77
}
fn reset() {
    TRACE.with_borrow_mut(|t|*t=Trace{owner:Some(1),programmed:true,..Default::default()});
    *SURFACES.lock()=vec![BlueprintSceneSurface{owner:1,render_target:7},BlueprintSceneSurface{owner:2,render_target:8}];
}
#[test]
fn only_live_owned_frames_can_change_the_shared_register() {
    reset();
    assert_eq!(trueos_cabi_ui4_scene_set_display_bottom_color(8,0x123456),ERROR_NOT_FOUND);
    assert_eq!(trueos_cabi_ui4_scene_set_display_bottom_color(9,0x123456),ERROR_NOT_FOUND);
    TRACE.with_borrow_mut(|t|t.owner=None);
    assert_eq!(trueos_cabi_ui4_scene_set_display_bottom_color(7,0x123456),ERROR_CONTEXT);
    TRACE.with_borrow_mut(|t|t.owner=Some(1));
    SURFACES.lock().clear();
    assert_eq!(trueos_cabi_ui4_scene_set_display_bottom_color(7,0x123456),ERROR_NOT_FOUND);
    TRACE.with_borrow(|t|assert!(t.writes.is_empty()));
}
#[test]
fn valid_rgb_is_written_once_and_hardware_failure_is_reported() {
    reset();
    for (rgb,bytes) in [(0,[0,0,0]),(0xffffff,[255,255,255]),(0x63c7f2,[99,199,242]),(0xd83cff,[216,60,255])] {
        assert_eq!(trueos_cabi_ui4_scene_set_display_bottom_color(7,rgb),0);
        TRACE.with_borrow_mut(|t| {assert_eq!(t.writes,[bytes]);t.writes.clear();});
    }
    TRACE.with_borrow_mut(|t|t.programmed=false);
    assert_eq!(trueos_cabi_ui4_scene_set_display_bottom_color(7,0),ERROR_UI4);
}
#[test]
fn guest_forwards_exact_color_and_rejects_alpha_before_mediation() {
    reset();TRACE.with_borrow_mut(|t|t.guest=true);
    assert_eq!(trueos_cabi_ui4_scene_set_display_bottom_color(7,0xff123456),ERROR_INVALID);
    assert_eq!(trueos_cabi_ui4_scene_set_display_bottom_color(7,0x123456),-77);
    TRACE.with_borrow(|t| {
        assert!(t.writes.is_empty());
        assert_eq!(t.forwarded,[(vmcall::OP_BP_UI4_SCENE_SET_DISPLAY_BOTTOM_COLOR,7,0x123456)]);
    });
}
#[test]
fn actual_host_arm_handles_only_its_guest_opcode_and_rejects_wide_arguments() {
    reset();
    for op in 0..=0x300 {
        if op == vmcall::OP_BP_UI4_SCENE_SET_DISPLAY_BOTTOM_COLOR {continue;}
        assert_eq!(host::dispatch(op,7,0x123456),DispatchOutcome::Unhandled,"opcode {op:#x} intercepted");
    }
    TRACE.with_borrow(|t|assert!(t.writes.is_empty() && t.responses.is_empty()));
    assert_eq!(host::dispatch(vmcall::OP_BP_UI4_SCENE_SET_DISPLAY_BOTTOM_COLOR,7,0x123456),DispatchOutcome::Resume);
    TRACE.with_borrow_mut(|t|{assert_eq!(t.writes,[[0x12,0x34,0x56]]);assert_eq!(t.responses,[0]);t.writes.clear();t.responses.clear();});
    for (window,rgb) in [(1u64<<32|7,0x123456),(7,1u64<<32|0x123456),(7,0xff123456)] {
        assert_eq!(host::dispatch(vmcall::OP_BP_UI4_SCENE_SET_DISPLAY_BOTTOM_COLOR,window,rgb),DispatchOutcome::Resume);
    }
    TRACE.with_borrow(|t|{assert!(t.writes.is_empty());assert_eq!(t.responses,[(ERROR_INVALID as i64) as u64;3]);});
}
'''
source += '\nmod vmcall {\n' + opcode + '\n}\n' + declarations
source += '\nmod host { use super::*;\n' + host_opcode + '''
pub(super) fn dispatch(op:u32,arg0:u64,arg1:u64)->DispatchOutcome {
    let vm_id = 0; let seq = 1;
    match op {
''' + host_arm + ',\n_ => DispatchOutcome::Unhandled\n}}}\n'
with tempfile.TemporaryDirectory(prefix='trueos-bottom-color-') as directory:
    root = Path(directory)
    (root/'tests.rs').write_text(source)
    subprocess.run(['rustc','--edition=2024','--test',str(root/'tests.rs'),'-o',str(root/'tests')],check=True)
    subprocess.run([str(root/'tests'),'--test-threads=1'],check=True)
