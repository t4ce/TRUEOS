#!/usr/bin/env python3
"""Compile production checked-write ABI, Hull wire, upload and worker routing.

Storage is a deterministic boundary here. The actual under-lease filesystem
operation and race handling are exercised by test_atomic_typed_fs_write.py.
"""
from pathlib import Path
import subprocess
import tempfile

from test_indexed_fs_remove import extract
from test_async_fs_rename_routing import block

ROOT = Path(__file__).resolve().parents[2]


HARNESS = r'''
#![allow(dead_code,unused_imports)]
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
#[derive(Clone,Copy,Debug)]pub struct Error;
pub fn device_handle(d:DiscId)->Option<DeviceHandle>{Some(d)}
}}
#[derive(Clone,Debug,PartialEq)]struct Call{disk:u32,path:String,bytes:Vec<u8>,content_type:u32,create:bool,hash:Option<[u8;32]>}
#[derive(Default)]struct State{calls:Vec<Call>,outcome:u8}
static STATE:Mutex<State>=Mutex::new(State{calls:Vec::new(),outcome:0});
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
const ASYNC_FS_MAX_RESULT_BYTES:u64=16*1024*1024;
const ASYNC_FS_MAX_FILE_WRITE_BYTES:u64=4*1024*1024*1024;
@OPTIONS@
#[derive(Debug)]enum RequestKind{Write{path:String,bytes:Vec<u8>,content_type:infer::ContentTypeId,legacy_blob:bool,checked:Option<CheckedWriteOptions>}}
impl RequestKind{fn progress_class(&self)->u32{2}}
struct Request{id:u32,owner:u32,kind:RequestKind}
#[derive(Debug)]enum OperationState{Pending,Upload{path:String,bytes:Vec<u8>,total_len:usize,content_type:infer::ContentTypeId,legacy_blob:bool,checked:Option<CheckedWriteOptions>},Read(Vec<u8>),Unit,Failed(i32)}
struct Operation{owner:u32,state:OperationState}
static ASYNC_FS_SEQUENCE:AtomicU32=AtomicU32::new(1);
static ASYNC_FS_REQUESTS:Mutex<VecDeque<Request>>=Mutex::new(VecDeque::new());
static ASYNC_FS_ACTIVE_REQUEST:Mutex<Option<(u32,u32)>>=Mutex::new(None);
static ASYNC_FS_OPERATIONS:Mutex<BTreeMap<u32,Operation>>=Mutex::new(BTreeMap::new());
fn direct_owner()->u32{77}fn is_virtual_write(path: &str)->bool{path=="vFile:appdb-install"}
fn map_block_error(_:disc::block::Error)->i32{FS_ERR_IO}
@PRODUCTION@
async fn process_write(request:&Request)->OperationState{
let RequestKind::Write{path,bytes,content_type,legacy_blob:_,checked}=&request.kind;
let (disk,selected_path)=match selected_disk(path){Ok(v)=>v,Err(e)=>return OperationState::Failed(e)};
if let Some(options)=checked{@WORKER@}
unreachable!()
}
pub fn reset(){ASYNC_FS_REQUESTS.lock().clear();ASYNC_FS_OPERATIONS.lock().clear();*ASYNC_FS_ACTIVE_REQUEST.lock()=None;}
pub fn submit(path:&str,len:usize,content_type:u32,flags:u32,hash:Option<[u8;32]>)->i32{unsafe{trueos_cabi_async_fs_checked_write_begin_v1(path.as_ptr(),path.len(),len,content_type,flags,hash.as_ref().map_or(std::ptr::null(),|h|h.as_ptr()))}}
pub fn queued()->usize{ASYNC_FS_REQUESTS.lock().len()}
pub fn operations()->usize{ASYNC_FS_OPERATIONS.lock().len()}
pub fn queued_owner()->u32{ASYNC_FS_REQUESTS.lock().front().unwrap().owner}
pub fn chunk(owner:u32,id:i32,offset:usize,data:&[u8])->i32{write_chunk(owner,id as u32,offset,data)}
pub fn commit(owner:u32,id:i32)->i32{write_commit(owner,id as u32)}
pub fn check(owner:u32,id:i32)->i32{status(owner,id as u32)}
pub fn release(owner:u32,id:i32)->i32{discard(owner,id as u32)}
pub fn run_one(){let request=take_request().unwrap();let mut f=Box::pin(process_write(&request));let state=match f.as_mut().poll(&mut Context::from_waker(Waker::noop())){Poll::Ready(s)=>s,_=>panic!("unexpected pending")};drop(f);finish_request(&request,Some(state));}
}
}
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
pub use infer::ContentTypeId;
pub enum CheckedWriteError{AlreadyExists,Conflict,Device(Error)}
pub struct Root{pub disk_id:D}pub fn list_roots()->Vec<Root>{vec![Root{disk_id:D(1)},Root{disk_id:D(2)}]}
pub fn primary_root_handle()->Option<D>{Some(D(1))}
pub async fn file_in_typed_checked_async(d:D,p:&str,b:&[u8],t:ContentTypeId,c:bool,h:Option<[u8;32]>)->Result<bool,CheckedWriteError>{let mut s=STATE.lock();s.calls.push(Call{disk:d.0,path:p.into(),bytes:b.to_vec(),content_type:t.raw(),create:c,hash:h});match s.outcome{0=>Ok(true),1=>Ok(false),2=>Err(CheckedWriteError::AlreadyExists),3=>Err(CheckedWriteError::Conflict),_=>Err(CheckedWriteError::Device(Error))}}
}
}
}
mod trueos_vm{pub mod vmcall{
pub const OP_BP_ASYNC_FS_CHECKED_WRITE_BEGIN_V1:u32=0x21e;pub const PAYLOAD_CAP:usize=4096;pub const STATUS_OK:u32=0;
pub fn call_with_payload(op:u32,a:u64,b:u64,p:&[u8],_:&mut[u8])->(u32,u64){assert_eq!(op,OP_BP_ASYNC_FS_CHECKED_WRITE_BEGIN_V1);crate::wire(a,b,p)}
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
fn wire(a:u64,b:u64,p:&[u8])->(u32,u64){assert!(p.len()<=PAYLOAD_CAP);unsafe{(&mut (*(&raw mut PAGE)).payload)[..p.len()].copy_from_slice(p)};dispatch(7,1,a,b,p.len()as u32);*RESPONSE.lock()}
fn payload(flags:u32,hash:[u8;32],path:&[u8])->Vec<u8>{let mut p=flags.to_le_bytes().to_vec();p.extend_from_slice(&hash);p.extend_from_slice(path);p}
fn rc(r:(u32,u64))->i32{assert_eq!(r.0,STATUS_OK);r.1 as i64 as i32}
fn setup(){GUEST.store(true,Ordering::Relaxed);r::io::async_fs_cabi::reset();r::fs::volatile_files::setup();*STATE.lock()=State::default();}
'''

TESTS = r'''
use r::io::async_fs_cabi as api;
const OWNER:u32=0x80000007;
#[test]fn hull_create_preserves_bytes_policy_owner_and_private_ram(){setup();let id=api::submit("hello.rs",11,2,1,None);assert!(id>0);assert_eq!(api::queued(),0);assert_eq!(api::chunk(77,id,0,b"hello codex"),FS_ERR_NOT_FOUND);assert_eq!(api::chunk(OWNER,id,0,b"hello codex"),0);assert_eq!(api::commit(OWNER,id),0);assert_eq!(api::queued_owner(),OWNER);api::run_one();assert_eq!(api::check(OWNER,id),1);assert_eq!(STATE.lock().calls,vec![Call{disk:2,path:"apps/codex/hello.rs".into(),bytes:b"hello codex".to_vec(),content_type:2,create:true,hash:None}]);}
#[test]fn hash_replacement_round_trips_all_digest_bytes(){setup();let hash=std::array::from_fn(|i|i as u8);let id=api::submit("index.html",0,infer::ContentTypeId::HTML.raw(),2,Some(hash));assert!(id>0);assert_eq!(api::commit(OWNER,id),0);api::run_one();assert_eq!(api::check(OWNER,id),1);let s=STATE.lock();assert_eq!((s.calls[0].hash,s.calls[0].create,s.calls[0].content_type),(Some(hash),false,infer::ContentTypeId::HTML.raw()));}
#[test]fn unconditional_mode_and_host_owner_follow_same_worker(){setup();GUEST.store(false,Ordering::Relaxed);let id=api::submit("hello.txt",0,2,0,None);assert!(id>0);assert_eq!(api::commit(OWNER,id),FS_ERR_NOT_FOUND);assert_eq!(api::commit(77,id),0);assert_eq!(api::queued_owner(),77);api::run_one();assert_eq!(api::check(77,id),1);let s=STATE.lock();assert_eq!((s.calls[0].hash,s.calls[0].create),(None,false));}
#[test]fn incomplete_or_noncontiguous_upload_never_queues(){setup();let id=api::submit("hello.txt",3,2,1,None);assert!(id>0);assert_eq!(api::chunk(OWNER,id,1,b"a"),FS_ERR_BAD_PARAM);assert_eq!(api::chunk(OWNER,id,0,b"abcd"),FS_ERR_BAD_PARAM);assert_eq!(api::chunk(OWNER,id,0,b"a"),0);assert_eq!(api::commit(OWNER,id),FS_ERR_BAD_PARAM);assert_eq!(api::queued(),0);assert!(STATE.lock().calls.is_empty());assert_eq!(api::chunk(OWNER,id,1,b"bc"),0);assert_eq!(api::commit(OWNER,id),0);api::run_one();assert_eq!(api::check(OWNER,id),1);}
#[test]fn backend_outcomes_have_distinct_codes(){for(outcome,want)in[(0,1),(1,FS_ERR_NO_SPACE),(2,FS_ERR_ALREADY_EXISTS),(3,FS_ERR_WRITE_CONFLICT),(4,FS_ERR_IO)]{setup();STATE.lock().outcome=outcome;let id=api::submit("hello.txt",0,2,1,None);assert_eq!(api::commit(OWNER,id),0);api::run_one();assert_eq!(api::check(OWNER,id),want);}}
#[test]fn invalid_abi_flags_or_missing_hash_never_stage(){setup();for(flags,hash)in[(3,None),(4,None),(u32::MAX,None),(2,None)]{assert_eq!(api::submit("hello",0,2,flags,hash),FS_ERR_BAD_PARAM);assert_eq!(api::operations(),0);}}
#[test]fn malformed_wire_headers_never_stage(){setup();for p in [vec![],vec![0;35],vec![0;36],payload(3,[0;32],b"hello"),payload(0,[1;32],b"hello"),payload(1,[1;32],b"hello"),payload(0,[0;32],&vec![b'x';BLUEPRINT_ASYNC_FS_MAX_PATH+1])]{assert_eq!(rc(wire(0,2,&p)),FS_ERR_BAD_PARAM);assert_eq!(api::operations(),0);}assert_eq!(rc(wire(0,2,&payload(0,[0;32],b"\xff"))),FS_ERR_BAD_UTF8);assert_eq!(api::operations(),0);}
#[test]fn unauthorized_or_malformed_paths_never_stage(){setup();assert_eq!(api::submit("../escape",0,2,1,None),FS_ERR_BAD_PATH);for path in ["../escape","trueosfs:disc1/private"]{assert_eq!(rc(wire(0,2,&payload(1,[0;32],path.as_bytes()))),FS_ERR_BAD_PATH);assert_eq!(api::operations(),0);}}
#[test]fn unscoped_selector_input_is_resolved_inside_app_namespace(){setup();let id=api::submit("trueosfs:disc1/private",0,2,1,None);assert!(id>0);assert_eq!(api::commit(OWNER,id),0);api::run_one();let s=STATE.lock();assert_eq!((s.calls[0].disk,s.calls[0].path.as_str()),(2,"apps/codex/trueosfs:disc1/private"));}
#[test]fn invalid_type_and_wide_wire_type_never_stage(){setup();assert_eq!(api::submit("hello",0,0,1,None),FS_ERR_TYPE_REQUIRED);assert_eq!(api::submit("hello",0,0xffff_ffff,1,None),FS_ERR_BAD_PARAM);assert_eq!(rc(wire(0,1u64<<32,&payload(1,[0;32],b"hello"))),FS_ERR_BAD_PARAM);assert_eq!(api::operations(),0);}
#[test]fn oversize_wire_length_is_rejected_before_payload_slice(){setup();dispatch(7,1,0,2,(PAYLOAD_CAP+1)as u32);assert_eq!(rc(*RESPONSE.lock()),FS_ERR_BAD_PARAM);assert_eq!(api::operations(),0);}
#[test]fn owner_cannot_read_discard_or_commit_another_upload(){setup();let id=api::submit("hello",0,2,1,None);assert_eq!(api::check(77,id),FS_ERR_NOT_FOUND);assert_eq!(api::release(77,id),FS_ERR_NOT_FOUND);assert_eq!(api::commit(77,id),FS_ERR_NOT_FOUND);assert_eq!(api::release(OWNER,id),0);assert_eq!(api::commit(OWNER,id),FS_ERR_NOT_FOUND);assert_eq!(api::operations(),0);assert!(STATE.lock().calls.is_empty());}
'''


def main():
    source = (ROOT / 'src/r/io/async_fs_cabi.rs').read_text()
    vmcall = (ROOT / 'src/hv/vmcall.rs').read_text()
    env = (ROOT / 'src/r/io.rs').read_text()
    path = (ROOT / 'src/r/path.rs').read_text()
    path = path[:path.rfind('#[derive', 0, path.index('pub struct StripPrefixError'))]
    start = source.index('#[derive(Clone, Copy, Debug)]\npub(crate) struct CheckedWriteOptions')
    options = source[start:source.index('/// Fixed, versioned wire header:', start)]
    funcs = ['decode_checked_write_payload', 'next_operation_id', 'start_write_checked',
             'start_write_inner', 'stage_write', 'write_chunk', 'write_commit',
             'owner_for_vm', 'take_request', 'finish_request', 'status', 'discard',
             'selected_disk', 'parse_path', 'guest_checked_write_begin',
             'trueos_cabi_async_fs_checked_write_begin_v1']
    production = '\n'.join(extract(source, name) for name in funcs)
    production = '''
#[path="@ROOT@/src/r/io/async_fs_progress.rs"]mod progress;
static PROGRESS:progress::Tracker=progress::Tracker::new();
fn progress_now_ms()->u64{100}
fn trace_slow_progress(_:progress::Snapshot){}
fn lock_operations()->std::sync::MutexGuard<'static,BTreeMap<u32,Operation>>{ASYNC_FS_OPERATIONS.lock()}
'''.replace('@ROOT@', str(ROOT)) + production
    volatile = (ROOT / 'src/r/fs/volatile_files.rs').read_text()
    rust = HARNESS
    for marker, value in {
        '@CODES@': (ROOT / 'src/r/cabi_codes.rs').read_text(),
        '@PATH@': path,
        '@ENV@': '\n'.join(extract(env, n) for n in ['normalize_app_path', 'resolve_fs_path']),
        '@OPTIONS@': options,
        '@SELECTORS@': '\n'.join(extract(volatile, n) for n in ['contains', 'disk_for_path', 'selected_disk']),
        '@PRODUCTION@': production,
        '@WORKER@': block(source, '            if let Some(options) = checked {'),
        '@MOUNT_POLICY@': extract(vmcall, 'vm_mount_selector_allowed'),
        '@ARM@': block(vmcall, '        OP_BP_ASYNC_FS_CHECKED_WRITE_BEGIN_V1 => {'),
    }.items():
        rust = rust.replace(marker, value)
    with tempfile.TemporaryDirectory(prefix='async-fs-checked-write-') as directory:
        project = Path(directory)
        (project / 'src').mkdir()
        (project / 'src/lib.rs').write_text(rust + TESTS)
        (project / 'Cargo.toml').write_text(f'''[package]
name="async-fs-checked-write-tests"
version="0.0.0"
edition="2024"
[dependencies]
infer={{path="{ROOT / 'vendor/infer'}",default-features=false}}
[workspace]
''')
        subprocess.run(['cargo', 'test', '--offline', '--quiet', '--manifest-path',
                        str(project / 'Cargo.toml'), '--', '--test-threads=1'],
                       cwd=project, check=True)


if __name__ == '__main__':
    main()
