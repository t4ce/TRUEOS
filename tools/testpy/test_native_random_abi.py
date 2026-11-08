#!/usr/bin/env python3
"""Fault-inject the production fallible RNG entry point on both execution paths."""
from pathlib import Path
import subprocess
import tempfile
from test_thread_control_abi import function

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
use std::{cell::Cell, ptr, slice, sync::atomic::{AtomicUsize, Ordering}};
const TRUEOS_EINVAL:i32=22;
const TRUEOS_EIO:i32=5;
thread_local! { static GUEST:Cell<bool>=const{Cell::new(false)}; }
static CALLS:AtomicUsize=AtomicUsize::new(0);
static DIRECT:AtomicUsize=AtomicUsize::new(0);
static FAIL:AtomicUsize=AtomicUsize::new(0);
static INVALID:AtomicUsize=AtomicUsize::new(0);
fn abi_host_ptr(p:*mut u8,_:usize)->Option<*mut u8>{(INVALID.load(Ordering::SeqCst)==0).then_some(p)}
mod hv {pub fn current_hull_guest_context_vm_id()->Option<u8>{crate::GUEST.with(|g|g.get().then_some(1))}}
mod tyche {pub fn fill_bytes(b:&mut[u8])->bool{
 crate::DIRECT.fetch_add(1,crate::Ordering::SeqCst);b.fill(91);crate::FAIL.load(crate::Ordering::SeqCst)==0
}}
mod trueos_vm {pub mod vmcall {
 pub const PAYLOAD_CAP:usize=16;pub const STATUS_OK:u32=0;pub const OP_RAND_BYTES:u32=6;
 pub fn call_with_payload_reported_len(op:u32,n:u64,arg:u64,req:&[u8],out:&mut[u8])->(u32,u64,usize){
  assert!(crate::GUEST.with(|g|g.get()),"raw vmcall on a hosted worker");
  assert_eq!(op,OP_RAND_BYTES);assert_eq!(arg,0);assert!(req.is_empty());assert_eq!(out.len(),n as usize);
  let call=crate::CALLS.fetch_add(1,crate::Ordering::SeqCst)+1;out.fill(call as u8);
  if call==2 {match crate::FAIL.load(crate::Ordering::SeqCst){
   1=>return(1,n,out.len()),2=>return(0,n-1,out.len()),3=>return(0,n,out.len()-1),4=>return(0,n,out.len()+1),_=>{}
  }}(STATUS_OK,n,out.len())
 }
}}
@FUNCTION@
fn reset(guest:bool){GUEST.with(|g|g.set(guest));for a in [&CALLS,&DIRECT,&FAIL,&INVALID]{a.store(0,Ordering::SeqCst)}}
#[test]fn empty_and_invalid_storage_never_call_provider(){reset(false);unsafe{
 assert_eq!(trueos_cabi_random_bytes_v1(ptr::null_mut(),0),0);
 assert_eq!(trueos_cabi_random_bytes_v1(ptr::null_mut(),1),-22);
 assert_eq!(trueos_cabi_random_bytes_v1(ptr::dangling_mut(),usize::MAX),-22);
 INVALID.store(1,Ordering::SeqCst);let mut b=[7;4];assert_eq!(trueos_cabi_random_bytes_v1(b.as_mut_ptr(),4),-22);assert_eq!(b,[7;4]);
}assert_eq!(CALLS.load(Ordering::SeqCst)+DIRECT.load(Ordering::SeqCst),0)}
#[test]fn guest_checks_exact_response_and_chunks(){reset(true);let mut b=[7;35];unsafe{assert_eq!(trueos_cabi_random_bytes_v1(b.as_mut_ptr(),b.len()),0)}
 assert_eq!(&b[..16],&[1;16]);assert_eq!(&b[16..32],&[2;16]);assert_eq!(&b[32..],&[3;3]);assert_eq!(DIRECT.load(Ordering::SeqCst),0);
}
#[test]fn every_partial_guest_failure_clears_entire_output(){for mode in 1..=4{reset(true);FAIL.store(mode,Ordering::SeqCst);let mut b=[7;35];unsafe{assert_eq!(trueos_cabi_random_bytes_v1(b.as_mut_ptr(),b.len()),-5)}assert_eq!(b,[0;35]);assert_eq!(CALLS.load(Ordering::SeqCst),2)}}
#[test]fn hosted_worker_uses_kernel_provider(){reset(false);let mut b=std::mem::MaybeUninit::<[u8;35]>::uninit();unsafe{assert_eq!(trueos_cabi_random_bytes_v1(b.as_mut_ptr().cast(),35),0);assert_eq!(b.assume_init(),[91;35]);}assert_eq!(CALLS.load(Ordering::SeqCst),0);assert_eq!(DIRECT.load(Ordering::SeqCst),1)}
#[test]fn hosted_failure_clears_partial_output(){reset(false);FAIL.store(1,Ordering::SeqCst);let mut b=[7;35];unsafe{assert_eq!(trueos_cabi_random_bytes_v1(b.as_mut_ptr(),b.len()),-5)}assert_eq!(b,[0;35]);assert_eq!(CALLS.load(Ordering::SeqCst),0)}
#[test]fn concurrent_hosted_workers_never_execute_vmcall(){reset(false);std::thread::scope(|s|{for _ in 0..16{s.spawn(||{let mut b=[7;65];unsafe{assert_eq!(trueos_cabi_random_bytes_v1(b.as_mut_ptr(),b.len()),0)}assert_eq!(b,[91;65]);});}});assert_eq!(CALLS.load(Ordering::SeqCst),0);assert_eq!(DIRECT.load(Ordering::SeqCst),16)}
'''

def main():
    source = (ROOT / "src/native_abi.rs").read_text()
    harness = HARNESS.replace("@FUNCTION@", function(source, "trueos_cabi_random_bytes_v1"))
    with tempfile.TemporaryDirectory(prefix="trueos-native-random-") as tmp:
        path = Path(tmp) / "random.rs"
        path.write_text(harness)
        for level in ["0", "3"]:
            binary = Path(tmp) / f"random-{level}"
            subprocess.run(["rustc", "--edition=2024", "--test", "-C", f"opt-level={level}", str(path), "-o", str(binary)], check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True)

if __name__ == "__main__":
    main()
