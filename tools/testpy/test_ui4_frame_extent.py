#!/usr/bin/env python3
"""Compile actual frame capability query and SDK aspect fitting on the host."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item, constant

ROOT = Path(__file__).resolve().parents[2]
KERNEL = "src/ui4/blueprint_ui4.rs"
SDK = "../TRUEOS-Blueprints/api/src/ui4_solara_text.rs"
name = "OP_BP_UI4_SCENE_FRAME_MAX_EXTENT_V1"


def extern_item(path: str, name: str) -> str:
    text = (ROOT / path).read_text()
    start = text.index(f'pub extern "C" fn {name}(')
    opening = text.index("{", start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (text[end] == "{") - (text[end] == "}")
        end += 1
    return text[start:end]

source = r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
static GUEST:AtomicBool=AtomicBool::new(false);
static REPLY:AtomicU64=AtomicU64::new(0);
static STATUS:std::sync::atomic::AtomicI32=std::sync::atomic::AtomicI32::new(0);
static OUTPUT:AtomicU64=AtomicU64::new(0);
static CAPS:AtomicU64=AtomicU64::new(0);
static RENDER_LIMIT:AtomicU64=AtomicU64::new((2560u64<<32)|1440);
#[derive(Clone,Copy)]struct OutputId;
impl OutputId {fn from_slot(slot:usize)->Option<Self>{(slot==0).then_some(Self)}}
struct OutputCaps {width:u32,height:u32}
fn unpack(packed:u64)->Option<(u32,u32)> {
    (packed!=0).then_some(((packed>>32)as u32,packed as u32))
}
fn ui4_output_capabilities(_:OutputId)->Option<OutputCaps> {
    unpack(CAPS.load(Ordering::Acquire)).map(|(width,height)|OutputCaps{width,height})
}
fn output_dimensions()->Option<(u32,u32)> {unpack(OUTPUT.load(Ordering::Acquire))}
mod hv {pub fn current_hull_guest_context_vm_id()->Option<u16> {
    super::GUEST.load(super::Ordering::Acquire).then_some(1)
}}
mod intel {pub mod render {
'''
source += constant("src/intel/render/constants.rs", "RESIDENT_SCENE_TARGET_WIDTH")
source += constant("src/intel/render/constants.rs", "RESIDENT_SCENE_TARGET_HEIGHT")
source += "pub(crate) fn ui4_frame_extent_limit()->(u32,u32){crate::unpack(crate::RENDER_LIMIT.load(crate::Ordering::Acquire)).unwrap()}\n}}\nmod trueos_vm {pub mod vmcall {\n"
source += constant("crates/trueos-vm/src/vmcall.rs", name)
source += r'''
pub const STATUS_OK:i32=0;
pub fn call_with_payload(op:u32,a:u64,b:u64,input:&[u8],output:&mut[u8])->(i32,u64) {
    assert_eq!(op,OP_BP_UI4_SCENE_FRAME_MAX_EXTENT_V1);
    assert_eq!((a,b,input.len(),output.len()),(0,0,0,0));
    (crate::STATUS.load(crate::Ordering::Acquire),crate::REPLY.load(crate::Ordering::Acquire))
}
}}
mod kernel {
use crate::trueos_vm;
use crate::OutputId;
fn pack_u32_pair(a:u32,b:u32)->u64 {(u64::from(a)<<32)|u64::from(b)}
'''
source += constant(KERNEL, "MAX_FRAME_WIDTH")
source += constant(KERNEL, "MAX_FRAME_HEIGHT")
source += item(KERNEL, "blueprint_frame_limits_for_output")
source += item(KERNEL, "blueprint_frame_max_extent")
source += item(KERNEL, "blueprint_frame_extent_admitted").replace(
    "fn blueprint_frame_extent_admitted", "pub(crate) fn blueprint_frame_extent_admitted"
)
source += extern_item(KERNEL, "trueos_cabi_ui4_scene_frame_max_extent_v1")
source += "}\nmod v {pub mod bp_abi {\n"
source += "pub unsafe fn trueos_cabi_ui4_scene_frame_max_extent_v1()->u64 {crate::kernel::trueos_cabi_ui4_scene_frame_max_extent_v1()}\n}}\n"
source += item(SDK, "Error")
source += item(SDK, "frame_max_extent")
source += item(SDK, "fit_frame_extent")
source += r'''
fn cap(w:u32,h:u32) {GUEST.store(true,Ordering::Release);STATUS.store(0,Ordering::Release);
    REPLY.store((u64::from(w)<<32)|u64::from(h),Ordering::Release);}
#[test]fn capability_pack_and_transport_failure_are_separate_from_output_resolution() {
    GUEST.store(false,Ordering::Release);
    assert_eq!(frame_max_extent().unwrap(),
        (intel::render::RESIDENT_SCENE_TARGET_WIDTH as u32,intel::render::RESIDENT_SCENE_TARGET_HEIGHT as u32));
    cap(3840,2160);assert_eq!(frame_max_extent().unwrap(),(3840,2160));
    STATUS.store(-1,Ordering::Release);assert_eq!(frame_max_extent().unwrap(),(2560,1440));
    for (w,h) in [(0,0),(3840,0),(0,2160)] {cap(w,h);assert_eq!(frame_max_extent().unwrap(),(2560,1440));}
}
#[test]fn fits_a_4k_panel_to_the_actual_advertised_render_cap() {
    cap(2560,1440);assert_eq!(fit_frame_extent(2880,1620).unwrap(),(2560,1440));
    cap(3840,2160);assert_eq!(fit_frame_extent(2880,1620).unwrap(),(2880,1620));
    assert_eq!(fit_frame_extent(3840,2160).unwrap(),(3840,2160));
}
#[test]fn admission_and_query_share_active_output_and_renderer_limits() {
    GUEST.store(false,Ordering::Release);
    CAPS.store((3840u64<<32)|2160,Ordering::Release);
    OUTPUT.store((1920u64<<32)|1080,Ordering::Release);
    RENDER_LIMIT.store((3840u64<<32)|2160,Ordering::Release);
    assert_eq!(frame_max_extent().unwrap(),(3840,2160),"published D01 geometry takes priority");
    assert!(kernel::blueprint_frame_extent_admitted(3840,2160));
    assert!(!kernel::blueprint_frame_extent_admitted(3841,2160));
    assert!(!kernel::blueprint_frame_extent_admitted(3840,2161));
    assert!(!kernel::blueprint_frame_extent_admitted(0,1));
    RENDER_LIMIT.store((2560u64<<32)|1440,Ordering::Release);
    assert_eq!(frame_max_extent().unwrap(),(2560,1440),"legacy renderer remains bounded on4K output");
    assert!(!kernel::blueprint_frame_extent_admitted(2880,1620));
    CAPS.store(0,Ordering::Release);
    assert_eq!(frame_max_extent().unwrap(),(1920,1080),"active native/emulator dimensions are fallback");
    OUTPUT.store(0,Ordering::Release);RENDER_LIMIT.store((3840u64<<32)|2160,Ordering::Release);
    assert_eq!(frame_max_extent().unwrap(),(2560,1440),"no output retains historical admission");
    RENDER_LIMIT.store((2560u64<<32)|1440,Ordering::Release);
}
#[test]fn preserves_small_extents_and_fits_portrait_and_ultrawide() {
    cap(2560,1440);
    for extent in [(1,1),(800,600),(1440,810),(2560,1440)] {
        assert_eq!(fit_frame_extent(extent.0,extent.1).unwrap(),extent);
    }
    assert_eq!(fit_frame_extent(2160,3840).unwrap(),(810,1440));
    assert_eq!(fit_frame_extent(7680,2160).unwrap(),(2560,720));
    assert_eq!(fit_frame_extent(u32::MAX,u32::MAX).unwrap(),(1440,1440));
    assert_eq!(fit_frame_extent(u32::MAX,1).unwrap(),(2560,1));
    assert_eq!(fit_frame_extent(1,u32::MAX).unwrap(),(1,1440));
    assert_eq!(fit_frame_extent(0,1),Err(Error::Invalid));
    assert_eq!(fit_frame_extent(1,0),Err(Error::Invalid));
}
'''

# Transport IDs must agree without assuming any neighboring operation numbers.
assert constant("src/hv/vmcall.rs", name).split("=")[1].split(";")[0] == constant(
    "crates/trueos-vm/src/vmcall.rs", name
).split("=")[1].split(";")[0]
assert "OP_BP_UI4_SCENE_FRAME_MAX_EXTENT_V1 =>" in (ROOT / "src/hv/vmcall.rs").read_text()
with tempfile.TemporaryDirectory(prefix="trueos-ui4-frame-extent-") as directory:
    path = Path(directory)
    (path / "tests.rs").write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "tests.rs"),
                    "-o", str(path / "tests")], check=True, cwd=ROOT)
    subprocess.run([str(path / "tests"), "--test-threads=1"], check=True)
