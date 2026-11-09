#!/usr/bin/env python3
"""Exercise production child pipes on Hull and authenticated native workers.

The CABI bodies, child broker and exit queue retention come from production.
Only execution identity, VM slots and the communication-page transport are
modeled. The actual std stdout provider is used for the worker result test.
"""

from pathlib import Path
import re
import subprocess
import tempfile
from test_thread_control_abi import function


ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
pub use std::{env, ffi, io, thread, time};
use std::{alloc::{GlobalAlloc,Layout,System}, cell::Cell, collections::VecDeque, ptr, sync::{Mutex as StdMutex, atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering}}};
mod sys {pub mod io {pub const DEFAULT_BUF_SIZE:usize=8192;}}
thread_local! {
    static HULL:Cell<Option<u8>>=const{Cell::new(None)};
    static OWNER:Cell<Option<u8>>=const{Cell::new(None)};
    static BROKER_ACTIVE:Cell<bool>=const{Cell::new(false)};
}
static VMCALLS:AtomicUsize=AtomicUsize::new(0);
static TRANSPORT_FAIL:AtomicBool=AtomicBool::new(false);
static SPAWN_FAIL:AtomicI32=AtomicI32::new(0);
static BROKER_ALLOCS:AtomicUsize=AtomicUsize::new(0);
static WRONG_HEAP_ALLOCS:AtomicUsize=AtomicUsize::new(0);
struct CheckedAllocator;
#[global_allocator]static ALLOCATOR:CheckedAllocator=CheckedAllocator;
unsafe impl GlobalAlloc for CheckedAllocator {
    unsafe fn alloc(&self,layout:Layout)->*mut u8{
        if BROKER_ACTIVE.try_with(Cell::get).unwrap_or(false){
            BROKER_ALLOCS.fetch_add(1,Ordering::SeqCst);
            if !allocators::host_active(){WRONG_HEAP_ALLOCS.fetch_add(1,Ordering::SeqCst);}
        }
        unsafe{System.alloc(layout)}
    }
    unsafe fn dealloc(&self,p:*mut u8,layout:Layout){unsafe{System.dealloc(p,layout)}}
}
mod allocators {
    use super::*;
    static HOST_ALLOC_DOMAIN_STRONG_DEPTH_BY_CPU:[AtomicUsize;1]=[AtomicUsize::new(0)];
    fn cpuid_slot()->Option<usize>{Some(0)}
    @HOST_SCOPE@
    pub fn host_active()->bool{HOST_ALLOC_DOMAIN_STRONG_DEPTH_BY_CPU[0].load(Ordering::Acquire)>0}
}
fn broker_scope<T>(vm:u8,f:impl FnOnce()->T)->T{
    assert_eq!(OWNER.with(Cell::get),Some(vm),"broker actor changed");
    assert!(allocators::host_active(),"child broker must own allocations in the host domain");
    let old=BROKER_ACTIVE.with(|v|v.replace(true));let out=f();BROKER_ACTIVE.with(|v|v.set(old));out
}
#[unsafe(no_mangle)]pub extern "C" fn trueos_cabi_write(_:u32,_:*const u8,_:usize){panic!("worker stdout escaped child pipe")}
struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self{Self(StdMutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}
}
mod r {pub mod services {
    pub mod ide_workspace_service {pub fn release_vm(_:u8,_:u64){}}
    pub mod ide_source_service {pub fn release_vm(_:u8,_:u64){}}
}}
mod hv {
use super::*;
type AllocVec<T> = Vec<T>;
const BLUEPRINT_CHILD_ERR_INVALID:i32=-1;
const BLUEPRINT_CHILD_ERR_NOT_FOUND:i32=-2;
const BLUEPRINT_CHILD_ERR_QUEUE_FULL:i32=-3;
const BLUEPRINT_CHILD_ERR_UNAVAILABLE:i32=-4;
const BLUEPRINT_CHILD_MESSAGE_LIMIT:usize=crate::trueos_vm::vmcall::PAYLOAD_CAP;
const BLUEPRINT_CHILD_QUEUE_LIMIT:usize=16;
const BLUEPRINT_CHILD_STATE_STARTING:u8=1;
const BLUEPRINT_CHILD_STATE_RUNNING:u8=2;
const BLUEPRINT_CHILD_STATE_STOPPING:u8=3;
const BLUEPRINT_CHILD_STATE_EXITED:u8=4;
struct BlueprintChildLink {
    handle:u64,parent_vm_id:u8,parent_generation:u64,child_generation:u64,
    child_lifecycle_generation:u64,state:u8,
    parent_to_child:VecDeque<Vec<u8>>,child_to_parent:VecDeque<Vec<u8>>,
}
static BLUEPRINT_CHILD_LINKS:[Mutex<Option<BlueprintChildLink>>;4]=[const{Mutex::new(None)};4];
static GENERATIONS:Mutex<[u64;4]>=Mutex::new([0;4]);
static SPAWNS:Mutex<Vec<(u8,u8,Vec<u8>)>>=Mutex::new(Vec::new());
static STOPS:Mutex<Vec<(u8,Option<u64>)>>=Mutex::new(Vec::new());
struct Vm {running:AtomicBool,starting:AtomicBool}
static VMS:[Vm;4]=[const{Vm{running:AtomicBool::new(false),starting:AtomicBool::new(false)}};4];
fn vm_slot(vm:u8)->Option<&'static Vm>{VMS.get(vm as usize)}
fn vm_run_generation(vm:u8)->Option<u64>{GENERATIONS.lock().get(vm as usize).copied()}
fn clear_blueprint_child_template(_:u8){}
fn stop_matching_generation(vm:u8,generation:Option<u64>)->Result<bool,()>{STOPS.lock().push((vm,generation));Ok(true)}
// The launch admission and archive parser are outside this pipe proof. Capture
// their authenticated inputs and fault-inject the returned admission error.
pub fn blueprint_child_spawn(vm:u8,data:&[u8])->Result<u64,i32>{
    broker_scope(vm,||{
        SPAWNS.lock().push((vm,0,data.to_vec()));
        match SPAWN_FAIL.load(Ordering::SeqCst){0=>Ok(77),error=>Err(error)}
    })
}
pub fn blueprint_child_spawn_program(vm:u8,data:&[u8])->Result<u64,i32>{
    broker_scope(vm,||{
        SPAWNS.lock().push((vm,1,data.to_vec()));
        match SPAWN_FAIL.load(Ordering::SeqCst){0=>Ok(78),error=>Err(error)}
    })
}
pub fn current_hull_guest_context_vm_id()->Option<u8>{HULL.with(Cell::get)}
pub fn current_guest_execution_context_vm_id()->Option<u8>{OWNER.with(Cell::get)}
@BROKER@
pub fn blueprint_child_send(vm:u8,handle:u64,data:&[u8])->Result<usize,i32>{broker_scope(vm,||production_blueprint_child_send(vm,handle,data))}
pub fn blueprint_child_receive(vm:u8,handle:u64,out:&mut[u8])->Result<usize,i32>{broker_scope(vm,||production_blueprint_child_receive(vm,handle,out))}
pub fn blueprint_child_receive_stream(vm:u8,handle:u64,out:&mut[u8])->Result<usize,i32>{broker_scope(vm,||production_blueprint_child_receive_stream(vm,handle,out))}
pub fn blueprint_child_status(vm:u8,handle:u64)->Result<u8,i32>{broker_scope(vm,||production_blueprint_child_status(vm,handle))}
pub fn blueprint_child_terminate(vm:u8,handle:u64)->Result<(),i32>{broker_scope(vm,||production_blueprint_child_terminate(vm,handle))}
pub fn setup(){
    SPAWNS.lock().clear();STOPS.lock().clear();SPAWN_FAIL.store(0,Ordering::SeqCst);
    *GENERATIONS.lock()=[0,10,20,30];
    for slot in &BLUEPRINT_CHILD_LINKS{*slot.lock()=None;}
    for vm in &VMS{vm.running.store(true,Ordering::SeqCst);vm.starting.store(false,Ordering::SeqCst);}
    *BLUEPRINT_CHILD_LINKS[2].lock()=Some(BlueprintChildLink{
        handle:42,parent_vm_id:1,parent_generation:10,child_generation:20,
        child_lifecycle_generation:20,state:BLUEPRINT_CHILD_STATE_RUNNING,
        parent_to_child:VecDeque::new(),child_to_parent:VecDeque::new(),
    });
}
pub fn exit_child(){blueprint_child_lifecycle_cleanup(2,20,false)}
pub fn reuse(vm:u8){GENERATIONS.lock()[vm as usize]+=1}
pub fn launches()->Vec<(u8,u8,Vec<u8>)>{SPAWNS.lock().clone()}
pub fn stops()->Vec<(u8,Option<u64>)>{STOPS.lock().clone()}
}
mod trueos_vm {pub mod vmcall {
use super::super::*;
@CONSTANTS@
pub const STATUS_OK:u32=0;
pub const OP_BP_CHILD_SEND_V1:u32=1;
pub const OP_BP_CHILD_RECEIVE_V1:u32=2;
pub const OP_BP_CHILD_STATUS_V1:u32=3;
pub const OP_BP_CHILD_SPAWN_V1:u32=4;
pub const OP_BP_CHILD_TERMINATE_V1:u32=5;
pub fn call_with_payload(op:u32,handle:u64,arg:u64,data:&[u8],out:&mut[u8])->(u32,u64){
    let vm=HULL.with(Cell::get).expect("raw VMCALL on native worker");
    VMCALLS.fetch_add(1,Ordering::SeqCst);
    if TRANSPORT_FAIL.load(Ordering::SeqCst){return(1,0)}
    if op==OP_BP_CHILD_SPAWN_V1{
        assert_eq!(arg,0);assert!(out.is_empty());
        let result=allocators::with_host_alloc_domain_strong(||match handle{0=>hv::blueprint_child_spawn(vm,data),1=>hv::blueprint_child_spawn_program(vm,data),_=>panic!("spawn variant")});
        return(STATUS_OK,result.unwrap_or_else(|error|(error as i64)as u64))
    }
    let result=allocators::with_host_alloc_domain_strong(||match op {
        OP_BP_CHILD_SEND_V1=>{assert_eq!(arg,0);hv::blueprint_child_send(vm,handle,data)},
        OP_BP_CHILD_RECEIVE_V1=>{
            assert!(data.is_empty());
            if arg==0{hv::blueprint_child_receive(vm,handle,out)}
            else{assert_eq!(arg as usize,out.len());hv::blueprint_child_receive_stream(vm,handle,out)}
        },_=>panic!("unexpected operation"),
    });
    (STATUS_OK,result.map(|n|n as i64).unwrap_or_else(i64::from) as u64)
}
pub fn call(op:u32,handle:u64,arg:u64)->(u32,u64){
    let vm=HULL.with(Cell::get).expect("raw VMCALL on native worker");
    VMCALLS.fetch_add(1,Ordering::SeqCst);
    assert_eq!(arg,0);
    if TRANSPORT_FAIL.load(Ordering::SeqCst){return(1,0)}
    let result=allocators::with_host_alloc_domain_strong(||match op{
        OP_BP_CHILD_STATUS_V1=>hv::blueprint_child_status(vm,handle).map(i64::from),
        OP_BP_CHILD_TERMINATE_V1=>hv::blueprint_child_terminate(vm,handle).map(|()|0),
        _=>panic!("unexpected control operation"),
    });
    (STATUS_OK,result.unwrap_or_else(i64::from)as u64)
}
}}
@CABI@
mod native_stdio {@STDIO@}
fn context(hull:bool,owner:Option<u8>){
    HULL.with(|v|v.set(if hull{owner}else{None}));OWNER.with(|v|v.set(owner));
}
fn setup(){
    hv::setup();context(false,Some(2));VMCALLS.store(0,Ordering::SeqCst);TRANSPORT_FAIL.store(false,Ordering::SeqCst);
    BROKER_ALLOCS.store(0,Ordering::SeqCst);WRONG_HEAP_ALLOCS.store(0,Ordering::SeqCst);
    assert!(!allocators::host_active());
}
fn send(handle:u64,bytes:&[u8])->isize{unsafe{trueos_cabi_blueprint_child_send_v1(handle,bytes.as_ptr(),bytes.len())}}
fn read(handle:u64,out:&mut[u8])->isize{unsafe{trueos_cabi_blueprint_child_read_v1(handle,out.as_mut_ptr(),out.len())}}
#[test]fn actual_std_worker_result_is_drainable_after_fast_exit(){
    setup();unsafe{env::set_var("TRUEOS_MCP_STDIO","1")};
    thread::spawn(||{
        context(false,Some(2));
        use io::Write;
        let mut stdout=native_stdio::Stdout::new();
        stdout.write_all(b"{\"dry_run\":true}\n").unwrap();stdout.flush().unwrap();
    }).join().unwrap();
    hv::exit_child();context(false,Some(1));assert_eq!(trueos_cabi_blueprint_child_status_v1(42),4);
    let mut output=Vec::new();let mut part=[0;5];loop{let n=read(42,&mut part);assert!(n>=0);if n==0{break}output.extend_from_slice(&part[..n as usize]);}
    assert_eq!(output,b"{\"dry_run\":true}\n");assert_eq!(VMCALLS.load(Ordering::SeqCst),0);
    unsafe{env::remove_var("TRUEOS_MCP_STDIO")};
}
#[test]fn worker_parent_and_child_use_owned_bidirectional_pipe(){
    setup();context(false,Some(1));assert_eq!(send(42,b"input"),5);
    context(false,Some(2));let mut out=[0;16];assert_eq!(read(0,&mut out),5);assert_eq!(&out[..5],b"input");
    assert_eq!(trueos_cabi_blueprint_child_status_v1(0),2);assert_eq!(send(0,b"result"),6);
    context(false,Some(1));assert_eq!(read(42,&mut out[..3]),3);assert_eq!(&out[..3],b"res");
    assert_eq!(read(42,&mut out),3);assert_eq!(&out[..3],b"ult");assert_eq!(read(42,&mut out),0);
    assert_eq!(VMCALLS.load(Ordering::SeqCst),0);
}
#[test]fn unauthorized_and_stale_identities_cannot_touch_another_pipe(){
    setup();let mut out=[9;16];context(false,None);
    assert_eq!(send(0,b"x"),-1);assert_eq!(read(0,&mut out),-1);assert_eq!(trueos_cabi_blueprint_child_status_v1(0),-1);
    assert_eq!(unsafe{trueos_cabi_blueprint_child_receive_v1(0,out.as_mut_ptr(),out.len())},-1);
    context(false,Some(3));assert_eq!(send(42,b"x"),-2);assert_eq!(read(42,&mut out),-2);assert_eq!(trueos_cabi_blueprint_child_status_v1(42),-2);
    context(false,Some(1));hv::reuse(1);assert_eq!(send(42,b"x"),-2);assert_eq!(read(42,&mut out),-2);
    context(false,Some(2));hv::reuse(2);assert_eq!(send(0,b"x"),-2);assert_eq!(trueos_cabi_blueprint_child_status_v1(0),-2);
    assert_eq!(out,[9;16]);assert_eq!(VMCALLS.load(Ordering::SeqCst),0);
}
#[test]fn bounds_backpressure_and_closed_output_preserve_broker_errors(){
    setup();let mut out=[0;1];unsafe{
        assert_eq!(trueos_cabi_blueprint_child_send_v1(0,ptr::null(),1),-1);
        assert_eq!(trueos_cabi_blueprint_child_send_v1(0,ptr::dangling(),trueos_vm::vmcall::PAYLOAD_CAP+1),-1);
        assert_eq!(trueos_cabi_blueprint_child_read_v1(0,ptr::null_mut(),1),-1);
        assert_eq!(trueos_cabi_blueprint_child_read_v1(0,out.as_mut_ptr(),0),-1);
    }
    for _ in 0..16{assert_eq!(send(0,b"x"),1)}assert_eq!(send(0,b"x"),-3);
    hv::exit_child();assert_eq!(send(0,b"x"),-4);assert_eq!(VMCALLS.load(Ordering::SeqCst),0);
}
#[test]fn full_message_receive_retains_existing_size_and_copy_contract(){
    for guest in [false,true]{
        setup();context(guest,Some(2));assert_eq!(send(0,b"message"),7);
        context(guest,Some(1));let mut out=[9;16];assert_eq!(unsafe{trueos_cabi_blueprint_child_receive_v1(42,out.as_mut_ptr(),3)},7);
        assert_eq!(out,[9;16]);assert_eq!(read(42,&mut out),0);
        context(guest,Some(2));assert_eq!(send(0,b"again"),5);context(guest,Some(1));
        assert_eq!(unsafe{trueos_cabi_blueprint_child_receive_v1(42,out.as_mut_ptr(),out.len())},5);assert_eq!(&out[..5],b"again");
    }
}
#[test]fn hull_transport_remains_distinct_and_propagates_failures(){
    setup();context(true,Some(2));assert_eq!(send(0,b"guest"),5);
    context(true,Some(1));let mut out=[0;8];assert_eq!(read(42,&mut out),5);assert_eq!(&out[..5],b"guest");assert_eq!(trueos_cabi_blueprint_child_status_v1(42),2);
    assert_eq!(VMCALLS.load(Ordering::SeqCst),3);
    TRANSPORT_FAIL.store(true,Ordering::SeqCst);
    assert_eq!(send(42,b"x"),-1);assert_eq!(read(42,&mut out),-1);assert_eq!(trueos_cabi_blueprint_child_status_v1(42),-1);
    assert_eq!(unsafe{trueos_cabi_blueprint_child_receive_v1(42,out.as_mut_ptr(),out.len())},-1);
}
#[test]fn worker_spawn_passes_owned_identity_and_preserves_admission_result(){
    for guest in [false,true]{
        setup();context(guest,Some(2));let request=b"{\"program\":\"codex\",\"args\":[\"app-server\"]}";let mut handle=999;
        unsafe{
            assert_eq!(trueos_cabi_blueprint_child_spawn_v1(b"initial".as_ptr(),7,&mut handle),0);assert_eq!(handle,77);
            assert_eq!(trueos_cabi_blueprint_child_spawn_program_v1(request.as_ptr(),request.len(),&mut handle),0);assert_eq!(handle,78);
        }
        assert_eq!(hv::launches(),vec![(2,0,b"initial".to_vec()),(2,1,request.to_vec())]);
        SPAWN_FAIL.store(-4,Ordering::SeqCst);handle=999;unsafe{
            assert_eq!(trueos_cabi_blueprint_child_spawn_v1(b"initial".as_ptr(),7,&mut handle),-1);
            assert_eq!(trueos_cabi_blueprint_child_spawn_program_v1(request.as_ptr(),request.len(),&mut handle),-4);
        }assert_eq!(handle,999);if !guest{assert_eq!(VMCALLS.load(Ordering::SeqCst),0)}
    }
    setup();context(false,None);let mut handle=999;unsafe{
        assert_eq!(trueos_cabi_blueprint_child_spawn_v1(ptr::null(),0,&mut handle),-1);
        assert_eq!(trueos_cabi_blueprint_child_spawn_program_v1(ptr::dangling(),0,&mut handle),-1);
    }assert!(hv::launches().is_empty());assert_eq!(handle,999);
}
#[test]fn worker_termination_uses_same_parent_and_generation_checks(){
    for guest in [false,true]{
        setup();context(guest,Some(3));assert_eq!(trueos_cabi_blueprint_child_terminate_v1(42),-2);
        context(guest,Some(2));assert_eq!(trueos_cabi_blueprint_child_terminate_v1(0),-1);assert!(hv::stops().is_empty());
        context(guest,Some(1));assert_eq!(trueos_cabi_blueprint_child_terminate_v1(42),0);
        assert_eq!(hv::stops(),vec![(2,Some(20))]);assert_eq!(trueos_cabi_blueprint_child_status_v1(42),3);
        hv::exit_child();assert_eq!(trueos_cabi_blueprint_child_terminate_v1(42),0);assert_eq!(hv::stops().len(),1);
        if !guest{assert_eq!(VMCALLS.load(Ordering::SeqCst),0)}
    }
    setup();context(false,None);assert_eq!(trueos_cabi_blueprint_child_terminate_v1(42),-1);assert!(hv::stops().is_empty());
}
#[test]fn native_broker_allocations_are_host_owned_and_scope_is_restored(){
    setup();let mut handle=999;let mut out=[0;16];
    assert_eq!(send(0,b"retained result"),15);assert!(!allocators::host_active());
    unsafe{
        assert_eq!(trueos_cabi_blueprint_child_spawn_v1(b"initial".as_ptr(),7,&mut handle),0);assert!(!allocators::host_active());
        assert_eq!(trueos_cabi_blueprint_child_spawn_program_v1(b"request".as_ptr(),7,&mut handle),0);assert!(!allocators::host_active());
    }
    SPAWN_FAIL.store(-4,Ordering::SeqCst);unsafe{
        assert_eq!(trueos_cabi_blueprint_child_spawn_v1(b"initial".as_ptr(),7,&mut handle),-1);assert!(!allocators::host_active());
        assert_eq!(trueos_cabi_blueprint_child_spawn_program_v1(b"request".as_ptr(),7,&mut handle),-4);assert!(!allocators::host_active());
    }
    assert_eq!(OWNER.with(Cell::get),Some(2));context(false,Some(1));
    assert_eq!(trueos_cabi_blueprint_child_status_v1(42),2);assert!(!allocators::host_active());
    assert_eq!(read(42,&mut out[..4]),4);assert!(!allocators::host_active());
    assert_eq!(unsafe{trueos_cabi_blueprint_child_receive_v1(42,out.as_mut_ptr(),out.len())},11);assert!(!allocators::host_active());
    assert_eq!(trueos_cabi_blueprint_child_terminate_v1(42),0);assert!(!allocators::host_active());
    context(false,Some(3));assert_eq!(trueos_cabi_blueprint_child_terminate_v1(42),-2);assert!(!allocators::host_active());
    assert_eq!(OWNER.with(Cell::get),Some(3));assert!(BROKER_ALLOCS.load(Ordering::SeqCst)>0);
    assert_eq!(WRONG_HEAP_ALLOCS.load(Ordering::SeqCst),0);assert_eq!(VMCALLS.load(Ordering::SeqCst),0);
}
'''


def main():
    cabi = (ROOT / "src/r/io/fs_cabi.rs").read_text()
    broker = (ROOT / "src/hv/mod.rs").read_text()
    transport = (ROOT / "crates/trueos-vm/src/vmcall.rs").read_text()
    names = ["blueprint_child_actor_is_parent", "blueprint_child_find_link_for_actor",
             "blueprint_child_send", "blueprint_child_receive", "blueprint_child_receive_stream",
             "blueprint_child_status", "blueprint_child_terminate", "blueprint_child_lifecycle_cleanup"]
    bodies = []
    observed = {"blueprint_child_send", "blueprint_child_receive", "blueprint_child_receive_stream",
                "blueprint_child_status", "blueprint_child_terminate"}
    for name in names:
        body = function(broker, name)
        if name in observed:
            body = body.replace(f"fn {name}(", f"fn production_{name}(", 1)
        bodies.append(body)
    source = HARNESS.replace("@BROKER@", "\n".join(bodies))
    allocators = (ROOT / "src/allocators.rs").read_text()
    source = source.replace("@HOST_SCOPE@", function(allocators, "with_host_alloc_domain_strong"))
    names = ["trueos_cabi_blueprint_child_read_v1", "trueos_cabi_blueprint_child_send_v1",
             "trueos_cabi_blueprint_child_receive_v1", "trueos_cabi_blueprint_child_status_v1",
             "trueos_cabi_blueprint_child_spawn_v1", "trueos_cabi_blueprint_child_spawn_program_v1",
             "trueos_cabi_blueprint_child_terminate_v1"]
    functions = ["#[unsafe(no_mangle)]\n" + function(cabi, name) for name in names]
    functions.extend(function(cabi, name) for name in ["vmcall_signed", "vmcall_signed_i32"])
    source = source.replace("@CABI@", "\n".join(functions))
    source = source.replace("@STDIO@", (ROOT / "tools/rust-std/trueos_stdio.rs").read_text())
    constants = [re.search(rf"^pub const {name}:.*$", transport, re.MULTILINE).group(0)
                 for name in ["COMM_PAGE_BYTES", "PAYLOAD_CAP"]]
    source = source.replace("@CONSTANTS@", "\n".join(constants))
    with tempfile.TemporaryDirectory(prefix="trueos-native-child-stdio-") as temporary:
        directory = Path(temporary)
        fixture = directory / "child_stdio.rs"
        fixture.write_text(source)
        for level in ["0", "3"]:
            executable = directory / ("debug" if level == "0" else "optimized")
            subprocess.run(["rustc", "--edition=2024", "--test", "-C", f"opt-level={level}",
                            str(fixture), "-o", str(executable)], check=True)
            subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
