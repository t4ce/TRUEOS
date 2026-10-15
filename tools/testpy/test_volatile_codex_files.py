#!/usr/bin/env python3
"""Compile production RAM seeding/routing against controlled storage failures."""
from pathlib import Path
import subprocess
import tempfile

from test_indexed_fs_remove import extract

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as embassy_sync;
use std::{collections::BTreeMap, sync::{Mutex as StdMutex, MutexGuard}, future::Future, task::{Context, Waker}, pin::pin};
pub struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> { pub const fn new(v:T)->Self{Self(StdMutex::new(v))} pub fn lock(&self)->MutexGuard<'_,T>{self.0.lock().unwrap()} }
pub mod mutex { pub struct Mutex<R,T>(crate::Mutex<T>,std::marker::PhantomData<R>); impl<R,T> Mutex<R,T>{pub const fn new(v:T)->Self{Self(crate::Mutex::new(v),std::marker::PhantomData)}pub async fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock()}} }
#[macro_export] macro_rules! log_info { ($($t:tt)*) => {} }
mod wait { pub struct EmbassySpinRawMutex; }
mod disc { pub mod block {
    #[derive(Debug,Clone,Copy,PartialEq)]pub struct DeviceHandle(pub u32);
    impl DeviceHandle {pub fn id(self)->Self{self}pub fn raw(self)->u32{self.0}pub fn from_raw(raw:u32)->Self{Self(raw)}}
    pub type DiscId=DeviceHandle;
    pub fn device_handle(id:DiscId)->Option<DeviceHandle>{Some(id)}
    #[derive(Debug,Clone,Copy,PartialEq)]pub enum Error{NotReady,Corrupted,Io}
} }
#[derive(Clone,Debug,PartialEq)]struct Node{kind:r::fs::trueosfs::NodeKind,bytes:Vec<u8>,content_type:u32,record_key:u32}
#[derive(Default)]struct State{nodes:BTreeMap<(u32,String),Node>,streams:BTreeMap<u32,(String,Node)>,next:u32,created:usize,formatted:usize,snapshot:bool,fail:bool,chunks:usize,aborts:usize}
static STATE:Mutex<State>=Mutex::new(State{nodes:BTreeMap::new(),streams:BTreeMap::new(),next:0,created:0,formatted:0,snapshot:false,fail:false,chunks:0,aborts:0});
fn dir(d:u32,p:&str){STATE.lock().nodes.insert((d,p.into()),Node{kind:r::fs::trueosfs::NodeKind::Directory,bytes:vec![],content_type:0,record_key:0});}
fn put(p:&str,data:&[u8]){for(i,_)in p.match_indices('/'){dir(1,&p[..i]);}STATE.lock().nodes.insert((1,p.into()),Node{kind:r::fs::trueosfs::NodeKind::File,bytes:data.into(),content_type:7,record_key:99});}
fn run<F:Future>(f:F)->F::Output{let mut f=pin!(f);let mut cx=Context::from_waker(Waker::noop()); for _ in 0..100_000 {if let std::task::Poll::Ready(v)=f.as_mut().poll(&mut cx){return v}}panic!("mock future did not finish")}
mod r {
    #[path="__PATH__"]pub mod path;
    pub mod io {pub mod kfs {
        #[derive(Debug,PartialEq)]pub enum FsError{BadPath,AlreadyExists,NoRoot,NoSpace,NotFound,Device(crate::disc::block::Error)}
        impl From<crate::disc::block::Error> for FsError{fn from(v:crate::disc::block::Error)->Self{Self::Device(v)}}
        pub type Result<T>=std::result::Result<T,FsError>;
        pub fn exists(_: &str)->Result<bool>{Ok(false)}
        use crate::disc::block;
        __KFS_SELECTOR__
        pub fn test_select(path:&str)->(u32,String){let(d,p)=path_disk(path,true).unwrap();(d.0,p)}
    }
    pub mod async_fs_cabi {
        const FS_ERR_NOT_FOUND:i32=-5;const FS_ERR_BAD_PATH:i32=-2;
        __ASYNC_SELECTOR__
        pub fn test_select(path:&str)->(u32,String){let(d,p)=selected_disk(path).unwrap();(d.0,p.into())}
    }}
    pub mod disc {pub mod ramdisk {
        pub fn create_private(cap:u64,bs:u32,label:&str)->Result<crate::disc::block::DeviceHandle,crate::disc::block::Error>{assert_eq!(cap,512*1024*1024);assert_eq!(bs,512);assert_eq!(label,"codex-files");crate::STATE.lock().created+=1;Ok(crate::disc::block::DeviceHandle(2))}
    }}
    pub mod fs {
        pub mod trueosfs {
            use crate::{STATE,Node,disc::block::{DeviceHandle as D,Error as E}};
            #[derive(Clone,Copy,Debug,PartialEq)]pub enum NodeKind{File,Directory}
            pub struct DirEntry{pub name:String,pub kind:NodeKind}
            pub struct DirListing{pub entries:Vec<DirEntry>,pub truncated:bool}
            pub struct FileInfo{pub data_len:u64,pub content_type:u32,pub record_key:u32}
            pub struct Snapshot;impl Drop for Snapshot{fn drop(&mut self){STATE.lock().snapshot=false;}}
            pub fn primary_root_handle()->Option<D>{Some(D(1))}
            pub struct Root{pub disk_id:D}
            pub fn list_roots()->Vec<Root>{vec![Root{disk_id:D(1)}]}
            pub async fn format_blank_at_async(d:D,_:u64)->Result<(),E>{assert_eq!(d.0,2);let mut s=STATE.lock();s.formatted+=1;s.nodes.retain(|(disk,_),_|*disk!=2);Ok(())}
            pub async fn mount_root_async(d:D)->Result<Option<u32>,E>{Ok(Some(d.0))}
            pub async fn read_snapshot_lease_async(d:D)->Result<Snapshot,E>{assert_eq!(d.0,1);STATE.lock().snapshot=true;Ok(Snapshot)}
            pub async fn remove_recursive_async(d:D,p:&str)->Result<bool,E>{assert_eq!(d.0,2);let prefix=format!("{p}/");STATE.lock().nodes.retain(|(disk,path),_|*disk!=d.0||!(path==p||path.starts_with(&prefix)));Ok(true)}
            pub async fn dir_create_all_async(d:D,p:&str)->Result<bool,E>{assert_eq!(d.0,2);for(i,_)in p.match_indices('/'){crate::dir(d.0,&p[..i]);}crate::dir(d.0,p);Ok(true)}
            pub async fn list_dir_with_limit_async(d:D,p:&str,limit:usize)->Result<Option<DirListing>,E>{assert_eq!(limit,usize::MAX);let s=STATE.lock();assert!(s.snapshot);if !s.nodes.contains_key(&(d.0,p.into())){return Ok(None)}let prefix=format!("{p}/");Ok(Some(DirListing{entries:s.nodes.iter().filter_map(|((disk,path),n)|{if *disk!=d.0{return None}let rest=path.strip_prefix(&prefix)?;if rest.contains('/'){return None}Some(DirEntry{name:rest.into(),kind:n.kind})}).collect(),truncated:false}))}
            pub async fn file_info_async(d:D,p:&str)->Result<Option<FileInfo>,E>{let s=STATE.lock();assert!(s.snapshot);Ok(s.nodes.get(&(d.0,p.into())).map(|n|FileInfo{data_len:n.bytes.len()as u64,content_type:n.content_type,record_key:n.record_key}))}
            // A read handle pins the mock immutable source file by a stable key.
            pub async fn file_read_open_async(d:D,p:&str)->Result<Option<u32>,E>{let s=STATE.lock();Ok(s.nodes.keys().position(|(disk,path)|*disk==d.0&&path==p).map(|n|n as u32))}
            pub async fn file_write_begin_with_metadata_async(d:D,p:&str,_:u64,ct:u32,key:u32,_:bool)->Result<Option<u32>,E>{assert_eq!(d.0,2);let mut s=STATE.lock();assert!(s.snapshot);s.next+=1;let id=s.next;s.streams.insert(id,(p.into(),Node{kind:NodeKind::File,bytes:vec![],content_type:ct,record_key:key}));Ok(Some(id))}
            pub async fn file_read_handle_range_async(id:u32,offset:u64,out:&mut[u8])->Result<Option<usize>,E>{let s=STATE.lock();let source=s.nodes.iter().nth(id as usize).unwrap().1;let n=out.len().min(source.bytes.len()-offset as usize);out[..n].copy_from_slice(&source.bytes[offset as usize..offset as usize+n]);Ok(Some(n))}
            pub async fn file_write_chunk_async(id:u32,bytes:&[u8])->Result<(),E>{let mut s=STATE.lock();s.chunks+=1;if s.fail{return Err(E::Io)}s.streams.get_mut(&id).unwrap().1.bytes.extend_from_slice(bytes);Ok(())}
            pub async fn file_write_finish_async(id:u32)->Result<(),E>{let mut s=STATE.lock();let(p,n)=s.streams.remove(&id).unwrap();s.nodes.insert((2,p),n);Ok(())}
            pub async fn file_write_abort_async(id:u32)->Result<(),E>{let mut s=STATE.lock();s.streams.remove(&id);s.aborts+=1;Ok(())}
        }
        pub mod volatile_files {
__PRODUCTION__
            #[cfg(test)]mod tests {
                use super::*;
                use crate::{STATE,put,dir,run};
                fn reset(){*STATE.lock()=crate::State::default();*STORAGE.lock()=None;ROOTS.lock().clear();}
                fn disk(path:&str)->u32{selected_disk(path).unwrap().0}
                #[test]fn all_state_keeps_paths_bytes_types_and_keys_and_disk_seed(){
                    reset();put("apps/codex/.codex/auth.json",b"test-login");put("apps/codex/.codex/sessions/history.jsonl",&vec![42;140_000]);put("apps/codex/cache/test",b"cache");dir(1,"apps/codex/empty");
                    let seed=STATE.lock().nodes.clone();let stats=run(prepare("apps/codex")).unwrap();assert_eq!(stats.files,3);assert_eq!(stats.bytes,140_015);let s=STATE.lock();
                    for((d,p),n)in &seed{if *d==1&&p.starts_with("apps/codex/"){assert_eq!(s.nodes.get(&(2,p.clone())),Some(n));assert_eq!(s.nodes.get(&(1,p.clone())),Some(n));}}
                    assert!(s.streams.is_empty());assert!(!s.snapshot);assert_eq!(s.chunks,5);drop(s);assert_eq!(disk("apps/codex/.codex/auth.json"),2);
                }
                #[test]fn shell3_workspaces_other_apps_and_prefix_neighbors_stay_on_disk(){reset();run(prepare("apps/codex")).unwrap();for p in ["work/project/main.rs","apps/sqlite/db","apps/codexish/x","apps","tmp/global"]{assert_eq!(disk(p),1,"{p}");}assert_eq!(disk("apps/codex/tmp/a"),2);}
                #[test]fn sync_and_async_routes_agree_and_physical_selectors_reach_seed(){reset();put("apps/codex/.codex/auth.json",b"test");run(prepare("apps/codex")).unwrap();for p in ["apps/codex/.codex/auth.json","apps/codex/cache/test","apps/codex/tmp/lock","work/project/file","apps/codexish/file"]{let sync=crate::r::io::kfs::test_select(&format!("/{p}"));let asynchronous=crate::r::io::async_fs_cabi::test_select(p);assert_eq!(sync,asynchronous);assert_eq!(sync.0,if p.starts_with("apps/codex/"){2}else{1});}assert_eq!(crate::r::io::async_fs_cabi::test_select("trueosfs:disc1/apps/codex/.codex/auth.json").0,1);}
                #[test]fn failed_copy_aborts_stream_never_publishes_and_retry_reuses_ram(){reset();put("apps/codex/.codex/auth.json",b"test");STATE.lock().fail=true;assert!(run(prepare("apps/codex")).is_err());assert_eq!(disk("apps/codex/.codex/auth.json"),1);assert!(STATE.lock().streams.is_empty());assert_eq!(STATE.lock().aborts,1);STATE.lock().fail=false;run(prepare("apps/codex")).unwrap();assert_eq!(disk("apps/codex/.codex/auth.json"),2);assert_eq!(STATE.lock().created,1);assert_eq!(STATE.lock().formatted,1);}
                #[test]fn relaunch_preserves_ram_changes_and_boot_restores_disk_seed(){reset();put("apps/codex/.codex/auth.json",b"original");run(prepare("apps/codex")).unwrap();STATE.lock().nodes.get_mut(&(2,"apps/codex/.codex/auth.json".into())).unwrap().bytes=b"refreshed".to_vec();run(prepare("apps/codex")).unwrap();assert_eq!(STATE.lock().nodes[&(2,"apps/codex/.codex/auth.json".into())].bytes,b"refreshed");ROOTS.lock().clear();STORAGE.lock().as_mut().unwrap().initialized=false;run(prepare("apps/codex")).unwrap();assert_eq!(STATE.lock().nodes[&(2,"apps/codex/.codex/auth.json".into())].bytes,b"original");}
                #[test]fn named_and_default_launch_order_preserves_live_ram(){reset();put("apps/codex/named/.codex/auth.json",b"named");put("apps/codex/.codex/auth.json",b"default");run(prepare("apps/codex/named")).unwrap();STATE.lock().nodes.get_mut(&(2,"apps/codex/named/.codex/auth.json".into())).unwrap().bytes=b"changed".to_vec();run(prepare("apps/codex")).unwrap();run(prepare("apps/codex/later")).unwrap();assert_eq!(STATE.lock().nodes[&(2,"apps/codex/named/.codex/auth.json".into())].bytes,b"changed");assert!(STATE.lock().nodes.contains_key(&(2,"apps/codex/later/.codex".into())));assert_eq!(STATE.lock().created,1);assert_eq!(STATE.lock().formatted,1);}
                #[test]fn seed_exceeds_interactive_listing_cap_without_losing_files(){reset();for i in 0..1100{put(&format!("apps/codex/.codex/sessions/{i}"),b"s");}let stats=run(prepare("apps/codex")).unwrap();assert_eq!(stats.files,1100);assert_eq!(STATE.lock().nodes.keys().filter(|(d,p)|*d==2&&p.starts_with("apps/codex/.codex/sessions/")).count(),1100);}
                #[test]fn creates_required_directories_on_empty_seed_and_rejects_other_roots(){reset();assert!(run(prepare("work/project")).is_err());assert_eq!(STATE.lock().created,0);run(prepare("/apps/codex/./")).unwrap();for leaf in [".codex","tmp","cache","config",".local/share",".local/state"]{assert!(STATE.lock().nodes.contains_key(&(2,format!("apps/codex/{leaf}"))));}}
            }
        }
    }
}
'''

with tempfile.TemporaryDirectory(prefix="volatile-codex-") as directory:
    rust = Path(directory) / "test.rs"
    binary = Path(directory) / "test"
    production = (ROOT / "src/r/fs/volatile_files.rs").read_text().replace("//!", "//")
    kfs_source = (ROOT / "src/r/io.rs").read_text()
    kfs = extract(kfs_source, "normalize_rel") + "\n" + extract(kfs_source, "path_disk")
    asynchronous = extract((ROOT / "src/r/io/async_fs_cabi.rs").read_text(), "selected_disk")
    rust.write_text(HARNESS.replace("__PATH__", str(ROOT / "src/r/path.rs")).replace("__PRODUCTION__", production).replace("__KFS_SELECTOR__", kfs).replace("__ASYNC_SELECTOR__", asynchronous))
    subprocess.run(["rustc", "--edition=2024", "--test", "-Awarnings", str(rust), "-o", str(binary)], check=True)
    subprocess.run([str(binary), "--test-threads=1"], check=True)
