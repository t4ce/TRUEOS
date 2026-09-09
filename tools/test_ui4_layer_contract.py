#!/usr/bin/env python3
"""Exercise production weighted lease policy and broker application on the host."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def method(name):
    source = (ROOT / 'src/ui4/window_broker.rs').read_text()
    start = source.index('    fn ' + name + '(')
    end = source.index('\n    }', start) + len('\n    }')
    return source[start:end]

def main():
    source = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::vec::Vec;
#[path = "LAYER_PATH"] mod layer_contract;
const LEASE_PLANE_COUNT: usize = 3;
const FIRST_LEASE_PLANE_SLOT: usize = 1;
const INTERACTION_OVERLAY_PLANE_SLOT: usize = 4;
type OutputId = u8;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)] struct WindowId(u32);
impl WindowId { fn raw(self)->u32 { self.0 } fn from_raw(raw:u32)->Option<Self> { (raw!=0).then_some(Self(raw)) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] enum WindowPlane { Primary, Universal(u8), Interaction }
impl WindowPlane {
    fn is_application(self)->bool { self!=Self::Interaction }
    fn from_slot(slot:usize)->Option<Self> { match slot { 0=>Some(Self::Primary),1..=3=>Some(Self::Universal(slot as u8)),_=>None } }
    fn slot(self)->usize { match self { Self::Primary=>0,Self::Universal(slot)=>slot as usize,Self::Interaction=>4 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)] enum WindowState { Ready, Closed }
#[derive(Clone, Copy)] struct Placement { z:i32 }
#[derive(Clone, Copy)] struct DamageRegion;
impl DamageRegion { const FULL:Self=Self; }
fn next_serial(serial:u64)->u64 { serial.wrapping_add(1).max(1) }
#[derive(Clone, Copy)] struct Record { damage:Option<DamageRegion>, revision:u64, generation:u16,background:Option<()>,output:OutputId,plane:WindowPlane,state:WindowState,placement:Placement,open_transition:Option<()> }
#[derive(Clone, Copy)] struct PlaneLease { window:WindowId,last_hot_ms:u64 }
fn pack_handle(slot:usize,generation:u16)->Result<u32,()> { Ok((generation as u32)<<16 | slot as u32+1) }
fn unpack_handle(raw:u32)->Result<(usize,u16),()> { Ok(((raw as u16-1) as usize,(raw>>16) as u16)) }
struct WindowBroker { windows:Vec<Record>,leases:[Option<PlaneLease>;3] }
impl WindowBroker {
    fn set_window_plane(&mut self,id:WindowId,plane:WindowPlane) { self.windows[unpack_handle(id.raw()).unwrap().0].plane=plane; }
    fn demote_to_stack(&mut self,id:WindowId) { self.set_window_plane(id,WindowPlane::Primary); }
    fn lease_index(slot:usize)->Option<usize> { (1..=3).contains(&slot).then(||slot-1) }
    fn lease_plane(index:usize)->WindowPlane { WindowPlane::Universal(index as u8+1) }
METHODS
}
fn broker(widths:&[u8])->WindowBroker {
    WindowBroker { windows: widths.iter().enumerate().map(|(index,width)| Record { damage:None,revision:0,generation:1,background:(*width==2).then_some(()),output:0,plane:WindowPlane::Primary,state:WindowState::Ready,placement:Placement{z:index as i32},open_transition:None }).collect(),leases:[None;3] }
}
fn id(slot:usize)->WindowId { WindowId(pack_handle(slot,1).unwrap()) }
#[test] fn two_duals_do_not_share_or_split_the_remaining_single_slot() {
    let mut b=broker(&[2,2]);b.rebalance_application_planes(0,1000);
    assert_eq!(b.windows[0].plane,WindowPlane::Primary);
    assert_eq!(b.windows[1].plane,WindowPlane::Universal(3));
    assert!(b.leases[0].is_none());
    assert_eq!(b.leases[1].unwrap().window,id(1));assert_eq!(b.leases[2].unwrap().window,id(1));
    assert_eq!(b.group_plan(0).iter().flatten().count(),1);
}
#[test] fn foreground_plane_tracks_the_whole_group_on_focus_swap() {
    let mut b=broker(&[2,1]);b.rebalance_application_planes(0,1000);
    let plan=layer_contract::claim_group(b.group_plan(0),layer_contract::LeaseGroup{window:id(0).raw(),slots:2,last_hot_ms:1010},1010,500,true).unwrap();
    b.apply_group_plan(0,plan);
    assert_eq!(b.windows[0].plane.slot(),3);assert_eq!(b.windows[1].plane.slot(),1);
    assert_eq!(b.leases[1].unwrap().window,id(0));assert_eq!(b.leases[2].unwrap().window,id(0));
}
#[test] fn selecting_stacked_pair_releases_two_single_windows() {
    let mut b=broker(&[2,1,1,1]);b.rebalance_application_planes(0,0);
    let plan=layer_contract::claim_group(b.group_plan(0),layer_contract::LeaseGroup{window:id(0).raw(),slots:2,last_hot_ms:1000},1000,500,true).unwrap();
    b.apply_group_plan(0,plan);
    assert_eq!(b.windows.iter().filter(|w|w.plane==WindowPlane::Primary).count(),2);
    assert_eq!(b.windows[0].plane.slot(),3);
    assert_eq!(b.group_plan(0).iter().flatten().count(),2);
}
#[test] fn every_mixed_topology_preserves_whole_suffix_order() {
    for mask in 1u32..256 {
        let widths:Vec<_>=(0..8).map(|i| if mask&(1<<i)==0 {1}else{2}).collect();
        let mut b=broker(&widths);b.rebalance_application_planes(0,10);
        let mut previous=0;
        for (index,w) in b.windows.iter().enumerate() {
            let slot=w.plane.slot();
            if slot!=0 {
                assert!(slot>previous);previous=slot;
                let count=b.leases.iter().flatten().filter(|lease|lease.window==id(index)).count();
                assert_eq!(count,widths[index] as usize);
                if widths[index]==2 { assert!(slot>=2);assert_eq!(b.leases[slot-2].unwrap().window,id(index)); }
            }
        }
        assert!(layer_contract::used(b.group_plan(0))<=3);
    }
}
'''
    source = source.replace('LAYER_PATH', str(ROOT / 'src/ui4/layer_contract.rs'))
    source = source.replace('METHODS', '\n'.join(method(name) for name in ['rebalance_application_planes', 'group_plan', 'apply_group_plan']))
    # Production methods refer to the parent UI4 module.
    source = source.replace('super::layer_contract::', 'crate::layer_contract::')
    with tempfile.TemporaryDirectory(prefix='trueos-layer-contract-') as directory:
        path=Path(directory)/'tests.rs';path.write_text(source)
        binary=Path(directory)/'tests'
        subprocess.run(['rustc','--edition=2024','--test',str(path),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)

if __name__ == '__main__': main()
