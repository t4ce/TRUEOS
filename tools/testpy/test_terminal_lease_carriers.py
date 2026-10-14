#!/usr/bin/env python3
"""Compile the real terminal lease CABI and implicit claim path.

Mock VMX transport and host ownership only. Native carriers must use their VM
context directly, while Hull calls use VMX. Ownership cannot be cached across
parking or VM changes, and transport/lease errors must retain their ABI codes.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    path = ROOT / "src/r/io/fs_cabi.rs"
    definitions = re.search(
        r"^unsafe fn blueprint_terminal_lease_v1\(.*?^}",
        path.read_text(), re.MULTILINE | re.DOTALL,
    ).group() + "\n"
    definitions += "\n".join(function(path, name) for name in (
        "trueos_cabi_blueprint_terminal_lease_current_v1",
        "trueos_cabi_blueprint_terminal_lease_release_v1",
        "trueos_cabi_blueprint_terminal_lease_poll_reentry_v1",
    ))
    definitions += "\n" + function(path, "claim_attached_console_for_terminal_io")
    harness = r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
const TERMINAL_LEASE_CABI_TRANSPORT_ERROR:i32=-6;
std::thread_local! {
    static VM:std::cell::Cell<Option<u8>>=const { std::cell::Cell::new(None) };
    static HULL:std::cell::Cell<bool>=const { std::cell::Cell::new(false) };
}
static HOST_CALLS:Mutex<Vec<(u8,u32,u64)>>=Mutex::new(Vec::new());
static VMCALLS:AtomicUsize=AtomicUsize::new(0);
static STATE:Mutex<u8>=Mutex::new(0); // reserved, active, parked, pending reentry
mod hv {
    pub fn current_guest_execution_context_vm_id()->Option<u8> { crate::VM.with(|v|v.get()) }
    pub fn current_hull_guest_context_vm_id()->Option<u8> {
        if crate::HULL.with(|v|v.get()) { current_guest_execution_context_vm_id() } else {None}
    }
    pub enum BlueprintTerminalReentryPoll {Pending,Ready(u64)}
    pub struct Error(u32);
    impl Error {pub fn code(&self)->u32 {self.0}}
    pub fn blueprint_terminal_lease_current(vm:u8,value:u64)->Result<u64,Error> {
        crate::HOST_CALLS.lock().unwrap().push((vm,1,value));
        let mut state=crate::STATE.lock().unwrap();
        if *state>1 {return Err(Error(3));}
        if value!=0 && value!=1 {return Err(Error(4));}
        *state=1;Ok(1)
    }
    pub fn blueprint_terminal_lease_release(vm:u8,value:u64)->Result<u64,Error> {
        crate::HOST_CALLS.lock().unwrap().push((vm,2,value));
        let mut state=crate::STATE.lock().unwrap();
        if *state!=1 {return Err(Error(3));}
        if value!=0 && value!=1 {return Err(Error(4));}
        *state=2;Ok(1)
    }
    pub fn blueprint_terminal_lease_poll_reentry(vm:u8,value:u64)->Result<BlueprintTerminalReentryPoll,Error> {
        crate::HOST_CALLS.lock().unwrap().push((vm,3,value));
        if value!=1 {return Err(Error(4));}
        match *crate::STATE.lock().unwrap() {
            2=>Ok(BlueprintTerminalReentryPoll::Pending),
            3=>Ok(BlueprintTerminalReentryPoll::Ready(2)),
            _=>Err(Error(4)),
        }
    }
}
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_TERMINAL_LEASE_CURRENT_V1:u32=1;
    pub const OP_BP_TERMINAL_LEASE_RELEASE_V1:u32=2;
    pub const OP_BP_TERMINAL_LEASE_POLL_REENTRY_V1:u32=3;
    pub const STATUS_OK:u32=0;
    pub fn call(op:u32,value:u64,_:u64)->(u32,u64) {
        assert!(crate::hv::current_hull_guest_context_vm_id().is_some(),"native carriers cannot vmcall");
        crate::VMCALLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        let vm=crate::hv::current_guest_execution_context_vm_id().unwrap();
        let result=match op {
            1=>crate::hv::blueprint_terminal_lease_current(vm,value),
            2=>crate::hv::blueprint_terminal_lease_release(vm,value),
            3=>crate::hv::blueprint_terminal_lease_poll_reentry(vm,value).map(|p| match p {
                crate::hv::BlueprintTerminalReentryPoll::Pending=>0,
                crate::hv::BlueprintTerminalReentryPoll::Ready(epoch)=>epoch,
            }),
            _=>panic!("invalid transport operation"),
        };
        match result {Ok(epoch)=>(0,epoch),Err(error)=>(1,error.code() as u64)}
    }
} }
fn setup(vm:Option<u8>,hull:bool) {
    VM.with(|v|v.set(vm)); HULL.with(|v|v.set(hull));
    *STATE.lock().unwrap()=0;HOST_CALLS.lock().unwrap().clear();VMCALLS.store(0,Ordering::Relaxed);
}
#[test] fn implicit_raw_mode_claim_works_on_native_carrier() {
    setup(Some(7),false); claim_attached_console_for_terminal_io();
    assert_eq!(*STATE.lock().unwrap(),1);
    assert_eq!(*HOST_CALLS.lock().unwrap(),vec![(7,1,0)]);
    assert_eq!(VMCALLS.load(Ordering::Relaxed),0);
}
#[test] fn claim_does_not_cache_another_vm_or_previous_ownership() {
    setup(Some(7),false);claim_attached_console_for_terminal_io();
    let mut ticket=0;assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_release_v1(1,&mut ticket)},0);
    claim_attached_console_for_terminal_io();
    assert_eq!(*STATE.lock().unwrap(),2,"a parked VM must not automatically steal input back");
    VM.with(|v|v.set(Some(8)));*STATE.lock().unwrap()=0;
    claim_attached_console_for_terminal_io();
    assert_eq!(HOST_CALLS.lock().unwrap().last(),Some(&(8,1,0)));
    assert_eq!(*STATE.lock().unwrap(),1);
}
#[test] fn native_current_release_pending_and_reentry_preserve_values() {
    setup(Some(7),false);let mut epoch=99;
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_current_v1(0,&mut epoch)},0);assert_eq!(epoch,1);
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_release_v1(2,&mut epoch)},-4);
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_release_v1(1,&mut epoch)},0);
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_poll_reentry_v1(1,&mut epoch)},1);assert_eq!(epoch,1);
    *STATE.lock().unwrap()=3;
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_poll_reentry_v1(1,&mut epoch)},0);assert_eq!(epoch,2);
}
#[test] fn hull_current_release_and_pending_still_use_transport() {
    setup(Some(7),true);let mut epoch=0;
    claim_attached_console_for_terminal_io();
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_release_v1(1,&mut epoch)},0);
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_poll_reentry_v1(1,&mut epoch)},1);
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_current_v1(0,&mut epoch)},-3);
    assert_eq!(VMCALLS.load(Ordering::Relaxed),4);
}
#[test] fn missing_vm_and_null_outputs_do_not_claim() {
    setup(None,false);let mut epoch=0;
    claim_attached_console_for_terminal_io();
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_current_v1(0,&mut epoch)},-6);
    VM.with(|v|v.set(Some(7)));
    assert_eq!(unsafe {trueos_cabi_blueprint_terminal_lease_current_v1(0,std::ptr::null_mut())},-6);
    assert!(HOST_CALLS.lock().unwrap().is_empty());
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-lease-carriers-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(harness + definitions)
        subprocess.run(["rustc", "--edition=2024", "--test", "--target",
                        "x86_64-unknown-linux-gnu", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
