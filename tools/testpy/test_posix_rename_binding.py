#!/usr/bin/env python3
"""Production rename + open-file-world tests with a deterministic filesystem.

The namespace CABI alone is mocked. Production open/materialize/write/fsync/close
and rename bookkeeping run with mutexes that reject suspension while locked.
"""
from pathlib import Path
import subprocess
import tempfile
from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "src/std_abi_shim.rs"


def harness():
    source = SOURCE.read_text()
    worlds = source[source.index("pub(crate) struct RegularFileWorld {"):source.index("enum SocketFd {")]
    definitions = worlds + '\n'.join(function(SOURCE, name) for name in (
        "rename_regular_file_worlds", "commit_regular_file_rename", "rename",
        "renameat_dirfd_check", "renameat", "open_regular_file_world",
        "materialize_regular_file_world", "writeback_regular_file_world",
        "cleanup_regular_file_world", "fsync", "write", "close", "remove_open_file_for_close",
    ))
    return SUPPORT.replace('@ROOT@', str(ROOT)) + definitions


SUPPORT = r'''
#![allow(non_camel_case_types, dead_code)]
extern crate alloc;
use alloc::{string::String,vec::Vec,sync::Arc};
use std::{ffi::{c_char,c_int,c_void,CStr,CString},collections::BTreeMap,sync::atomic::{AtomicI32,AtomicUsize,Ordering}};
#[path="@ROOT@/src/r/static_map.rs"] mod static_map;
use static_map::FixedKeyMap;
#[macro_export] macro_rules! log {($($args:tt)*)=>{{let _=format_args!($($args)*);}};}
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {const fn new(v:T)->Self {Self(std::sync::Mutex::new(v))} fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.try_lock().expect("mutex held across cooperative wait")}}
const OPEN_FILE_CAPACITY:usize=8;
const TRUEOS_EINVAL:i32=22;const TRUEOS_ENOENT:i32=2;const TRUEOS_EIO:i32=5;
const TRUEOS_EBADF:i32=9;const TRUEOS_ENOTDIR:i32=20;const TRUEOS_EAGAIN:i32=11;
const TRUEOS_AT_FDCWD:i32=-100;const TRUEOS_O_APPEND:i32=1024;
static TRUEOS_ERRNO:AtomicI32=AtomicI32::new(0);
static REGULAR_FILE_WORLDS:Mutex<RegularFileWorldRegistry>=Mutex::new(RegularFileWorldRegistry::new());
static OPEN_FILES:Mutex<FixedKeyMap<i32,OpenFile,8>>=Mutex::new(FixedKeyMap::new());
static SOCKET_FDS:Mutex<FixedKeyMap<i32,(),8>>=Mutex::new(FixedKeyMap::new());
static FD_FLAGS:Mutex<FixedKeyMap<i32,i32,8>>=Mutex::new(FixedKeyMap::new());
#[derive(Clone)] enum OpenFile {
 Regular {path:Option<String>,world:Arc<Mutex<RegularFileWorld>>,offset:usize,writable:bool,flags:i32},
 PipeRead {pipe:Arc<Mutex<BytePipe>>},PipeWrite {pipe:Arc<Mutex<BytePipe>>},
 UnixSocket {rx:Arc<Mutex<BytePipe>>,tx:Arc<Mutex<BytePipe>>},
}
struct BytePipe {bytes:Vec<u8>,read_open:bool,write_open:bool}
enum SocketFd {Cabi{backend:u32},PendingListener{backend:u32},MioListener{backend:u32},MioStream{backend:u32},MioUdp{backend:u32},PendingUdp}
fn remove_socket_fd_for_close(_:i32)->Option<(SocketFd,bool)>{None}
mod mio_compat {pub unsafe fn trueos_mio_socket_close(_:u32)->i32 {unreachable!()}}
fn child_vmx_stdio()->bool {false}
fn write_child_vmx_stdout(_: &[u8])->Result<usize,i32>{unreachable!()}
unsafe fn sys_write(_:u32,_:*const u8,_:usize){unreachable!()}
unsafe fn send(_:i32,_:*const c_void,_:usize,_:i32)->isize {unreachable!()}
fn wake_current_blueprint_io(){}
fn trace_posix_file_world(_:Option<&str>)->bool {false}
fn abi_read_bytes<'a>(p:*const u8,n:usize)->Option<&'a [u8]> {if p.is_null(){None}else{Some(unsafe{std::slice::from_raw_parts(p,n)})}}
fn abi_cstr_to_string(p:*const c_char,_:usize)->Option<String>{if p.is_null(){None}else{Some(unsafe{CStr::from_ptr(p)}.to_str().ok()?.into())}}
fn fs_rc_to_errno(rc:i32)->i32 {match rc {-5=>2,-9=>17,_=>5}}
#[derive(Default)]struct Disk {files:BTreeMap<String,Vec<u8>>,fail_write:bool,fail_rename:i32,pending:Option<(String,String)>,poll:bool}
static DISK:Mutex<Disk>=Mutex::new(Disk{files:BTreeMap::new(),fail_write:false,fail_rename:0,pending:None,poll:false});
static POLLS:AtomicUsize=AtomicUsize::new(0);
static DISCARDS:AtomicUsize=AtomicUsize::new(0);
fn key(p:&str)->String {r::io::env::resolve_fs_path(p,false).unwrap()}
fn boundary(){
 drop(OPEN_FILES.lock());drop(SOCKET_FDS.lock());drop(REGULAR_FILE_WORLDS.lock());
 let worlds:Vec<_>=REGULAR_FILE_WORLDS.lock().entries.iter().flatten().map(|e|e.world.clone()).collect();
 for w in worlds {drop(w.lock());}
}
fn read_file_from_cabi(p:&str)->Result<Vec<u8>,i32>{boundary();DISK.lock().files.get(&key(p)).cloned().ok_or(2)}
fn write_file_to_cabi(p:&str,b:&[u8])->Result<(),i32>{boundary();let mut d=DISK.lock();if d.fail_write{return Err(5)}d.files.insert(key(p),b.into());Ok(())}
fn regular_file_metadata(p:&str)->Result<r::io::kfs::FsStat,i32>{boundary();if !DISK.lock().files.contains_key(&key(p)){return Err(2)}Ok(r::io::kfs::FsStat{revision:1,file_id:17,root_id:2})}
mod r {
 #[path="@ROOT@/src/r/path.rs"]pub mod path;
 pub mod net {pub mod socket_cabi {pub fn trueos_cabi_socket_tcp_close(_:u32)->i32{unreachable!()}}}
 pub mod io {
  pub mod kfs {pub fn exists(_: &str)->Result<bool,()> {unreachable!()} #[derive(Clone,Copy)]pub struct FsStat{pub revision:u64,pub file_id:u64,pub root_id:u32}}
  pub mod env {
   pub fn resolve_fs_path(p:&str,_:bool)->Option<String>{
    // Alias normalization happens outside all file/world locks.
    drop(crate::OPEN_FILES.lock());drop(crate::REGULAR_FILE_WORLDS.lock());
    let p=p.trim_start_matches('/');
    if p.is_empty()||p.split('/').any(|c|c==".."){return None}
    Some(if p.starts_with("apps/"){p.into()}else{format!("apps/codex/{p}")})
   }
  }
  pub mod cabi {
   pub unsafe fn trueos_cabi_fs_exists(p:*const u8,n:usize)->i32{crate::boundary();i32::from(crate::DISK.lock().files.contains_key(std::str::from_utf8(unsafe{std::slice::from_raw_parts(p,n)}).unwrap()))}
   pub fn trueos_cabi_poll_once(){crate::boundary();crate::POLLS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}
  }
  pub mod async_fs_cabi {
   pub const FS_ERR_ALREADY_EXISTS:i32=-9;
   pub unsafe fn rename_replace_start(a:*const u8,n:usize,b:*const u8,m:usize)->i32{
    crate::boundary();let mut d=crate::DISK.lock();
    d.pending=Some((String::from_utf8(unsafe{std::slice::from_raw_parts(a,n)}.into()).unwrap(),String::from_utf8(unsafe{std::slice::from_raw_parts(b,m)}.into()).unwrap()));d.poll=true;7
   }
   pub fn trueos_cabi_async_fs_status(_:u32)->i32{
    crate::boundary();let mut d=crate::DISK.lock();if d.poll{d.poll=false;return 0}
    if d.fail_rename!=0{return d.fail_rename}
    let (a,b)=d.pending.take().unwrap();let Some(bytes)=d.files.remove(&a)else{return -5};d.files.insert(b,bytes);1
   }
   pub fn trueos_cabi_async_fs_discard(_:u32)->i32{crate::boundary();crate::DISCARDS.fetch_add(1,std::sync::atomic::Ordering::Relaxed);0}
  }
 }
}
fn reset(){*OPEN_FILES.lock()=FixedKeyMap::new();*SOCKET_FDS.lock()=FixedKeyMap::new();*REGULAR_FILE_WORLDS.lock()=RegularFileWorldRegistry::new();*DISK.lock()=Disk::default();POLLS.store(0,Ordering::Relaxed);DISCARDS.store(0,Ordering::Relaxed);}
fn disk(p:&str,b:&[u8]){DISK.lock().files.insert(key(p),b.into());}
fn open_fd(fd:i32,p:&str,create:bool)->Arc<Mutex<RegularFileWorld>>{let (w,_)=open_regular_file_world(p,create,create,true).unwrap();assert!(OPEN_FILES.lock().insert(fd,OpenFile::Regular{path:Some(p.into()),world:w.clone(),offset:0,writable:true,flags:0}).is_ok());w}
fn put(fd:i32,b:&[u8]){assert_eq!(unsafe{write(fd,b.as_ptr().cast(),b.len())},b.len()as isize);}
fn invoke(a:&str,b:&str)->i32{let a=CString::new(a).unwrap();let b=CString::new(b).unwrap();unsafe{rename(a.as_ptr(),b.as_ptr())}}
fn fd_path(fd:i32)->Option<String>{match OPEN_FILES.lock().get(fd).unwrap(){OpenFile::Regular{path,..}=>path.clone(),_=>panic!()}}
'''

TESTS = r'''
#[test]fn dirty_open_temporary_persists_then_write_sync_close_follow_new_name(){
 reset();disk("config",b"old");let source=open_fd(7,"/apps/codex/.tmp",true);put(7,b"new");
 assert!(!DISK.lock().files.contains_key("apps/codex/.tmp"));
 assert_eq!(invoke("apps/codex/.tmp","/apps/codex/config"),0);
 assert_eq!(DISK.lock().files["apps/codex/config"],b"new");assert!(!DISK.lock().files.contains_key("apps/codex/.tmp"));
 assert_eq!(fd_path(7).as_deref(),Some("/apps/codex/config"));
 assert!(REGULAR_FILE_WORLDS.lock().get("/apps/codex/.tmp").is_none());
 assert!(Arc::ptr_eq(&REGULAR_FILE_WORLDS.lock().get("/apps/codex/config").unwrap(),&source));
 put(7,b"!");assert_eq!(unsafe{fsync(7)},0);put(7,b"?");assert_eq!(unsafe{close(7)},0);
 assert_eq!(DISK.lock().files["apps/codex/config"],b"new!?");assert!(!DISK.lock().files.contains_key("apps/codex/.tmp"));
 assert_eq!(POLLS.load(Ordering::Relaxed),1);assert_eq!(DISCARDS.load(Ordering::Relaxed),1);
}
#[test]fn replaced_open_description_keeps_old_bytes_and_cannot_clobber_destination(){
 reset();disk("config",b"old");let old=open_fd(6,"/apps/codex/config",false);
 open_fd(7,"tmp",true);put(7,b"new");assert_eq!(invoke("tmp","/apps/codex/config"),0);
 assert_eq!(old.lock().bytes,b"old");assert_eq!(fd_path(6),None);
 let (fresh,_)=open_regular_file_world("/apps/codex/config",false,false,false).unwrap();assert_eq!(fresh.lock().bytes,b"new");assert!(!Arc::ptr_eq(&old,&fresh));
 put(6,b"BAD");assert_eq!(unsafe{fsync(6)},0);assert_eq!(unsafe{close(6)},0);assert_eq!(unsafe{close(7)},0);
 assert_eq!(DISK.lock().files["apps/codex/config"],b"new");
}
#[test]fn failed_rename_preserves_all_bindings_and_original_destination(){
 for flush_fails in [false,true]{reset();disk("config",b"old");let old=open_fd(6,"config",false);let source=open_fd(7,"tmp",true);put(7,b"new");
 {let mut d=DISK.lock();d.fail_write=flush_fails;d.fail_rename=-9;}
 assert_eq!(invoke("tmp","config"),-1);assert_eq!(fd_path(7).as_deref(),Some("tmp"));assert_eq!(fd_path(6).as_deref(),Some("config"));
 assert!(Arc::ptr_eq(&REGULAR_FILE_WORLDS.lock().get("tmp").unwrap(),&source));assert!(Arc::ptr_eq(&REGULAR_FILE_WORLDS.lock().get("config").unwrap(),&old));
 assert_eq!(DISK.lock().files["apps/codex/config"],b"old");}
}
#[test]fn same_missing_path_fails_without_namespace_operation(){reset();assert_eq!(invoke("missing","missing"),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),2);assert_eq!(DISCARDS.load(Ordering::Relaxed),0);}
'''


def run(source):
    with tempfile.TemporaryDirectory(prefix="posix-rename-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", "-Awarnings", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=30)


def main():
    run(harness() + TESTS)


if __name__ == "__main__":
    main()
