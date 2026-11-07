#!/usr/bin/env python3
"""Exercise production provisioning/store/policy, with fault-injected host storage.

The real engine's key inheritance and lease authorization have separate tests.
This fixture checks durable publication and grant ordering, not block drivers.
"""
from pathlib import Path
import argparse, json, re, subprocess, tempfile
ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::collections::BTreeMap;
use std::sync::{Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}};
use trueos_crypto::AccountId;
use trueos_fs::RecordKey;
#[path="@ROOT@/src/access_key_db.rs"] mod access_key_db;
#[path="@ROOT@/src/access_key_store.rs"] mod access_key_store;
#[path="@ROOT@/src/access_key_workspace.rs"] mod access_key_workspace;
macro_rules! log_warn { (target:$t:expr;$($a:tt)*) => {{ let _=format_args!($($a)*); }}; }
pub(crate) use log_warn;
mod allcaps { pub mod hv { pub const VM_ID_LIMIT:usize=4; } }
mod allocators { pub fn with_host_alloc_domain_strong<T>(f:impl FnOnce()->T)->T { f() } }
mod hv { pub mod blueprint { pub fn app_fs_root_for_archive(a:&str,_:&[u8])->String { format!("apps/{}",a.strip_suffix(".bp").unwrap_or(a)) } } }
mod wait {
 pub struct EmbassySpinRawMutex(spin::Mutex<()>);
 unsafe impl embassy_sync::blocking_mutex::raw::RawMutex for EmbassySpinRawMutex {
  const INIT:Self=Self(spin::Mutex::new(()));
  fn lock<R>(&self,f:impl FnOnce()->R)->R {let _g=self.0.lock();f()}
 }
}
mod disc { pub mod block {
 #[derive(Clone,Copy)]pub struct DeviceHandle;
 #[derive(Clone,Copy,Debug,PartialEq)]pub enum Error{Io,Corrupted,NotReady}
} }
mod tyche {
 pub fn fill_bytes(bytes:&mut[u8])->bool {
  if crate::FAIL_ENTROPY.swap(false,crate::Ordering::SeqCst){return false;}
  let n=crate::ENTROPY.fetch_add(1,crate::Ordering::SeqCst) as u8;
  bytes.fill(n.max(1)); true
 }
}
mod r {
 pub mod io {pub mod kfs {
  #[derive(Clone,Copy,Debug,PartialEq)]pub enum FsError{NoRoot,BadPath,NoSpace,AlreadyExists,PermissionDenied,Device(crate::disc::block::Error)}
  pub type Result<T>=core::result::Result<T,FsError>;
  impl From<crate::disc::block::Error> for FsError{fn from(e:crate::disc::block::Error)->Self{Self::Device(e)}}
 }}
 pub mod fs {
  pub mod trueosfs {
   use crate::*;pub use trueos_fs::{RecordKey,ContentTypeId};use crate::disc::block::{DeviceHandle,Error};use sha2::{Digest,Sha256};
   #[derive(Clone,Copy)]pub struct NodeInfo{pub record_key:RecordKey}
   pub enum CheckedWriteError{Device(Error),AlreadyExists,Conflict}
   pub fn primary_root_handle()->Option<DeviceHandle>{Some(DeviceHandle)}
   pub async fn node_info_async(_:DeviceHandle,path:&str)->Result<Option<NodeInfo>,Error>{Ok(KEYS.lock().unwrap().get(path).copied().map(|record_key|NodeInfo{record_key}))}
   pub async fn file_out_async(_:DeviceHandle,path:&str)->Result<Option<Vec<u8>>,Error>{EVENTS.lock().unwrap().push("read");if FAIL_READ.swap(false,Ordering::SeqCst){return Err(Error::Io)}Ok(FILES.lock().unwrap().get(path).cloned())}
   pub async fn dir_create_all_async(_:DeviceHandle,_:&str)->Result<bool,Error>{EVENTS.lock().unwrap().push("mkdir");Ok(true)}
   pub async fn file_in_typed_checked_async(_:DeviceHandle,path:&str,bytes:&[u8],_:ContentTypeId,create_new:bool,expected:Option<[u8;32]>,_:bool)->Result<bool,CheckedWriteError>{
    EVENTS.lock().unwrap().push("publish");if FAIL_PUBLISH.swap(false,Ordering::SeqCst){return Err(CheckedWriteError::Device(Error::Io))}
    let mut fs=FILES.lock().unwrap();let old=fs.get(path);
    if create_new&&old.is_some(){return Err(CheckedWriteError::AlreadyExists)}
    if let Some(expected)=expected{if old.map(|b|<[u8;32]>::from(Sha256::digest(b)))!=Some(expected){return Err(CheckedWriteError::Conflict)}}
    fs.insert(path.into(),bytes.to_vec());Ok(true)
   }
   pub async fn stamp_directory_key_async(_:DeviceHandle,path:&str,key:RecordKey)->Result<bool,Error>{EVENTS.lock().unwrap().push("stamp");if FAIL_STAMP.swap(false,Ordering::SeqCst){return Ok(false)}let mut keys=KEYS.lock().unwrap();if keys.get(path).is_some_and(|k|*k!=RecordKey::Ffa&&*k!=key){return Ok(false)}keys.insert(path.into(),key);Ok(true)}
  }
  pub mod volatile_files {
   #[derive(Debug)]pub struct SeedStats;
   pub async fn prepare(_:&str)->crate::r::io::kfs::Result<SeedStats>{crate::EVENTS.lock().unwrap().push("seed");if crate::FAIL_SEED.swap(false,crate::Ordering::SeqCst){Err(crate::r::io::kfs::FsError::NoSpace)}else{Ok(SeedStats)}}
   pub async fn stamp_workspace_key(_:&str,_:trueos_fs::RecordKey)->crate::r::io::kfs::Result<()>{crate::EVENTS.lock().unwrap().push("ram-stamp");Ok(())}
  }
 }
}
static FILES:Mutex<BTreeMap<String,Vec<u8>>>=Mutex::new(BTreeMap::new());
static KEYS:Mutex<BTreeMap<String,RecordKey>>=Mutex::new(BTreeMap::new());
static EVENTS:Mutex<Vec<&'static str>>=Mutex::new(Vec::new());
static ENTROPY:AtomicUsize=AtomicUsize::new(1);
static FAIL_ENTROPY:AtomicBool=AtomicBool::new(false);
static FAIL_READ:AtomicBool=AtomicBool::new(false);
static FAIL_PUBLISH:AtomicBool=AtomicBool::new(false);
static FAIL_STAMP:AtomicBool=AtomicBool::new(false);
static FAIL_SEED:AtomicBool=AtomicBool::new(false);
fn run<F:std::future::Future>(f:F)->F::Output{let mut f=std::pin::pin!(f);let mut cx=std::task::Context::from_waker(std::task::Waker::noop());loop{if let std::task::Poll::Ready(x)=f.as_mut().poll(&mut cx){return x}}}
fn reset(){FILES.lock().unwrap().clear();KEYS.lock().unwrap().clear();EVENTS.lock().unwrap().clear();for flag in [&FAIL_ENTROPY,&FAIL_READ,&FAIL_PUBLISH,&FAIL_STAMP,&FAIL_SEED]{flag.store(false,Ordering::SeqCst)}access_key_db::configure_policy(|db|*db=trueos_fs::access::AccessKeyDb::new());for vm in 0..4{access_key_db::clear_vm(vm)}}
const ALICE:AccountId=AccountId::new(1);const BOB:AccountId=AccountId::new(2);
fn prepare(root:&str,account:Option<AccountId>)->r::io::kfs::Result<r::fs::volatile_files::SeedStats>{run(access_key_workspace::prepare("codex.bp",root,account))}
fn bound_key(root:&str)->RecordKey{KEYS.lock().unwrap()[root]}
#[test]fn anonymous_or_noncanonical_launch_cannot_touch_storage(){reset();assert!(prepare("apps/codex_anon",None).is_err());assert!(prepare("apps/other",Some(ALICE)).is_err());assert!(EVENTS.lock().unwrap().is_empty());}
#[test]fn durable_key_reused_after_policy_reset_and_second_instance(){reset();let root="apps/codex_reuse";prepare(root,Some(ALICE)).unwrap();let key=bound_key(root);let image=FILES.lock().unwrap()[access_key_workspace::CONFIGURATION_DB_PATH].clone();assert_eq!(*EVENTS.lock().unwrap(),vec!["read","mkdir","publish","read","mkdir","stamp","seed","ram-stamp"]);access_key_db::configure_policy(|db|*db=trueos_fs::access::AccessKeyDb::new());prepare(root,Some(ALICE)).unwrap();assert_eq!(bound_key(root),key);assert_eq!(FILES.lock().unwrap()[access_key_workspace::CONFIGURATION_DB_PATH],image);for vm in [0,1]{access_key_db::bind_launch(vm,"codex.bp",root,Some(ALICE));assert_eq!(access_key_db::authorize_protected_workspace(vm,key),Ok(()));}}
#[test]fn foreign_owner_cannot_reassign_or_start_existing_tree(){reset();let root="apps/codex_foreign";prepare(root,Some(ALICE)).unwrap();let before=FILES.lock().unwrap().clone();EVENTS.lock().unwrap().clear();assert_eq!(prepare(root,Some(BOB)).unwrap_err(),r::io::kfs::FsError::PermissionDenied);assert_eq!(*EVENTS.lock().unwrap(),vec!["read"]);assert_eq!(*FILES.lock().unwrap(),before);access_key_db::bind_launch(0,"codex.bp",root,Some(BOB));assert!(access_key_db::authorize_protected_workspace(0,bound_key(root)).is_err());}
#[test]fn publication_failure_never_stamps_or_grants(){reset();FAIL_PUBLISH.store(true,Ordering::SeqCst);let root="apps/codex_pubfail";assert!(prepare(root,Some(ALICE)).is_err());assert!(KEYS.lock().unwrap().is_empty());assert!(FILES.lock().unwrap().is_empty());assert!(!EVENTS.lock().unwrap().contains(&"seed"));}
#[test]fn failed_stamp_or_ram_preparation_never_grants_and_can_retry(){reset();let root="apps/codex_stampfail";FAIL_STAMP.store(true,Ordering::SeqCst);assert!(prepare(root,Some(ALICE)).is_err());let image=FILES.lock().unwrap()[access_key_workspace::CONFIGURATION_DB_PATH].clone();FAIL_SEED.store(true,Ordering::SeqCst);assert!(prepare(root,Some(ALICE)).is_err());let key=bound_key(root);access_key_db::bind_launch(0,"codex.bp",root,Some(ALICE));assert!(access_key_db::authorize_protected_workspace(0,key).is_err());prepare(root,Some(ALICE)).unwrap();assert_eq!(FILES.lock().unwrap()[access_key_workspace::CONFIGURATION_DB_PATH],image);assert_eq!(access_key_db::authorize_protected_workspace(0,key),Ok(()));}
#[test]fn read_or_entropy_failure_cannot_initialize_a_new_catalog(){reset();FAIL_READ.store(true,Ordering::SeqCst);assert!(prepare("apps/codex_readfail",Some(ALICE)).is_err());FAIL_ENTROPY.store(true,Ordering::SeqCst);assert!(prepare("apps/codex_rngfail",Some(ALICE)).is_err());assert!(FILES.lock().unwrap().is_empty());assert!(KEYS.lock().unwrap().is_empty());}
'''
def main():
 p=argparse.ArgumentParser();p.add_argument('--target-dir',type=Path);a=p.parse_args()
 channel=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1]
 with tempfile.TemporaryDirectory(prefix='trueos-owned-workspace-') as temporary:
  d=Path(temporary);(d/'lib.rs').write_text(HARNESS.replace('@ROOT@',str(ROOT)))
  (d/'Cargo.toml').write_text('[package]\nname="trueos-owned-workspace-tests"\nversion="0.0.0"\nedition="2024"\n[lib]\npath="lib.rs"\n[workspace]\n[dependencies]\n'+''.join(f'{name}={{path={json.dumps(str(path))}, features=["std"]}}\n' if name=='trueos-redb' else f'{name}={{path={json.dumps(str(path))}}}\n' for name,path in [('trueos-fs',ROOT/'crates/trueos-fs'),('trueos-crypto',ROOT/'crates/trueos-crypto'),('trueos-redb',ROOT.parent/'TRUEOS-Blueprints/crates/trueos-redb')])+'spin="0.10"\nsha2="0.10.9"\nembassy-sync="0.8"\n')
  cmd=['cargo','+'+channel,'test','--offline','--target','x86_64-unknown-linux-gnu']
  if a.target_dir:cmd+=['--target-dir',str(a.target_dir)]
  subprocess.run(cmd+['--','--test-threads=1'],cwd=d,check=True)
if __name__=='__main__':main()
