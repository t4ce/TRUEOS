#!/usr/bin/env python3
"""Poll real read-range observation code with host-owned filesystem boundaries.

The async helper, enum, broker arm, tracker and completion hooks come directly
from production sources. Host filesystem steps can return, fail or stay pending;
no rig, disk contents or saved external baseline is required.
"""
from pathlib import Path
import hashlib
import importlib.util
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('native_fs_base_harness', Path(__file__).with_name('test_native_fs_progress.py'))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


def item(source, signature):
    start = source.index(signature)
    opening = source.index('{', start)
    end = opening + 1
    depth = 1
    while depth:
        if source[end] == '{': depth += 1
        if source[end] == '}': depth -= 1
        end += 1
    return source[start:end]


def range_arm(source):
    start = source.index('        Request::ReadFileRange {\n            id,', source.index('async fn process_request'))
    start = source.index('} => {', start) + len('} => ')
    end = start + 1
    depth = 1
    while depth:
        if source[end] == '{': depth += 1
        if source[end] == '}': depth -= 1
        end += 1
    return source[start + 1:end - 1]


LEGACY_HELPER_BODY_SHA256 = '238e265bfbb834606d706da0cbfc800fd51c2dddfd8bd8d5367842935493a836'
LEGACY_BROKER_ARM_SHA256 = '9d55d60b77ad1278ccf5825765cd821c9780b9704b067abc7d10015136ddb704'

STUBS = r'''
use std::sync::Mutex;
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct Placement {super_lba:u64,data_lba:u64,data_end_lba_exclusive:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum EngineError {Failure}
#[derive(Clone)] struct State {
 activity_error:bool,activity_live:usize,placement_ready:bool,placement:Result<Option<Placement>,disc::block::Error>,
 index_ready:bool,index:Result<Option<trueos_fs::FileRecordRef>,disc::block::Error>,
 read_ready:bool,read:Result<Option<usize>,EngineError>,cache:Option<trueos_fs::FileRecordRef>,
 events:Vec<&'static str>,assert_tracked_phase:bool,
}
impl State {fn ready()->Self {Self{activity_error:false,activity_live:0,placement_ready:true,
 placement:Ok(Some(Placement{super_lba:1,data_lba:2,data_end_lba_exclusive:3})),index_ready:true,
 index:Ok(Some(trueos_fs::FileRecordRef)),read_ready:true,read:Ok(Some(3)),cache:None,events:Vec::new(),assert_tracked_phase:false}}}
static STATE:Mutex<Option<State>>=Mutex::new(None);
fn with_state<T>(f:impl FnOnce(&mut State)->T)->T {f(STATE.lock().unwrap().as_mut().unwrap())}
fn boundary(name:&'static str,phase:u32) {with_state(|s| {s.events.push(name);if s.assert_tracked_phase {assert_eq!(broker::observed_phase(),phase);}})}
mod disc {
 pub mod block {
  #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum Error {InvalidParam,Failure}
  #[derive(Clone,Copy)] pub struct DeviceHandle {pub parent:bool}
  impl DeviceHandle {pub fn parent(self)->Option<Self>{self.parent.then_some(self)}pub fn id(self)->u8{1}}
 }
 pub mod access {
  pub struct Activity;
  impl Activity {pub fn begin(_:super::block::DeviceHandle)->Result<Self,super::block::Error>{
   crate::boundary("activity",9);crate::with_state(|s| {if s.activity_error {Err(super::block::Error::Failure)}else{s.activity_live+=1;Ok(Self)}})
  }}
  impl Drop for Activity {fn drop(&mut self){crate::with_state(|s|{s.events.push("drop");s.activity_live-=1;});}}
 }
}
mod trueos_fs {
 #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub struct FileRecordRef;
 pub struct FsParams {pub super_lba:u64,pub data_lba:u64,pub data_end_lba_exclusive:u64}
 pub async fn read_file_range_at(_: &crate::trueosfs::KernelBlockIo,p:&FsParams,_:&FileRecordRef,offset:u64,out:&mut[u8])->Result<Option<usize>,crate::EngineError>{
  assert_eq!((p.super_lba,p.data_lba,p.data_end_lba_exclusive,offset),(1,2,3,7));
  core::future::poll_fn(|_|{crate::boundary("read",13);crate::with_state(|s|{
   if s.read_ready {if let Ok(Some(n))=s.read{out[..n].fill(7);}core::task::Poll::Ready(s.read)}else{core::task::Poll::Pending}
  })}).await
 }
}
mod trueosfs {
 use crate::{Placement as TrueosFsPlacement,trueos_fs,disc::block};
 pub struct KernelBlockIo;
 impl KernelBlockIo {fn new(_:block::DeviceHandle)->Self{Self}}
 fn map_engine_err(_:crate::EngineError)->block::Error{block::Error::Failure}
 async fn placement_for_io_async(_:block::DeviceHandle)->Result<Option<TrueosFsPlacement>,block::Error>{
  core::future::poll_fn(|_|{crate::boundary("placement",10);crate::with_state(|s|if s.placement_ready{core::task::Poll::Ready(s.placement)}else{core::task::Poll::Pending})}).await
 }
 fn file_record_cache_lookup(_:u8,_:&str)->Option<trueos_fs::FileRecordRef>{crate::boundary("cache_lookup",11);crate::with_state(|s|s.cache)}
 fn file_record_cache_insert(_:u8,_:&str,record:trueos_fs::FileRecordRef){crate::boundary("cache_insert",11);crate::with_state(|s|s.cache=Some(record));}
 async fn lookup_via_index_async(_:block::DeviceHandle,_:&TrueosFsPlacement,_:&str)->Result<Option<trueos_fs::FileRecordRef>,block::Error>{
  core::future::poll_fn(|_|{crate::boundary("index",12);crate::with_state(|s|if s.index_ready{core::task::Poll::Ready(s.index)}else{core::task::Poll::Pending})}).await
 }
 @PHASE_ENUM@
 @OBSERVED_HELPER@
 @PUBLIC_WRAPPER@
}
'''

BROKER_ADDITION = r'''
use crate::disc::block;
pub(super) fn observed_phase()->u32 {PROGRESS.sample(crate::CLOCK.load(Ordering::Relaxed),&mut 0,4).into_iter().flatten().next().unwrap().phase}
async fn read_range_arm(id:u64,disk:block::DeviceHandle,path:String,offset:u64,cap:usize,completion:Completion<Vec<u8>>,mut progress:progress::Ticket<'_>) {
 @RANGE_ARM@
}
fn poll<F:core::future::Future>(future:core::pin::Pin<&mut F>)->core::task::Poll<F::Output>{future.poll(&mut core::task::Context::from_waker(core::task::Waker::noop()))}
fn reset(state:crate::State){*crate::STATE.lock().unwrap()=Some(state);}
fn disk()->block::DeviceHandle{block::DeviceHandle{parent:false}}
fn observe(phase:crate::trueosfs::ReadRangePhase){
 use crate::trueosfs::ReadRangePhase::*;
 crate::with_state(|s|s.events.push(match phase{Placement=>"phase10",RecordCache=>"phase11",Index=>"phase12",Read=>"phase13"}));
}
#[test] fn range_broker_actual_arm_tracks_each_pending_boundary_and_completion() {
 let mut state=crate::State::ready();state.placement_ready=false;state.index_ready=false;state.read_ready=false;state.assert_tracked_phase=true;reset(state);
 PROGRESS.arm(0,10,20,0);let mut ticket=PROGRESS.admit(0,10,9001,4,||1);ticket.advance(progress::SERVICE,0,||2);ticket.advance(progress::FILESYSTEM,0,||3);
 crate::CLOCK.store(10,Ordering::Relaxed);let completion=Arc::new(CompletionCell::new(true));
 let mut future=Box::pin(read_range_arm(9001,disk(),String::from("host-fixture"),7,8,completion.clone(),ticket));
 assert!(poll(future.as_mut()).is_pending());assert_eq!(observed_phase(),10);
 // A repeated poll of the same awaited step does not repeat its callback.
 assert!(poll(future.as_mut()).is_pending());assert_eq!(observed_phase(),10);
 crate::with_state(|s|s.placement_ready=true);assert!(poll(future.as_mut()).is_pending());assert_eq!(observed_phase(),12);
 crate::with_state(|s|s.index_ready=true);assert!(poll(future.as_mut()).is_pending());assert_eq!(observed_phase(),13);
 let snapshot=PROGRESS.sample(11,&mut 0,4).into_iter().flatten().next().unwrap();assert_eq!((snapshot.begun,snapshot.queued,snapshot.status),(2,1,0));
 crate::with_state(|s|s.read_ready=true);assert!(poll(future.as_mut()).is_ready());
 assert_eq!(*completion.value.lock().unwrap(),Some(Ok(vec![7,7,7])));assert_eq!(crate::with_state(|s|s.activity_live),0);
 assert!(PROGRESS.sample(12,&mut 0,4).iter().all(Option::is_none));
}
#[test] fn range_inactive_ticket_and_public_noop_wrapper_do_not_read_clock() {
 let mut state=crate::State::ready();state.cache=Some(crate::trueos_fs::FileRecordRef);reset(state.clone());
 let before=crate::CLOCK_CALLS.load(Ordering::Relaxed);let completion=Arc::new(CompletionCell::new(false));
 let mut future=Box::pin(read_range_arm(9002,disk(),String::from("host-fixture"),7,8,completion.clone(),PROGRESS.inactive()));assert!(poll(future.as_mut()).is_ready());
 assert_eq!(*completion.value.lock().unwrap(),Some(Ok(vec![7,7,7])));assert_eq!(before,crate::CLOCK_CALLS.load(Ordering::Relaxed));
 reset(state);let mut out=[0;8];let mut future=Box::pin(crate::trueosfs::file_read_range_async(disk(),"host-fixture",7,&mut out));
 assert_eq!(poll(future.as_mut()),core::task::Poll::Ready(Ok(Some(3))));assert_eq!(before,crate::CLOCK_CALLS.load(Ordering::Relaxed));
 assert!(!crate::with_state(|s|s.events.iter().any(|e|e.starts_with("phase"))));
}
#[test] fn range_callbacks_stop_on_each_existing_error_and_missing_result() {
 for case in 0..7 {
  let mut state=crate::State::ready();let expected=match case {
   0=>{state.activity_error=true;(Err(block::Error::Failure),vec![])},
   1=>(Err(block::Error::InvalidParam),vec![]),
   2=>{state.placement=Err(block::Error::Failure);(Err(block::Error::Failure),vec!["phase10"])},
   3=>{state.placement=Ok(None);(Ok(None),vec!["phase10"])},
   4=>{state.index=Err(block::Error::Failure);(Err(block::Error::Failure),vec!["phase10","phase11","phase12"])},
   5=>{state.index=Ok(None);(Ok(None),vec!["phase10","phase11","phase12"])},
   _=>{state.read=Err(crate::EngineError::Failure);(Err(block::Error::Failure),vec!["phase10","phase11","phase12","phase11","phase13"])},
  };reset(state);let mut out=[0;8];let mut d=disk();d.parent=case==1;
  let mut future=Box::pin(crate::trueosfs::file_read_range_observed_async(d,"host-fixture",7,&mut out,observe));
  assert_eq!(poll(future.as_mut()),core::task::Poll::Ready(expected.0));drop(future);
  assert_eq!(crate::with_state(|s|s.events.iter().copied().filter(|e|e.starts_with("phase")).collect::<Vec<_>>()),expected.1);
  assert_eq!(crate::with_state(|s|s.activity_live),0);
 }
}
#[test] fn range_cached_hit_skips_index_and_preserves_callback_order() {
 let mut state=crate::State::ready();state.cache=Some(crate::trueos_fs::FileRecordRef);reset(state);
 let mut out=[0;8];let mut future=Box::pin(crate::trueosfs::file_read_range_observed_async(disk(),"host-fixture",7,&mut out,observe));
 assert_eq!(poll(future.as_mut()),core::task::Poll::Ready(Ok(Some(3))));drop(future);
 assert_eq!(crate::with_state(|s|s.events.clone()),vec!["activity","phase10","placement","phase11","cache_lookup","phase13","read","drop"]);
}
#[test] fn range_pending_future_keeps_activity_until_explicit_host_drop() {
 let mut state=crate::State::ready();state.index_ready=false;reset(state);
 let mut out=[0;8];let mut future=Box::pin(crate::trueosfs::file_read_range_observed_async(disk(),"host-fixture",7,&mut out,observe));
 assert!(poll(future.as_mut()).is_pending());assert_eq!(crate::with_state(|s|s.activity_live),1);drop(future);assert_eq!(crate::with_state(|s|s.activity_live),0);
}
#[test] fn range_expired_ticket_stops_all_later_subphase_clock_reads() {
 let mut state=crate::State::ready();state.cache=Some(crate::trueos_fs::FileRecordRef);reset(state);
 PROGRESS.arm(0,10,21,0);let ticket=PROGRESS.admit(0,10,9003,4,||1);
 crate::CLOCK.store(180001,Ordering::Relaxed);let before=crate::CLOCK_CALLS.load(Ordering::Relaxed);let expired=PROGRESS.expired.load(Ordering::Relaxed);
 let completion=Arc::new(CompletionCell::new(false));let mut future=Box::pin(read_range_arm(9003,disk(),String::from("host-fixture"),7,8,completion.clone(),ticket));
 assert!(poll(future.as_mut()).is_ready());assert_eq!(crate::CLOCK_CALLS.load(Ordering::Relaxed)-before,1);assert_eq!(PROGRESS.expired.load(Ordering::Relaxed)-expired,1);
 assert_eq!(*completion.value.lock().unwrap(),Some(Ok(vec![7,7,7])));
}
'''


def main():
    fs_source = (ROOT/'src/r/fs/trueosfs.rs').read_text()
    broker_source = (ROOT/'src/r/fs/request_broker.rs').read_text()
    helper = item(fs_source, 'pub(super) async fn file_read_range_observed_async(')
    wrapper = item(fs_source, 'pub async fn file_read_range_async(')
    enum = item(fs_source, 'pub(super) enum ReadRangePhase')
    body = helper[helper.index('{')+1:-1]
    stripped = re.sub(r'^\s*observe\(ReadRangePhase::(?:Placement|RecordCache|Index|Read)\);\n', '', body, flags=re.M)
    assert hashlib.sha256(stripped.encode()).hexdigest() == LEGACY_HELPER_BODY_SHA256, 'filesystem body changed beyond callbacks'
    arm = range_arm(broker_source)
    stripped = re.sub(r'^            progress.advance\(progress::RANGE_SCRATCH, 0, crate::r::input_timing::now_ms\);\n', '', arm, flags=re.M)
    stripped = stripped.replace('file_read_range_observed_async(', 'file_read_range_async(')
    stripped = re.sub(r'^                \|phase\| \{\n.*?^                \},\n', '', stripped, flags=re.M | re.S)
    assert hashlib.sha256(stripped.encode()).hexdigest() == LEGACY_BROKER_ARM_SHA256, 'broker arm changed beyond callbacks'
    assert 'file_read_range_observed_async(disk, name, offset, out, |_| {}).await' in wrapper
    assert arm.index('progress::RANGE_SCRATCH') < arm.index('let mut scratch')
    harness = base.head.replace('mod broker {', STUBS.replace('@PHASE_ENUM@','#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n'+enum).replace('@OBSERVED_HELPER@',helper).replace('@PUBLIC_WRAPPER@',wrapper)+'\nmod broker {')
    harness = harness.replace('type FsResult<T>=Result<T,()>;', '#[derive(Debug,PartialEq,Eq)] enum FsError{NotFound,Device(crate::disc::block::Error)}\nimpl From<crate::disc::block::Error> for FsError{fn from(e:crate::disc::block::Error)->Self{Self::Device(e)}}\ntype FsResult<T>=Result<T,FsError>;')
    harness += '\n'.join(base.parts) + base.tests[:-2] + BROKER_ADDITION.replace('@RANGE_ARM@',arm) + '\n}\n'
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', (ROOT/'rust-toolchain.toml').read_text(),re.M).group(1)
    with tempfile.TemporaryDirectory(prefix='trueos-native-range-progress-') as directory:
        work=Path(directory); (work/'tests.rs').write_text(harness)
        subprocess.run(['rustc','+'+channel,'--test','-Dwarnings','--edition=2024','--target','x86_64-unknown-linux-gnu',str(work/'tests.rs'),'-o',str(work/'tests')],cwd=work,check=True)
        subprocess.run([str(work/'tests'),'--test-threads=1'],cwd=work,check=True)
    print('Read-range callbacks and broker arm conserve their original bodies.')


if __name__ == '__main__':
    main()
