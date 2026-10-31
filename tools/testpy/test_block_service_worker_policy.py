#!/usr/bin/env python3
"""Exercise production disk-service policies and registration on the host.

The actual start method, policy enum, proxy layout and registration functions
are compiled. Host spawners/task tokens are counted; no AP preference picker is
provided. No disk, executor polling recursion, or rig connection is involved.
"""
from pathlib import Path
import hashlib
import re
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[2]
EXPECTED_CONSERVED_SOURCE_SHA256='c2b91854d383c4a8e43ca852c687ac6f7eb54a61a51473f16baa6c80d3c873f7'
EXPECTED_SKHYNIX_BEFORE_API_RENAME_SHA256='df967ca1f265c83d1ab681524e253af7d31c7ff06345681b6edd459719c5f43b'


def item(source, signature):
    start=source.index(signature);opening=source.index('{',start);end=opening+1;depth=1
    while depth:
        if source[end]=='{':depth+=1
        if source[end]=='}':depth-=1
        end+=1
    return source[start:end]


HARNESS=r'''
#![allow(dead_code)]
use std::sync::{Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}};
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum Error {NotReady}
type Result<T>=core::result::Result<T,Error>;
#[derive(Clone,Copy,Debug,PartialEq,Eq)] @POLICY@
@PROXY@
struct ServicedBlockDeviceFront {spawned:AtomicBool,_driver:Box<dyn BlockDevice>}
impl ServicedBlockDeviceFront {fn new(driver:Box<dyn BlockDevice>)->Self{Self{spawned:AtomicBool::new(false),_driver:driver}}}
pub trait BlockDevice {
 fn block_size_bytes(&self)->u32{512}fn block_count(&self)->u64{1024}fn dma_alignment_bytes(&self)->u32{0}
 fn max_transfer_bytes(&self)->u64{64}fn supports_write(&self)->bool{true}fn policy(&self)->Option<BlockServiceWorkerPolicy>{None}
}
struct TestDevice;impl BlockDevice for TestDevice{}
impl BlockDevice for ServicedBlockDevice {
 fn block_size_bytes(&self)->u32{self.block_size}fn block_count(&self)->u64{self.block_count}
 fn dma_alignment_bytes(&self)->u32{self.dma_alignment}fn max_transfer_bytes(&self)->u64{self.max_transfer_bytes}
 fn supports_write(&self)->bool{self.writable}fn policy(&self)->Option<BlockServiceWorkerPolicy>{Some(self.worker_policy)}
}
#[derive(Clone,Copy)] pub struct DeviceDescriptor{read_only:bool}
#[derive(Debug,PartialEq,Eq)] pub struct DeviceHandle(u8);
#[derive(Debug,PartialEq,Eq)] struct Registration {policy:BlockServiceWorkerPolicy,mount:bool,block_size:u32,blocks:u64,alignment:u32,max_transfer:u64,writable:bool}
static REGISTERED:Mutex<Option<Registration>>=Mutex::new(None);
fn register_boxed_device(_:DeviceDescriptor,device:Box<dyn BlockDevice>,mount:bool)->DeviceHandle{
 *REGISTERED.lock().unwrap()=Some(Registration{policy:device.policy().unwrap(),mount,block_size:device.block_size_bytes(),blocks:device.block_count(),alignment:device.dma_alignment_bytes(),max_transfer:device.max_transfer_bytes(),writable:device.supports_write()});DeviceHandle(42)
}
#[derive(Clone,Copy)] struct Spawner {slot:u32}
impl Spawner {
 fn cpu_slot(self)->u32{self.slot}
 fn spawn(&self,_:Token){SPAWNS.lock().unwrap().push(self.slot);}
 async unsafe fn for_current_executor()->Self{CURRENT.fetch_add(1,Ordering::Relaxed);Self{slot:19}}
}
struct Token;
static BSP:AtomicBool=AtomicBool::new(true);
static WRONG_BSP:AtomicBool=AtomicBool::new(false);
static FAIL_TOKEN:AtomicBool=AtomicBool::new(false);
static BSP_LOOKUPS:AtomicUsize=AtomicUsize::new(0);
static CURRENT:AtomicUsize=AtomicUsize::new(0);
static TOKENS:AtomicUsize=AtomicUsize::new(0);
static SPAWNS:Mutex<Vec<u32>>=Mutex::new(Vec::new());
static LOGS:Mutex<Vec<String>>=Mutex::new(Vec::new());
const BLOCK_DEVICE_SERVICE_TASK_POOL:usize=16;
mod workers {
 use super::*;
 pub(super) fn spawner_for_slot(slot:u32)->Option<Spawner>{
  assert_eq!(slot,0);BSP_LOOKUPS.fetch_add(1,Ordering::Relaxed);BSP.load(Ordering::Relaxed).then_some(Spawner{slot:if WRONG_BSP.load(Ordering::Relaxed){1}else{0}})
 }
}
#[macro_export] macro_rules! log_info {(target:$target:literal;$($args:tt)*)=>{{crate::LOGS.lock().unwrap().push(format!($($args)*));}}}
fn serviced_block_device_task(_: &'static ServicedBlockDeviceFront)->core::result::Result<Token,()>{TOKENS.fetch_add(1,Ordering::Relaxed);if FAIL_TOKEN.load(Ordering::Relaxed){Err(())}else{Ok(Token)}}
impl ServicedBlockDevice {@ENSURE@}
@REGISTER_INNER@
@REGISTER_CURRENT@
@REGISTER_CURRENT_DEFERRED@
@REGISTER_BSP@
fn reset(policy:BlockServiceWorkerPolicy)->ServicedBlockDevice{
 for a in [&BSP_LOOKUPS,&CURRENT,&TOKENS]{a.store(0,Ordering::Relaxed);}
 BSP.store(true,Ordering::Relaxed);WRONG_BSP.store(false,Ordering::Relaxed);FAIL_TOKEN.store(false,Ordering::Relaxed);SPAWNS.lock().unwrap().clear();LOGS.lock().unwrap().clear();
 ServicedBlockDevice{front:Box::leak(Box::new(ServicedBlockDeviceFront::new(Box::new(TestDevice)))),block_size:512,block_count:1024,dma_alignment:1,max_transfer_bytes:512,writable:true,worker_policy:policy,next_seq:1}
}
fn run(device:&mut ServicedBlockDevice)->Result<()>{let mut future=Box::pin(device.ensure_worker_started());match future.as_mut().poll(&mut core::task::Context::from_waker(core::task::Waker::noop())){core::task::Poll::Ready(result)=>result,core::task::Poll::Pending=>panic!("unexpected async fallback wait")}}
use core::future::Future;
#[test] fn bsp_policy_uses_only_explicit_slot_zero_and_reports_actual_slot(){
 let mut d=reset(BlockServiceWorkerPolicy::Bsp);assert_eq!(run(&mut d),Ok(()));assert_eq!(*SPAWNS.lock().unwrap(),vec![0]);assert_eq!(CURRENT.load(Ordering::Relaxed),0);assert_eq!(BSP_LOOKUPS.load(Ordering::Relaxed),1);
 assert!(LOGS.lock().unwrap().iter().any(|s|s.contains("placement=bsp slot=0 policy=bsp-required")));
}
#[test] fn unavailable_bsp_returns_not_ready_without_current_fallback(){
 let mut d=reset(BlockServiceWorkerPolicy::Bsp);BSP.store(false,Ordering::Relaxed);assert_eq!(run(&mut d),Err(Error::NotReady));assert_eq!(CURRENT.load(Ordering::Relaxed),0);assert!(SPAWNS.lock().unwrap().is_empty());assert_eq!(TOKENS.load(Ordering::Relaxed),0);assert!(!d.front.spawned.load(Ordering::Relaxed));
}
#[test] fn wrong_bsp_slot_rejected_without_ap1_fallback(){
 let mut d=reset(BlockServiceWorkerPolicy::Bsp);WRONG_BSP.store(true,Ordering::Relaxed);assert_eq!(run(&mut d),Err(Error::NotReady));assert_eq!(CURRENT.load(Ordering::Relaxed),0);assert!(SPAWNS.lock().unwrap().is_empty());
}
#[test] fn pool_failure_resets_claim_and_bsp_retry_stays_bsp(){
 let mut d=reset(BlockServiceWorkerPolicy::Bsp);FAIL_TOKEN.store(true,Ordering::Relaxed);assert_eq!(run(&mut d),Err(Error::NotReady));assert!(!d.front.spawned.load(Ordering::Relaxed));assert_eq!(CURRENT.load(Ordering::Relaxed),0);
 FAIL_TOKEN.store(false,Ordering::Relaxed);assert_eq!(run(&mut d),Ok(()));assert_eq!(*SPAWNS.lock().unwrap(),vec![0]);
}
#[test] fn repeated_start_is_idempotent_for_both_policies(){
 for policy in [BlockServiceWorkerPolicy::CurrentExecutor,BlockServiceWorkerPolicy::Bsp]{let mut d=reset(policy);assert_eq!(run(&mut d),Ok(()));let counts=(CURRENT.load(Ordering::Relaxed),BSP_LOOKUPS.load(Ordering::Relaxed),TOKENS.load(Ordering::Relaxed));assert_eq!(run(&mut d),Ok(()));assert_eq!(counts,(CURRENT.load(Ordering::Relaxed),BSP_LOOKUPS.load(Ordering::Relaxed),TOKENS.load(Ordering::Relaxed)));assert_eq!(SPAWNS.lock().unwrap().len(),1);}
}
#[test] fn current_executor_policy_ignores_bsp_availability(){
 let mut d=reset(BlockServiceWorkerPolicy::CurrentExecutor);BSP.store(false,Ordering::Relaxed);WRONG_BSP.store(true,Ordering::Relaxed);assert_eq!(run(&mut d),Ok(()));assert_eq!(BSP_LOOKUPS.load(Ordering::Relaxed),0);assert_eq!(CURRENT.load(Ordering::Relaxed),1);assert_eq!(*SPAWNS.lock().unwrap(),vec![19]);
}
#[test] fn current_executor_pool_failure_preserves_retry_behavior(){
 let mut d=reset(BlockServiceWorkerPolicy::CurrentExecutor);FAIL_TOKEN.store(true,Ordering::Relaxed);assert_eq!(run(&mut d),Err(Error::NotReady));assert!(!d.front.spawned.load(Ordering::Relaxed));assert_eq!(BSP_LOOKUPS.load(Ordering::Relaxed),0);assert_eq!(CURRENT.load(Ordering::Relaxed),1);assert_eq!(TOKENS.load(Ordering::Relaxed),1);
}
#[test] fn registration_apis_select_only_the_intended_policy_and_keep_metadata(){
 for (api,policy,mount) in [(0,BlockServiceWorkerPolicy::CurrentExecutor,true),(1,BlockServiceWorkerPolicy::CurrentExecutor,false),(2,BlockServiceWorkerPolicy::Bsp,false)]{
  for read_only in [false,true]{let desc=DeviceDescriptor{read_only};let result=match api{0=>register_device_with_worker(desc,TestDevice),1=>register_device_with_worker_deferred_mount(desc,TestDevice),_=>register_device_with_bsp_worker_deferred_mount(desc,TestDevice)};assert_eq!(result,DeviceHandle(42));assert_eq!(*REGISTERED.lock().unwrap(),Some(Registration{policy,mount,block_size:512,blocks:1024,alignment:1,max_transfer:512,writable:!read_only}));}
 }
}
'''


def main():
    source=(ROOT/'src/disc/block.rs').read_text()
    assert 'ISOLATION_CONTROL' not in source and 'prefer_eff_worker' not in source
    assert 'pick_eff_background_spawner' not in source
    assert 'register_device_with_worker_deferred_mount_ecore_preferred' not in source
    # Mask only the deliberately replaced start method and normalize the typed
    # policy/API names back to B. Every service/signal/driver/Request/Reply byte
    # and every unrelated registration path must remain identical.
    conserved=source.replace(item(source,'    async fn ensure_worker_started('),'@PLACEMENT_METHOD@',1)
    conserved=conserved.replace('#[derive(Clone, Copy, PartialEq, Eq)]\n'+item(source,'enum BlockServiceWorkerPolicy')+'\n\n','',1)
    conserved=conserved.replace('    worker_policy: BlockServiceWorkerPolicy,','    prefer_eff_worker: bool,').replace('        worker_policy,','        prefer_eff_worker,')
    conserved=conserved.replace('BlockServiceWorkerPolicy::CurrentExecutor','false').replace('BlockServiceWorkerPolicy::Bsp','true')
    conserved=conserved.replace('register_device_with_bsp_worker_deferred_mount','register_device_with_worker_deferred_mount_ecore_preferred')
    conserved=conserved.replace('/// Register a deferred-mount device whose serialized transport worker requires\n/// the BSP executor. If BSP registration is unavailable, I/O returns NotReady.','/// Register a deferred-mount device whose serialized transport worker should\n/// prefer an AP2+ efficiency core. If no E-core is available when first used,\n/// preserve the existing behavior and spawn on the calling executor.')
    assert hashlib.sha256(conserved.encode()).hexdigest()==EXPECTED_CONSERVED_SOURCE_SHA256, 'unrelated block service/driver/request-reply code changed'
    usb=(ROOT/'src/usb3/skhynix.rs').read_text()
    restored=usb.replace('register_device_with_bsp_worker_deferred_mount','register_device_with_worker_deferred_mount_ecore_preferred')
    assert hashlib.sha256(restored.encode()).hexdigest()==EXPECTED_SKHYNIX_BEFORE_API_RENAME_SHA256, 'UAS transport changed beyond registration API'
    symbol='register_device_with_bsp_worker_deferred_mount('
    callers=[str(p.relative_to(ROOT)) for p in (ROOT/'src').rglob('*.rs') if symbol in p.read_text() and p.relative_to(ROOT)!=Path('src/disc/block.rs')]
    assert callers==['src/usb3/skhynix.rs'], callers

    values={
        'POLICY':'pub '+item(source,'enum BlockServiceWorkerPolicy'),
        'PROXY':item(source,'struct ServicedBlockDevice {'),
        'ENSURE':item(source,'    async fn ensure_worker_started('),
        'REGISTER_INNER':item(source,'fn register_device_with_worker_inner<D>('),
        'REGISTER_CURRENT':item(source,'pub fn register_device_with_worker<D>('),
        'REGISTER_CURRENT_DEFERRED':item(source,'pub fn register_device_with_worker_deferred_mount<D>('),
        'REGISTER_BSP':item(source,'pub fn register_device_with_bsp_worker_deferred_mount<D>('),
    }
    harness=HARNESS
    for key,value in values.items():harness=harness.replace('@'+key+'@',value)
    channel=re.search(r'^channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text(),re.M).group(1)
    with tempfile.TemporaryDirectory(prefix='trueos-block-service-policy-') as directory:
        work=Path(directory);(work/'tests.rs').write_text(harness)
        subprocess.run(['rustc','+'+channel,'--test','-Dwarnings','--edition=2024','--target','x86_64-unknown-linux-gnu',str(work/'tests.rs'),'-o',str(work/'tests')],cwd=work,check=True)
        subprocess.run([str(work/'tests'),'--test-threads=1'],cwd=work,check=True)
    print('Production worker policies and actual registration functions passed.')


if __name__=='__main__':main()
