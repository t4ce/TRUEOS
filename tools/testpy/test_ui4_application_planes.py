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
    source += "mod intel {pub fn active_scanout_dimensions()->Option<(u32,u32)> {Some((3840,2160))}}\n"
    source += "mod ui4 {\nuse alloc::vec::Vec;\n"
    for name in ("OUTPUT_COUNT", "MAX_APPLICATION_PLANE_SLOTS", "ADDITIONAL_APPLICATION_PLANE_SLOT",
                 "DEFAULT_APPLICATION_PLANE_MASK", "SUPPORTED_APPLICATION_PLANE_MASK",
                 "PRIMARY_PLANE_SLOT", "INTERACTION_OVERLAY_PLANE_SLOT"):
        source += constant(MOD, name) + "\n"
    for name in ("is_application_plane_slot", "application_plane_slots", "application_plane_mask", "application_plane_scaling_enabled", "background_plane_slot", "publish_output_capabilities_into"):
        source += item(MOD, name)
    source += item(MOD, "Ui4OutputCapabilities") + impl("Ui4OutputCapabilities", MOD)
    source += r'''
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub struct OutputId(usize);
impl OutputId { pub fn slot(self)->usize { self.0 } }
thread_local! {
    static MASKS: std::cell::Cell<[u8;4]> = const {std::cell::Cell::new([0x0f;4])};
    static SCALING: std::cell::Cell<[u8;4]> = const {std::cell::Cell::new([0x0e;4])};
}
fn ui4_output_capabilities(output:OutputId)->Option<Ui4OutputCapabilities> {
    Some(Ui4OutputCapabilities {output,width:3840,height:2160,application_plane_mask:MASKS.with(|m|m.get()[output.slot()]),application_plane_scaling_mask:SCALING.with(|m|m.get()[output.slot()])})
}
fn set_scaling(output:OutputId, mask:u8) { SCALING.with(|m| {let mut masks=m.get(); masks[output.slot()]=mask;m.set(masks);}); }
fn set_mask(output:OutputId, mask:u8) { MASKS.with(|m| {let mut masks=m.get(); masks[output.slot()]=mask;m.set(masks);}); }
#[test] fn capability_publication_admits_only_supported_application_carriers() {
    for mask in [0x0f,0x2f,0x29] {
        let caps=Ui4OutputCapabilities {output:OutputId(0),width:3840,height:2160,application_plane_mask:mask,application_plane_scaling_mask:mask&0x0e};
        let mut outputs=[None;4];
        assert_eq!(publish_output_capabilities_into(&mut outputs,caps),Ok(()));
        assert_eq!(caps.application_plane_count(),mask.count_ones() as usize);
    }
    for mask in [0,0x10,0x40,0x3f,0x6f,0x2e] {
        let caps=Ui4OutputCapabilities {output:OutputId(0),width:3840,height:2160,application_plane_mask:mask,application_plane_scaling_mask:mask&0x0e};
        assert!(publish_output_capabilities_into(&mut [None;4],caps).is_err());
    }
}
#[test] fn scaling_capability_is_an_admitted_subset_and_part_of_the_immutable_contract() {
    let output=OutputId(0);
    let base=Ui4OutputCapabilities {output,width:3840,height:2160,application_plane_mask:0x2f,
        application_plane_scaling_mask:0};
    for mask in [0,2,4,8,0x0a,0x0e] {
        let caps=Ui4OutputCapabilities {application_plane_scaling_mask:mask,..base};
        assert_eq!(publish_output_capabilities_into(&mut [None;4],caps),Ok(()));
        for slot in 0..8 {assert_eq!(caps.supports_application_plane_scaling(slot),mask&(1<<slot)!=0);}
        assert!(!caps.supports_application_plane_scaling(usize::MAX));
    }
    for mask in [1,0x10,0x20,0x40,0xff] {
        assert_eq!(publish_output_capabilities_into(&mut [None;4],
            Ui4OutputCapabilities {application_plane_scaling_mask:mask,..base}),
            Err("ui4-output-capabilities-invalid-application-plane-scaling-mask"));
    }
    assert!(publish_output_capabilities_into(&mut [None;4],
        Ui4OutputCapabilities {application_plane_mask:0x0b,application_plane_scaling_mask:4,..base}).is_err());
    let mut outputs=[None;4];
    assert_eq!(publish_output_capabilities_into(&mut outputs,base),Ok(()));
    assert_eq!(publish_output_capabilities_into(&mut outputs,
        Ui4OutputCapabilities {application_plane_scaling_mask:0x0e,..base}),
        Err("ui4-output-capabilities-conflict"));
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
#[derive(Clone,Copy)] pub struct WindowSnapshot {pub output:OutputId,pub plane:WindowPlane,pub placement:WindowPlacement,pub presentation_placement:WindowPlacement,pub state:WindowState,pub interaction:Interaction}
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
    source += "}\n"
    for name in ("CLOSE_TRANSITION_DURATION_MS", "CLOSE_TRANSITION_PUFF_PER_MILLE",
                 "OPEN_TRANSITION_SHRINK_PER_MILLE", "DIRECT_PLANE_CLOSE_WAVE_DURATION_MS",
                 "DIRECT_PLANE_CLOSE_PUFF_PER_MILLE", "DIRECT_PLANE_OPEN_SHRINK_PER_MILLE"):
        source += constant(BROKER, name) + "\n"
    for name in ("WindowOpenTransition", "open_transition", "open_transition_placement",
                 "scaled_fade_transition_placement", "centered_scale_coordinate", "close_transition_timing"):
        source += item(BROKER, name)
    source += r'''
fn broker(output:OutputId,count:usize)->WindowBroker {
    WindowBroker {windows:(0..count).map(|z|WindowRecord {generation:1,state:WindowState::Ready,output,plane:WindowPlane::Primary,background:None,
        placement:WindowPlacement {x:0,y:0,width:640,height:480,z:z as i32,opacity:255},damage:None,revision:0,open_transition:None}).collect(),leases:[[None;4];4]}
}
fn slots(broker:&WindowBroker)->Vec<usize> {broker.windows.iter().map(|w|w.plane.slot()).collect()}
#[test] fn an_output_without_scaling_keeps_native_pixels_through_the_full_open_fade() {
    let output=OutputId(0);set_scaling(output,0);
    let initial=WindowPlacement {x:100,y:200,width:800,height:600,z:3,opacity:255};
    for slot in 1..=3 {
        let open=open_transition(initial,WindowPlane::Universal(slot),output,1000).unwrap();
        assert_eq!(open.shrink_per_mille,0);
        let mut previous=0;
        for elapsed in 0..=open.duration_ms {
            let current=open_transition_placement(initial,elapsed,open.duration_ms,open.shrink_per_mille);
            assert_eq!((current.x,current.y,current.width,current.height),(100,200,800,600));
            assert!(current.opacity>=previous);previous=current.opacity;
        }
        assert_eq!(previous,255);
    }
}
#[test] fn admitted_outputs_keep_the_existing_open_scale_and_direct_close_waves() {
    let output=OutputId(0);set_scaling(output,0x0e);
    let initial=WindowPlacement {x:100,y:200,width:800,height:600,z:3,opacity:255};
    let open=open_transition(initial,WindowPlane::Universal(1),output,1000).unwrap();
    assert_eq!((open.current.width,open.current.height),(280,210));
    assert_eq!(open.shrink_per_mille,650);
    assert_eq!(close_transition_timing(WindowPlane::Universal(1),true,true,true,true),(0,200,50));
    assert_eq!(close_transition_timing(WindowPlane::Universal(3),true,true,true,true),(200,200,50));
    assert_eq!(close_transition_timing(WindowPlane::Universal(3),false,true,true,true),(0,300,50));
    assert!(open_transition(initial,WindowPlane::Primary,output,1000).is_none());
    assert!(open_transition(initial,WindowPlane::Universal(5),output,1000).is_none());
}
#[test] fn unavailable_scaling_closes_with_a_fade_and_keeps_the_retirement_boundary() {
    let initial=WindowPlacement {x:100,y:200,width:800,height:600,z:3,opacity:255};
    for slot in [1,2,3,5] {
        for direct in [false,true] {for puff in [false,true] {
            let (delay,duration,puff)=close_transition_timing(WindowPlane::Universal(slot),direct,puff,false,true);
            assert_eq!((delay,duration,puff),(0,200,0));
            let mut previous=255;
            for elapsed in 0..=duration {
                let current=scaled_fade_transition_placement(initial,elapsed,duration,1000+puff);
                assert_eq!((current.x,current.y,current.width,current.height),(100,200,800,600));
                assert!(current.opacity<=previous);previous=current.opacity;
            }
            assert_eq!(previous,0);
        }}
    }
    assert_eq!(close_transition_timing(WindowPlane::Primary,true,true,false,true),(0,0,0));
}
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
use window_broker::{WindowPlacement,WindowSnapshot,WindowPlane,WindowState};
#[derive(Clone,Copy)] struct FrameRgbaView {width:u32,height:u32}
mod compositor {
use super::*;
'''
    for name in ("CompositionTarget", "composition_targets", "native_pixel_placement", "presentation_placement",
                 "bounded_direct_plane_scale", "direct_overlay_geometry_eligible"):
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
    let window=WindowSnapshot {output:OutputId(0),plane:WindowPlane::Universal(5),placement,presentation_placement:placement,state:window_broker::WindowState::Closing,
        interaction:window_broker::Interaction {resize_on_maximize:true}};
    let native=presentation_placement(window,640,480);
    assert_eq!((native.width,native.height),(640,480));
    assert_eq!((native.x,native.y,native.opacity),(10,20,64));
    let existing=presentation_placement(WindowSnapshot {plane:WindowPlane::Universal(3),..window},640,480);
    assert_eq!((existing.width,existing.height),(320,240),"existing scaler carrier remains unchanged");
}
#[test] fn unsupported_scaling_centers_the_native_resize_front_and_preserves_its_alpha() {
    let output=OutputId(0);set_scaling(output,0);
    let placement=WindowPlacement {x:100,y:200,width:800,height:600,z:3,opacity:64};
    let window=WindowSnapshot {output,plane:WindowPlane::Universal(1),placement,
        presentation_placement:placement,state:window_broker::WindowState::Ready,
        interaction:window_broker::Interaction {resize_on_maximize:true}};
    let native=presentation_placement(window,640,480);
    assert_eq!((native.x,native.y,native.width,native.height),(180,260,640,480));
    assert_eq!((native.opacity,native.z),(64,3));
    set_scaling(output,0x0e);
    assert_eq!(presentation_placement(window,640,480),placement,"admitted desktop sizing is unchanged");
}
#[test] fn unsupported_outputs_never_admit_a_scaled_direct_plane() {
    let output=OutputId(0);
    let placement=WindowPlacement {x:100,y:200,width:400,height:300,z:3,opacity:64};
    let mut window=WindowSnapshot {output,plane:WindowPlane::Universal(1),placement,
        presentation_placement:placement,state:window_broker::WindowState::Closing,
        interaction:window_broker::Interaction {resize_on_maximize:true}};
    let view=FrameRgbaView {width:800,height:600};
    set_scaling(output,0);
    assert!(!direct_overlay_geometry_eligible(window,view));
    window.placement=presentation_placement(window,view.width,view.height);
    assert_eq!((window.placement.x,window.placement.y),(0,50),"native source stays within the output");
    assert!(direct_overlay_geometry_eligible(window,view));
    window.placement=placement;set_scaling(output,0x0e);
    assert!(direct_overlay_geometry_eligible(window,view));
}
#[test] fn native_resize_front_clamps_at_every_output_edge_without_losing_pixels() {
    let output=OutputId(0);set_scaling(output,0);
    for (x,y,expected_x,expected_y) in [(0,0,0,0),(3800,2100,3040,1560)] {
        let placement=WindowPlacement {x,y,width:100,height:100,z:0,opacity:255};
        let window=WindowSnapshot {output,plane:WindowPlane::Universal(2),placement,
            presentation_placement:placement,state:window_broker::WindowState::Ready,
            interaction:window_broker::Interaction {resize_on_maximize:true}};
        let native=presentation_placement(window,800,600);
        assert_eq!((native.x,native.y,native.width,native.height),(expected_x,expected_y,800,600));
    }
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
