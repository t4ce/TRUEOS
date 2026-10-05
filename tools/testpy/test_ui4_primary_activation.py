#!/usr/bin/env python3
"""Verify UI4 selection-click and exact physical-presentation receipt policy."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
item, constant = extract.item, extract.constant

source = 'src/ui4/input_broker.rs'
harness = ''.join(constant(source, name) for name in ('PRIMARY_BUTTON_MASK', 'SECONDARY_BUTTON_MASK'))
harness += item(source, 'absorb_selection_gesture')
harness += item(source, 'primary_activation_tests')
with tempfile.TemporaryDirectory(prefix='trueos-primary-activation-') as directory:
    root = Path(directory)
    (root / 'test.rs').write_text(harness)
    subprocess.run(['rustc', '--edition=2024', '--test', str(root / 'test.rs'), '-o', str(root / 'test')], check=True)
    subprocess.run([str(root / 'test')], check=True)


def receipt_item(path, name):
    source = (ROOT / path).read_text()
    match = re.search(
        rf'^(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?(?:extern "C"\s+)?fn {name}\b',
        source, re.M,
    )
    assert match, (path, name)
    end = re.search(r'^}', source[match.end():], re.M)
    assert end, (path, name)
    return source[match.start():match.end() + end.end()]


def test_publication_receipts():
    harness = r'''
#![allow(dead_code)]
use std::sync::{Mutex, MutexGuard, atomic::{AtomicBool, AtomicU64, AtomicI32, Ordering}};
struct Lock<T>(Mutex<T>);
impl<T> Lock<T> {
    const fn new(value:T)->Self { Self(Mutex::new(value)) }
    fn lock(&self)->MutexGuard<'_,T> { self.0.lock().unwrap() }
}
type WindowOwner=u64;
#[derive(Clone,Copy)] struct WindowId(u32);
impl WindowId { fn raw(self)->u32 { self.0 } }
#[derive(Debug)] enum WindowBrokerError { InvalidHandle }
struct Window { generation:u16, owner:u64, presented_serials:[u64;8] }
struct Broker { windows:Vec<Window> }
static WINDOW_BROKER:Lock<Broker> = Lock::new(Broker { windows:Vec::new() });
static GUEST:AtomicBool=AtomicBool::new(false);
static OWNER:AtomicU64=AtomicU64::new(9);
static SERIAL:AtomicU64=AtomicU64::new(41);
static RC:AtomicI32=AtomicI32::new(0);
static TRANSPORT_RC:AtomicU64=AtomicU64::new(0);
static CALLS:AtomicU64=AtomicU64::new(0);
static WIRE:Lock<(u32,u64,u64,Vec<u8>)>=Lock::new((0,0,0,Vec::new()));
mod hv { pub fn current_hull_guest_context_vm_id()->Option<u32> {
    super::GUEST.load(super::Ordering::SeqCst).then_some(0)
} }
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_UI4_SOLARA_FRAME_PUBLISH:u32=0xB6;
    pub const OP_BP_UI4_SCENE_FIRST_PRESENTATION_TAKE:u32=0xF5;
    pub const UI4_SCENE_TRACK_PUBLICATION_V1:u64=1<<32;
    pub const STATUS_OK:u64=0;
    pub fn call_with_payload(op:u32,a:u64,b:u64,input:&[u8],output:&mut[u8])->(u64,u64) {
        *crate::WIRE.lock()=(op,a,b,input.to_vec());
        if output.len()==8 { output.copy_from_slice(&crate::SERIAL.load(crate::Ordering::SeqCst).to_le_bytes()); }
        (crate::TRANSPORT_RC.load(crate::Ordering::SeqCst), crate::RC.load(crate::Ordering::SeqCst) as i64 as u64)
    }
} }
'''
    harness += receipt_item("src/ui4/window_broker.rs", "unpack_handle")
    harness += receipt_item("src/ui4/window_broker.rs", "window_frame_was_presented")
    harness += r'''
mod abi {
use super::*;
const ERROR_INVALID:i32=-1; const ERROR_CONTEXT:i32=-2; const ERROR_NOT_FOUND:i32=-3;
const ERROR_STATE:i32=-4; const ERROR_UI4:i32=-6;
struct Surface { owner:u64, id:u32, window:WindowId, render_target:u32 }
static SURFACES:Lock<Vec<Surface>>=Lock::new(Vec::new());
fn blueprint_owner()->Option<u64> { let o=OWNER.load(Ordering::SeqCst); (o!=0).then_some(o) }
fn surface_mut(s:&mut[Surface],o:u64,id:u32)->Option<&mut Surface> { s.iter_mut().find(|s|s.owner==o&&s.id==id) }
fn pack_u32_pair(a:u32,b:u32)->u64 { u64::from(a)<<32 | u64::from(b) }
fn guest_status(op:u32,a:u64,b:u64,payload:&[u8])->i32 {
    trueos_vm::vmcall::call_with_payload(op,a,b,payload,&mut[]).1 as i64 as i32
}
fn publish_blueprint_frame(_:u32,_:u32,_:u32,_:u32,_:u32,out:Option<&mut u64>)->i32 {
    CALLS.fetch_add(1,Ordering::SeqCst);
    if let Some(out)=out { *out=SERIAL.load(Ordering::SeqCst); }
    RC.load(Ordering::SeqCst)
}
fn reset() {
    GUEST.store(false,Ordering::SeqCst); OWNER.store(9,Ordering::SeqCst);
    SERIAL.store(41,Ordering::SeqCst); RC.store(0,Ordering::SeqCst);
    TRANSPORT_RC.store(0,Ordering::SeqCst); CALLS.store(0,Ordering::SeqCst);
    *SURFACES.lock()=vec![Surface {owner:9,id:0x20001,window:WindowId(0x20001),render_target:0x20001}];
    WINDOW_BROKER.lock().windows=vec![Window {generation:2,owner:9,presented_serials:[0,41,0,0,0,0,0,0]}];
}
'''
    for name in ("trueos_cabi_ui4_scene_frame_was_presented_v1",
                 "trueos_cabi_ui4_scene_frame_publish_tracked_v1"):
        harness += receipt_item("src/ui4/blueprint_text.rs", name)
    harness += r'''
#[test] fn exact_serial_owner_generation_and_nonconsuming_query() {
    reset();
    for _ in 0..30 { assert_eq!(trueos_cabi_ui4_scene_frame_was_presented_v1(0x20001,41),0); }
    assert_eq!(trueos_cabi_ui4_scene_frame_was_presented_v1(0x20001,42),1);
    assert_eq!(trueos_cabi_ui4_scene_frame_was_presented_v1(0x20001,0),ERROR_INVALID);
    assert!(!super::window_frame_was_presented(10,WindowId(0x20001),41));
    assert!(!super::window_frame_was_presented(9,WindowId(0x30001),41));
    assert!(!super::window_frame_was_presented(9,WindowId(0),41));
    OWNER.store(10,Ordering::SeqCst);
    assert_eq!(trueos_cabi_ui4_scene_frame_was_presented_v1(0x20001,41),ERROR_NOT_FOUND);
    OWNER.store(0,Ordering::SeqCst);
    assert_eq!(trueos_cabi_ui4_scene_frame_was_presented_v1(0x20001,41),ERROR_CONTEXT);
}
#[test] fn publish_never_returns_a_receipt_for_busy_failure_or_no_commit() {
    reset(); let mut out=999;
    assert_eq!(unsafe {trueos_cabi_ui4_scene_frame_publish_tracked_v1(1,0,0,8,8,std::ptr::null_mut())},ERROR_INVALID);
    assert_eq!(CALLS.load(Ordering::SeqCst),0);
    for error in [-7,-6,-4] {
        RC.store(error,Ordering::SeqCst);
        assert_eq!(unsafe {trueos_cabi_ui4_scene_frame_publish_tracked_v1(1,0,0,8,8,&mut out)},error);
        assert_eq!(out,999);
    }
    RC.store(0,Ordering::SeqCst); SERIAL.store(0,Ordering::SeqCst);
    assert_eq!(unsafe {trueos_cabi_ui4_scene_frame_publish_tracked_v1(1,0,0,8,8,&mut out)},ERROR_STATE);
    assert_eq!(out,999);
    SERIAL.store(41,Ordering::SeqCst);
    assert_eq!(unsafe {trueos_cabi_ui4_scene_frame_publish_tracked_v1(1,0,0,8,8,&mut out)},0);
    assert_eq!(out,41);
}
#[test] fn guest_roundtrip_preserves_full_serial_damage_and_mode() {
    reset(); GUEST.store(true,Ordering::SeqCst);
    let serial=0xABCDEF01_12345678;
    SERIAL.store(serial,Ordering::SeqCst); let mut out=0;
    assert_eq!(unsafe {trueos_cabi_ui4_scene_frame_publish_tracked_v1(0x20001,5,6,1280,720,&mut out)},0);
    assert_eq!(out,serial);
    assert_eq!(*WIRE.lock(),(0xB6,(1u64<<32)|0x20001,(5u64<<32)|6,vec![0,5,0,0,208,2,0,0]));
    assert_eq!(trueos_cabi_ui4_scene_frame_was_presented_v1(0x20001,serial),0);
    assert_eq!(*WIRE.lock(),(0xF5,(1u64<<32)|0x20001,serial,vec![]));
    TRANSPORT_RC.store(1,Ordering::SeqCst); out=999;
    assert_eq!(unsafe {trueos_cabi_ui4_scene_frame_publish_tracked_v1(1,0,0,8,8,&mut out)},ERROR_UI4);
    assert_eq!(out,999);
}
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-receipt-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    test_publication_receipts()
