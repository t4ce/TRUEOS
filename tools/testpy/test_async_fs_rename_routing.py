#!/usr/bin/env python3
"""Execute production Hull rename wire decoding, queueing and async worker routing.

Mounted namespace mutation is a deterministic boundary; its real implementation
and durable replay are covered by test_indexed_fs_move.py and trueos-fs tests.
"""
from pathlib import Path
import subprocess
import tempfile
from test_indexed_fs_remove import extract

ROOT = Path(__file__).resolve().parents[2]


def block(text, marker):
    start = text.index(marker)
    opening = text.index('{', start)
    depth, end = 1, opening + 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    return text[opening + 1:end - 1]


def main():
    source = (ROOT / 'src/r/io/async_fs_cabi.rs').read_text()
    vmcall = (ROOT / 'src/hv/vmcall.rs').read_text()
    env = (ROOT / 'src/r/io.rs').read_text()
    path = (ROOT / 'src/r/path.rs').read_text()
    path = path[:path.rfind('#[derive', 0, path.index('pub struct StripPrefixError'))]
    funcs = ['next_operation_id', 'start', 'start_inner', 'start_rename',
             'start_rename_with_policy', 'owner_for_vm', 'take_request',
             'finish_request', 'status', 'discard', 'selected_disk', 'path_kind_async',
             'parse_path', 'guest_rename_start', 'rename_start_with_policy',
             'rename_replace_start', 'trueos_cabi_async_fs_rename_start']
    production = '\n'.join(extract(source, name) for name in funcs)
    # Extract the production rename arm of the worker, excluding unrelated I/O.
    worker_start = source.index('if let RequestKind::Rename {')
    statement = source[worker_start:source.index('    let path = match &request.kind', worker_start)]
    arm = block(vmcall, '        OP_BP_ASYNC_FS_RENAME_START => {')
    env_funcs = '\n'.join(extract(env, name) for name in ['normalize_app_path', 'resolve_fs_path'])
    volatile = (ROOT / 'src/r/fs/volatile_files.rs').read_text()
    selectors = '\n'.join(extract(volatile, n) for n in ['contains', 'disk_for_path', 'selected_disk'])
    rust = HARNESS.replace('@CODES@', (ROOT / 'src/r/cabi_codes.rs').read_text()).replace('@PATH@', path).replace('@ENV@', env_funcs).replace('@SELECTORS@', selectors)
    production = '\n#[path="@ROOT@/src/r/io/async_fs_progress.rs"]mod progress;\nstatic PROGRESS:progress::Tracker=progress::Tracker::new();\nfn progress_now_ms()->u64{100}\nfn trace_slow_progress(_:progress::Snapshot){}\nfn lock_operations()->std::sync::MutexGuard<\'static,BTreeMap<u32,Operation>>{ASYNC_FS_OPERATIONS.lock()}\n'.replace('@ROOT@',str(ROOT)) + production
    rust = rust.replace('@PRODUCTION@', production).replace('@WORKER@', statement).replace('@ARM@', arm).replace('@MOUNT_POLICY@', extract(vmcall, 'vm_mount_selector_allowed'))
    with tempfile.TemporaryDirectory(prefix='async-fs-rename-routing-') as directory:
        test = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        test.write_text(rust + TESTS)
        subprocess.run(['rustc', '--edition=2024', '--test', '-Awarnings', str(test), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::collections::{BTreeMap,VecDeque};
use std::sync::atomic::{AtomicU32,AtomicBool,Ordering};
use std::task::{Context,Poll,Waker};
use std::future::Future;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T>{const fn new(t:T)->Self{Self(std::sync::Mutex::new(t))}fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}
@CODES@
static GUEST:AtomicBool=AtomicBool::new(true);
mod hv{pub fn current_hull_guest_context_vm_id()->Option<u8>{crate::GUEST.load(crate::Ordering::Relaxed).then_some(7)}}
mod allocators{pub fn with_host_alloc_domain_strong<T>(f:impl FnOnce()->T)->T{f()}}
mod disc{pub mod block{
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]pub struct DeviceHandle(pub u32);pub type DiscId=DeviceHandle;
impl DeviceHandle{pub fn id(self)->Self{self}pub fn from_raw(n:u32)->Self{Self(n)}}
#[derive(Debug)]pub struct Error;
pub fn device_handle(d:DiscId)->Option<DeviceHandle>{Some(d)}
}}
#[derive(Clone,Copy,Debug,PartialEq)]struct Node{file:bool,identity:u32}
#[derive(Default)]struct State{nodes:BTreeMap<(u32,String),Node>,calls:Vec<(u32,bool)>,fail:bool}
static STATE:Mutex<State>=Mutex::new(State{nodes:BTreeMap::new(),calls:Vec::new(),fail:false});
mod r{
pub mod path{@PATH@}
pub mod io{
pub mod cabi{pub(crate) use crate::{FS_ERR_BAD_PARAM,FS_ERR_BAD_UTF8,FS_ERR_BAD_PATH};}
pub mod env{
fn trueosfs_scope_granted()->bool{false}
fn current_app_fs_root()->Option<String>{Some("apps/codex".into())}
@ENV@
}
pub mod async_fs_cabi{
use crate::*;
const ASYNC_FS_MAX_OPERATIONS:usize=64;
#[derive(Debug)]enum RequestKind{Rename{source:String,destination:String,replace_file:bool}}
impl RequestKind{fn progress_class(&self)->u32{14}}
struct Request{id:u32,owner:u32,kind:RequestKind}
#[derive(Debug)]enum OperationState{Pending,Upload{bytes:Vec<u8>},Read(Vec<u8>),Unit,Failed(i32)}
struct Operation{owner:u32,state:OperationState}
static ASYNC_FS_SEQUENCE:AtomicU32=AtomicU32::new(1);
static ASYNC_FS_REQUESTS:Mutex<VecDeque<Request>>=Mutex::new(VecDeque::new());
static ASYNC_FS_ACTIVE_REQUEST:Mutex<Option<(u32,u32)>>=Mutex::new(None);
static ASYNC_FS_OPERATIONS:Mutex<BTreeMap<u32,Operation>>=Mutex::new(BTreeMap::new());
fn direct_owner()->u32{77}fn is_virtual_write(_: &str)->bool{false}
fn map_block_error(_:disc::block::Error)->i32{FS_ERR_IO}
@PRODUCTION@
async fn process_rename(request:&Request)->OperationState{@WORKER@unreachable!()}
pub fn reset(){ASYNC_FS_REQUESTS.lock().clear();ASYNC_FS_OPERATIONS.lock().clear();*ASYNC_FS_ACTIVE_REQUEST.lock()=None;}
pub fn submit(replace:bool,src:&str,dst:&str)->i32{unsafe{if replace{rename_replace_start(src.as_ptr(),src.len(),dst.as_ptr(),dst.len())}else{trueos_cabi_async_fs_rename_start(src.as_ptr(),src.len(),dst.as_ptr(),dst.len())}}}
pub fn queued()->usize{ASYNC_FS_REQUESTS.lock().len()}
pub fn queued_owner()->u32{ASYNC_FS_REQUESTS.lock().front().unwrap().owner}
pub fn run_one(){let request=take_request().unwrap();let mut f=Box::pin(process_rename(&request));let state=match f.as_mut().poll(&mut Context::from_waker(Waker::noop())){Poll::Ready(s)=>s,_=>panic!("unexpected pending")};drop(f);finish_request(&request,Some(state));}
pub fn check(owner:u32,id:i32)->i32{status(owner,id as u32)}
}}
pub mod fs{
pub mod volatile_files{
use crate::{Mutex,disc::block};use super::trueosfs as fs;
#[derive(Clone,Copy)]struct Storage{disk:block::DeviceHandle}
static ROOTS:Mutex<Vec<String>>=Mutex::new(Vec::new());static STORAGE:Mutex<Option<Storage>>=Mutex::new(Some(Storage{disk:block::DeviceHandle(2)}));
@SELECTORS@
pub fn setup(){*ROOTS.lock()=vec!["apps/codex".into()];}
}
pub mod trueosfs{
use crate::*;use disc::block::{DeviceHandle as D,Error};
#[derive(Clone,Copy)]pub enum NodeKind{File,Directory}pub struct NodeInfo{pub kind:NodeKind}
pub struct Root{pub disk_id:D}pub fn list_roots()->Vec<Root>{vec![Root{disk_id:D(1)},Root{disk_id:D(2)}]}
pub fn primary_root_handle()->Option<D>{Some(D(1))}
pub async fn node_info_async(d:D,p:&str)->Result<Option<NodeInfo>,Error>{Ok(STATE.lock().nodes.get(&(d.0,p.into())).map(|n|NodeInfo{kind:if n.file{NodeKind::File}else{NodeKind::Directory}}))}
async fn rename(d:D,s:&str,t:&str,replace:bool)->Result<bool,Error>{let mut state=STATE.lock();state.calls.push((d.0,replace));if state.fail{return Err(Error)}let Some(node)=state.nodes.get(&(d.0,s.into())).copied()else{return Ok(false)};if !replace&&state.nodes.contains_key(&(d.0,t.into())){return Ok(false)}state.nodes.remove(&(d.0,s.into()));state.nodes.insert((d.0,t.into()),node);Ok(true)}
pub async fn file_rename_async(d:D,s:&str,t:&str)->Result<bool,Error>{rename(d,s,t,false).await}
pub async fn file_rename_replace_async(d:D,s:&str,t:&str)->Result<bool,Error>{rename(d,s,t,true).await}
pub async fn dir_rename_async(d:D,s:&str,t:&str)->Result<bool,Error>{rename(d,s,t,false).await}
}}}
mod trueos_vm{pub mod vmcall{
pub const OP_BP_ASYNC_FS_RENAME_START:u32=0x13a;pub const PAYLOAD_CAP:usize=4096;pub const STATUS_OK:u32=0;
pub fn call_with_payload(op:u32,a:u64,b:u64,p:&[u8],_:&mut[u8])->(u32,u64){assert_eq!(op,OP_BP_ASYNC_FS_RENAME_START);crate::wire(a,b,p)}
}}
const PAYLOAD_CAP:usize=4096;const STATUS_OK:u32=0;const STATUS_BAD_ARG:u32=1;
#[derive(Clone,Copy)]struct Page{payload:[u8;PAYLOAD_CAP]}
static mut PAGE:Page=Page{payload:[0;PAYLOAD_CAP]};
static RESPONSE:Mutex<(u32,u64)>=Mutex::new((0,0));
fn host_ptr(_:u8)->Option<*mut Page>{Some(&raw mut PAGE)}
fn write_response(vm:u8,_:u32,s:u32,value:u64,_:usize){assert_eq!(vm,7);*RESPONSE.lock()=(s,value);}
fn vm_has_trueosfs_scope(_:u8)->bool{false}
@MOUNT_POLICY@
enum DispatchOutcome{Resume}
fn dispatch(vm_id:u8,seq:u32,arg0:u64,arg1:u64,req_len:u32)->DispatchOutcome{@ARM@}
fn wire(a:u64,b:u64,p:&[u8])->(u32,u64){unsafe{(&mut (*(&raw mut PAGE)).payload)[..p.len()].copy_from_slice(p)};dispatch(7,1,a,b,p.len()as u32);*RESPONSE.lock()}
fn setup(){GUEST.store(true,Ordering::Relaxed);r::io::async_fs_cabi::reset();r::fs::volatile_files::setup();let mut s=STATE.lock();*s=State::default();for disk in [1,2]{s.nodes.insert((disk,"apps/codex/.codex/temp".into()),Node{file:true,identity:10});s.nodes.insert((disk,"apps/codex/.codex/config.toml".into()),Node{file:true,identity:20});}}
'''
TESTS = r'''
use r::io::async_fs_cabi as api;
const OWNER:u32=0x80000007;
#[test]fn hull_replace_retains_owner_and_resolves_both_aliases_to_private_ram(){setup();let id=api::submit(true,"/apps/codex/.codex/temp",".codex/config.toml");assert!(id>0);assert_eq!(api::queued_owner(),OWNER);assert_eq!(api::check(77,id),FS_ERR_NOT_FOUND);assert_eq!(api::check(OWNER,id),0);api::run_one();assert_eq!(api::check(OWNER,id),1);let s=STATE.lock();assert_eq!(s.calls,vec![(2,true)]);assert!(!s.nodes.contains_key(&(2,"apps/codex/.codex/temp".into())));assert_eq!(s.nodes[&(2,"apps/codex/.codex/config.toml".into())].identity,10);assert_eq!(s.nodes[&(1,"apps/codex/.codex/config.toml".into())].identity,20);}
#[test]fn native_no_replace_keeps_existing_destination_and_sources(){setup();let before=STATE.lock().nodes.clone();let id=api::submit(false,".codex/temp",".codex/config.toml");api::run_one();assert_eq!(api::check(OWNER,id),FS_ERR_ALREADY_EXISTS);let s=STATE.lock();assert_eq!(s.nodes,before);assert!(s.calls.is_empty());}
#[test]fn native_move_flag_zero_reaches_nonreplace_worker(){setup();let id=api::submit(false,".codex/temp",".codex/new");api::run_one();assert_eq!(api::check(OWNER,id),1);assert_eq!(STATE.lock().calls,vec![(2,false)]);}
#[test]fn failed_replacement_preserves_namespace_and_owner_result(){setup();let before=STATE.lock().nodes.clone();STATE.lock().fail=true;let id=api::submit(true,".codex/temp",".codex/config.toml");api::run_one();assert_eq!(api::check(OWNER,id),FS_ERR_IO);assert_eq!(STATE.lock().nodes,before);}
#[test]fn directory_destination_rejected_without_mutation(){setup();STATE.lock().nodes.get_mut(&(2,"apps/codex/.codex/config.toml".into())).unwrap().file=false;let before=STATE.lock().nodes.clone();let id=api::submit(true,".codex/temp",".codex/config.toml");api::run_one();assert_eq!(api::check(OWNER,id),FS_ERR_ALREADY_EXISTS);let s=STATE.lock();assert_eq!(s.nodes,before);assert!(s.calls.is_empty());}
#[test]fn directory_source_cannot_replace_file(){setup();STATE.lock().nodes.get_mut(&(2,"apps/codex/.codex/temp".into())).unwrap().file=false;let before=STATE.lock().nodes.clone();let id=api::submit(true,".codex/temp",".codex/config.toml");api::run_one();assert_eq!(api::check(OWNER,id),FS_ERR_ALREADY_EXISTS);let s=STATE.lock();assert_eq!(s.nodes,before);assert!(s.calls.is_empty());}
#[test]fn malformed_or_unauthorized_hull_paths_never_enqueue(){setup();for payload in [vec![],vec![1,0,0,0],vec![0,0,0,0,b'a'],vec![1,0,0,0,0xff,b'a']]{let(_,rc)=wire(1,0,&payload);assert!((rc as i64 as i32)<0);assert_eq!(api::queued(),0);}for src in ["../escape","trueosfs:disc1/private"]{let mut p=(src.len()as u32).to_le_bytes().to_vec();p.extend_from_slice(src.as_bytes());p.extend_from_slice(b"apps/codex/dst");let(_,rc)=wire(1,0,&p);assert_eq!(rc as i64 as i32,FS_ERR_BAD_PATH);assert_eq!(api::queued(),0);}}
#[test]fn mixed_private_ram_and_primary_disk_rejected(){setup();let before=STATE.lock().nodes.clone();let id=api::submit(true,".codex/temp","/common/new");api::run_one();assert_eq!(api::check(OWNER,id),FS_ERR_BAD_PATH);let s=STATE.lock();assert_eq!(s.nodes,before);assert!(s.calls.is_empty());}
#[test]fn invalid_hull_policy_flags_never_enqueue(){setup();for (a,b) in [(2,0),(u64::MAX,0),(1,1),(0,1)]{let (_,rc)=wire(a,b,b"\x01\0\0\0ab");assert_eq!(rc as i64 as i32,FS_ERR_BAD_PARAM);assert_eq!(api::queued(),0);}}
#[test]fn host_direct_path_keeps_direct_owner_and_replace_policy(){setup();GUEST.store(false,Ordering::Relaxed);let id=api::submit(true,".codex/temp",".codex/config.toml");assert_eq!(api::queued_owner(),77);api::run_one();assert_eq!(api::check(77,id),1);assert_eq!(STATE.lock().calls,vec![(2,true)]);}
'''
if __name__ == '__main__':
    main()
