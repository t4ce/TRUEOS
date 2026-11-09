#!/usr/bin/env python3
"""Exercise the production sparse-plane broker and compositor without MMIO.

Only window storage, display discovery and handles are fixtures. Plane
selection, whole-group packing, focus reassignment, compositor targets and
native presentation geometry are extracted from the actual kernel code.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]
MOD = "src/ui4/mod.rs"
BROKER = "src/ui4/window_broker.rs"
COMPOSITOR = "src/ui4/compositor_service.rs"


def method(name: str) -> str:
    source = (ROOT / BROKER).read_text()
    match = re.search(rf"^    (?:const )?fn {name}\b", source, re.M)
    assert match, name
    end = re.search(r"^    }\s*$", source[match.end():], re.M)
    assert end, name
    return source[match.start():match.end() + end.end()] + "\n"


def impl(name: str, path: str = BROKER) -> str:
    source = (ROOT / path).read_text()
    match = re.search(rf"^impl {name}\s*\{{", source, re.M)
    end = re.search(r"^}\s*$", source[match.end():], re.M)
    return source[match.start():match.end() + end.end()] + "\n"


def main() -> None:
    source = "#![allow(dead_code)]\nextern crate alloc;\n"
    source += "mod ui4 {\nuse alloc::vec::Vec;\n"
    for name in ("OUTPUT_COUNT", "MAX_APPLICATION_PLANE_SLOTS", "ADDITIONAL_APPLICATION_PLANE_SLOT",
                 "DEFAULT_APPLICATION_PLANE_MASK", "SUPPORTED_APPLICATION_PLANE_MASK",
                 "PRIMARY_PLANE_SLOT", "INTERACTION_OVERLAY_PLANE_SLOT"):
        source += constant(MOD, name) + "\n"
    for name in ("is_application_plane_slot", "application_plane_slots", "application_plane_mask", "background_plane_slot", "publish_output_capabilities_into"):
        source += item(MOD, name)
    source += item(MOD, "Ui4OutputCapabilities") + impl("Ui4OutputCapabilities", MOD)
    source += r'''
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub struct OutputId(usize);
impl OutputId { pub fn slot(self)->usize { self.0 } }
thread_local! { static MASKS: std::cell::Cell<[u8;4]> = const {std::cell::Cell::new([0x0f;4])}; }
fn ui4_output_capabilities(output:OutputId)->Option<Ui4OutputCapabilities> {
    Some(Ui4OutputCapabilities {output,width:3840,height:2160,application_plane_mask:MASKS.with(|m|m.get()[output.slot()])})
}
fn set_mask(output:OutputId, mask:u8) { MASKS.with(|m| {let mut masks=m.get(); masks[output.slot()]=mask;m.set(masks);}); }
#[test] fn capability_publication_admits_only_supported_application_carriers() {
    for mask in [0x0f,0x2f,0x29] {
        let caps=Ui4OutputCapabilities {output:OutputId(0),width:3840,height:2160,application_plane_mask:mask};
        let mut outputs=[None;4];
        assert_eq!(publish_output_capabilities_into(&mut outputs,caps),Ok(()));
        assert_eq!(caps.application_plane_count(),mask.count_ones() as usize);
    }
    for mask in [0,0x10,0x40,0x3f,0x6f,0x2e] {
        let caps=Ui4OutputCapabilities {output:OutputId(0),width:3840,height:2160,application_plane_mask:mask};
        assert!(publish_output_capabilities_into(&mut [None;4],caps).is_err());
    }
}
'''
    source += '#[path="' + str(ROOT / "src/ui4/layer_contract.rs") + '"] mod layer_contract;\n'
    source += "mod window_broker {\nuse super::*;\n"
    source += item(BROKER, "WindowPlane") + impl("WindowPlane")
    source += r'''
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)] pub struct WindowId(u32);
impl WindowId {fn raw(self)->u32 {self.0} fn from_raw(v:u32)->Option<Self> {(v!=0).then_some(Self(v))}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum WindowState {Ready,Closing,Closed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub struct WindowPlacement {pub x:i32,pub y:i32,pub width:u32,pub height:u32,pub z:i32,pub opacity:u8}
#[derive(Clone,Copy)] pub struct Interaction {pub resize_on_maximize:bool}
#[derive(Clone,Copy)] pub struct WindowSnapshot {pub plane:WindowPlane,pub placement:WindowPlacement,pub presentation_placement:WindowPlacement,pub state:WindowState,pub interaction:Interaction}
#[derive(Clone,Copy)] struct DamageRegion;
impl DamageRegion {const FULL:Self=Self;}
#[derive(Clone,Copy)] struct PlaneLease {window:WindowId,last_hot_ms:u64}
struct WindowRecord {generation:u16,state:WindowState,output:OutputId,plane:WindowPlane,background:Option<()>,placement:WindowPlacement,damage:Option<DamageRegion>,revision:u64,open_transition:Option<()>}
impl WindowRecord {fn skip_stack_transitions(&mut self) {} fn set_stack_z(&mut self,z:i32) {self.placement.z=z;}}
struct WindowBroker {windows:Vec<WindowRecord>,leases:[[Option<PlaneLease>;4];4]}
const STACK_PLANE_SLOT:usize=0;
const MAX_LEASE_PLANE_COUNT:usize=4;
#[derive(Debug)] enum WindowBrokerError {InvalidHandle}
fn pack_handle(slot:usize,generation:u16)->Result<u32,WindowBrokerError> {Ok((u32::from(generation)<<16)|(slot as u32+1))}
fn unpack_handle(raw:u32)->Result<(usize,u16),WindowBrokerError> {if raw as u16==0 {Err(WindowBrokerError::InvalidHandle)} else {Ok(((raw as u16-1) as usize,(raw>>16) as u16))}}
fn next_serial(serial:u64)->u64 {serial+1}
impl WindowBroker {
'''
    for name in ("lease_index", "release_lease_of", "demote_to_stack", "set_window_plane",
                 "rebalance_application_planes", "rebalance_application_planes_with_mask", "apply_group_plan",
                 "focus_window", "focus_window_with_mask"):
        source += method(name)
    source += r'''
}
fn broker(output:OutputId,count:usize)->WindowBroker {
    WindowBroker {windows:(0..count).map(|z|WindowRecord {generation:1,state:WindowState::Ready,output,plane:WindowPlane::Primary,background:None,
        placement:WindowPlacement {x:0,y:0,width:640,height:480,z:z as i32,opacity:255},damage:None,revision:0,open_transition:None}).collect(),leases:[[None;4];4]}
}
fn slots(broker:&WindowBroker)->Vec<usize> {broker.windows.iter().map(|w|w.plane.slot()).collect()}
#[test] fn desktop_still_has_three_leases_and_overflows_to_stack() {
    let output=OutputId(0); let mut b=broker(output,6);
    b.rebalance_application_planes(output,100);
    assert_eq!(slots(&b),[0,0,0,1,2,3]);
    assert_eq!(b.leases[0].iter().flatten().count(),3);
    assert!(b.leases[0][3].is_none());
}
#[test] fn extra_carrier_is_admitted_only_by_its_output_mask() {
    let output=OutputId(0);set_mask(output,0x2f);let mut b=broker(output,6);
    b.rebalance_application_planes(output,100);
    assert_eq!(slots(&b),[0,0,1,2,3,5]);
    assert_eq!(b.leases[0].iter().flatten().count(),4);
    let other=OutputId(1); let start=b.windows.len(); let extra=broker(other,4);
    b.windows.extend(extra.windows);
    b.rebalance_application_planes(other,101);
    assert_eq!(&slots(&b)[start..],[0,1,2,3]);
    assert_eq!(b.leases[0].iter().flatten().count(),4,"other output must not clear laptop leases");
}
#[test] fn focus_moves_one_frame_to_extra_carrier_without_reordering_peers() {
    let output=OutputId(0);set_mask(output,0x2f);let mut b=broker(output,5);
    b.rebalance_application_planes(output,100);
    assert_eq!(slots(&b),[0,1,2,3,5]);
    b.focus_window(WindowId(1<<16|1),output,101);
    assert_eq!(slots(&b),[5,0,1,2,3]);
    assert_eq!(b.windows.iter().map(|w|w.placement.z).collect::<Vec<_>>(),[4,0,1,2,3]);
}
#[test] fn layered_pairs_use_adjacent_advertised_carriers_across_the_reserved_hole() {
    let output=OutputId(0);set_mask(output,0x2f);let mut b=broker(output,2);
    for w in &mut b.windows {w.background=Some(());}
    b.rebalance_application_planes(output,100);
    assert_eq!(slots(&b),[2,5]);
    assert_eq!(background_plane_slot(0x2f,5),Some(3));
    assert_eq!(background_plane_slot(0x2f,2),Some(1));
    let holders:Vec<_>=b.leases[0].iter().flatten().map(|lease|lease.window.raw()).collect();
    assert_eq!(holders,[0x10001,0x10001,0x10002,0x10002]);
    set_mask(output,0x0f);b.rebalance_application_planes(output,101);
    assert_eq!(slots(&b),[0,3],"desktop must demote the whole lower pair");
}
#[test] fn sparse_masks_never_lease_unadvertised_or_reserved_slots() {
    let output=OutputId(0);set_mask(output,0b10_1001);let mut b=broker(output,4);
    b.rebalance_application_planes(output,100);
    assert_eq!(slots(&b),[0,0,3,5]);
    assert!(WindowPlane::Universal(5).valid());
    assert!(!WindowPlane::Universal(4).valid());
    assert!(!WindowPlane::Universal(6).valid());
    assert!(WindowPlane::Interaction.stack_rank()>WindowPlane::Universal(5).stack_rank());
}
}
use window_broker::{WindowPlacement,WindowSnapshot,WindowPlane};
mod compositor {
use super::*;
'''
    for name in ("CompositionTarget", "composition_targets", "presentation_placement"):
        source += item(COMPOSITOR, name)
    source += r'''
#[test] fn plans_follow_capabilities_without_interaction_or_diagnostics() {
    use CompositionTarget::{Primary,Overlay};
    assert_eq!(composition_targets(0x0f).collect::<Vec<_>>(),[Primary,Overlay(1),Overlay(2),Overlay(3)]);
    assert_eq!(composition_targets(0x2f).collect::<Vec<_>>(),[Primary,Overlay(1),Overlay(2),Overlay(3),Overlay(5)]);
    assert_eq!(composition_targets(0xff).collect::<Vec<_>>(),[Primary,Overlay(1),Overlay(2),Overlay(3),Overlay(5)]);
}
#[test] fn extra_carrier_keeps_native_source_geometry_without_a_scaler() {
    let placement=WindowPlacement {x:10,y:20,width:320,height:240,z:0,opacity:64};
    let window=WindowSnapshot {plane:WindowPlane::Universal(5),placement,presentation_placement:placement,state:window_broker::WindowState::Closing,
        interaction:window_broker::Interaction {resize_on_maximize:true}};
    let native=presentation_placement(window,640,480);
    assert_eq!((native.width,native.height),(640,480));
    assert_eq!((native.x,native.y,native.opacity),(10,20,64));
    let existing=presentation_placement(WindowSnapshot {plane:WindowPlane::Universal(3),..window},640,480);
    assert_eq!((existing.width,existing.height),(320,240),"existing scaler carrier remains unchanged");
}
}
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-app-planes-") as directory:
        code = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(code), "-o", str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
