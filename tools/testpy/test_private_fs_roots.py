#!/usr/bin/env python3
"""Verify production root selection never exposes private RAM as the shell disk."""
from pathlib import Path
import subprocess
import tempfile
from test_indexed_fs_remove import extract

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
#![allow(dead_code)]
use std::{sync::{Mutex as StdMutex,MutexGuard,atomic::{AtomicU32,AtomicU64,AtomicUsize,Ordering}},vec::Vec};
struct Mutex<T>(StdMutex<T>);impl<T>Mutex<T>{const fn new(v:T)->Self{Self(StdMutex::new(v))}fn lock(&self)->MutexGuard<'_,T>{self.0.lock().unwrap()}}
#[macro_export]macro_rules! log_info{($($t:tt)*)=>{}}
mod r{pub mod readiness{pub const TRUEOSFS_ROOT_MOUNTED:u32=1;pub fn set(_:u32){}}}
mod block {
    #[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct DiscId(u32);impl DiscId{pub fn raw(self)->u32{self.0}pub fn from_raw(raw:u32)->Self{Self(raw)}}
    #[derive(Clone,Copy,PartialEq,Eq)]pub struct DeviceHandle(pub u32);
    pub struct Info{pub user_visible:bool}
    impl DeviceHandle{pub fn id(self)->DiscId{DiscId(self.0)}pub fn info(self)->Info{Info{user_visible:self.0!=2}}pub fn into_raw(self)->usize{self.0 as usize}pub unsafe fn from_raw(raw:usize)->Self{Self(raw as u32)}}
    pub fn device_handle(id:DiscId)->Option<DeviceHandle>{(id.0!=0).then_some(DeviceHandle(id.0))}
}
#[derive(Clone,Copy)]struct TrueosFsPlacement;
struct RootMount{building_index:bool,disk_id:block::DiscId,placement:TrueosFsPlacement,seq:u64,index:Option<Vec<()>>,writes_since_checkpoint:u64,cache_gen:u64}
struct RootInfo{disk_id:block::DiscId,seq:u64,index_ready:bool,index_building:bool}
static ROOTS:Mutex<Vec<RootMount>>=Mutex::new(Vec::new());
static INDEX_QUEUE:Mutex<Vec<block::DeviceHandle>>=Mutex::new(Vec::new());
static ROOT_SEQ:AtomicU64=AtomicU64::new(0);
static PRIMARY_ROOT_RAW:AtomicU32=AtomicU32::new(0);
static PRIMARY_ROOT_HANDLE_RAW:AtomicUsize=AtomicUsize::new(0);
fn file_record_cache_invalidate_disk(_:block::DiscId){}
fn reset(){ROOTS.lock().clear();PRIMARY_ROOT_RAW.store(0,Ordering::Release);PRIMARY_ROOT_HANDLE_RAW.store(0,Ordering::Release);}
fn mount(raw:u32,replace:bool){register_root_mount(block::DeviceHandle(raw),TrueosFsPlacement,replace);}
#[test]fn private_mount_and_remount_preserve_shell_disk_and_public_roots(){reset();mount(1,false);mount(2,false);mount(2,true);assert_eq!(primary_root_handle(),Some(block::DeviceHandle(1)));assert_eq!(list_roots().iter().map(|r|r.disk_id.raw()).collect::<Vec<_>>(),vec![1]);mount(3,false);mount(2,true);assert_eq!(primary_root_handle(),Some(block::DeviceHandle(3)));assert_eq!(list_roots().iter().map(|r|r.disk_id.raw()).collect::<Vec<_>>(),vec![3,1]);}
#[test]fn fallback_ignores_newer_private_mount(){reset();mount(1,false);mount(2,false);PRIMARY_ROOT_RAW.store(0,Ordering::Release);PRIMARY_ROOT_HANDLE_RAW.store(0,Ordering::Release);assert_eq!(primary_root_id().map(|id|id.raw()),Some(1));}
#[test]fn private_only_mount_does_not_supply_default_or_public_root(){reset();mount(2,false);assert!(primary_root_handle().is_none());assert!(list_roots().is_empty());}
'''
source = (ROOT / "src/r/fs/trueosfs.rs").read_text()
production = "\n".join(extract(source, name) for name in ["register_root_mount", "list_roots", "primary_root_id", "primary_root_handle"])
with tempfile.TemporaryDirectory(prefix="private-fs-roots-") as directory:
    rust = Path(directory) / "test.rs"
    binary = Path(directory) / "test"
    rust.write_text(HARNESS.replace("#[derive(Clone,Copy,PartialEq,Eq)]", "#[derive(Clone,Copy,Debug,PartialEq,Eq)]") + production)
    subprocess.run(["rustc", "--edition=2024", "--test", "-Awarnings", str(rust), "-o", str(binary)], check=True)
    subprocess.run([str(binary), "--test-threads=1"], check=True)
