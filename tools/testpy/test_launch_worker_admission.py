#!/usr/bin/env python3
"""Compile production launch admission and its first-poll lease release on host."""
from pathlib import Path
import subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2]
s=(ROOT/'src/hv/launcher.rs').read_text()
start=s.index('pub(crate) fn enqueue(');end=s.index('\n#[trueos_executor::task',start)
enqueue=s[start:end]
prefix=s.split('queue_carrier: super::lane::LaneLease,',1)[1].split('    let live =',1)[0]
prefix=prefix.split(') {',1)[1]
source=r'''
#![allow(dead_code,unused_variables,unused_imports)]
extern crate alloc;
use std::{sync::{Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}},future::Future,pin::Pin};
#[macro_export] macro_rules! log_info {($($t:tt)*)=>{{}}}
mod trueos_time {pub struct Instant;pub struct Duration;impl Instant{pub fn now()->Self{Self}pub fn elapsed(&self)->Duration{Duration}}impl Duration{pub fn as_millis(&self)->u64{0}}}
static LEASED:AtomicBool=AtomicBool::new(false);static BUSY:AtomicBool=AtomicBool::new(false);
static POOL_BUSY:AtomicBool=AtomicBool::new(false);static ATTACH_FAIL:AtomicBool=AtomicBool::new(false);
static RESERVED:AtomicUsize=AtomicUsize::new(0);static DISCARDED:AtomicUsize=AtomicUsize::new(0);
static PARKED:AtomicUsize=AtomicUsize::new(0);static QUEUED:Mutex<Vec<Task>>=Mutex::new(Vec::new());
static EXPECT_ORIGIN:AtomicBool=AtomicBool::new(false);static ORIGIN_BOUND:AtomicBool=AtomicBool::new(false);
struct Task(Pin<Box<dyn Future<Output=()>+Send>>);struct Spawner;
impl Spawner{fn spawn(self,task:Task){assert!(LEASED.load(Ordering::SeqCst));assert_eq!(EXPECT_ORIGIN.load(Ordering::SeqCst),ORIGIN_BOUND.load(Ordering::SeqCst));QUEUED.lock().unwrap().push(task)}}
mod r{pub mod spawn_spec{pub enum SpawnPlacement{Worker}}}
mod shell3{#[derive(Clone)]pub struct MatrixTarget;pub mod tui{
 #[derive(Clone,Copy)]pub struct Frontend;
 #[derive(Clone,Copy)]pub struct RemoteInputOrigin;
 pub fn attach(_:Frontend,_:&super::MatrixTarget)->Result<(),String>{if crate::ATTACH_FAIL.load(crate::Ordering::SeqCst){Err("attach failed".into())}else{Ok(())}}
 pub fn bind_remote_input_origin(_:&super::MatrixTarget,_:RemoteInputOrigin){assert!(crate::QUEUED.lock().unwrap().is_empty());crate::ORIGIN_BOUND.store(true,crate::Ordering::SeqCst);}
 pub fn park_native_for_launch(_:Frontend){crate::PARKED.fetch_add(1,crate::Ordering::SeqCst);}
}}
mod hv{
 pub mod blueprint{pub fn prebind_required_readiness(bytes:&[u8])->Result<(u32,Vec<u8>),String>{Ok((0,bytes.into()))}}
 pub mod matrix{
  pub struct AppReceipt;
  pub fn reserve(_:&str,_:&[u8])->Result<(crate::shell3::MatrixTarget,AppReceipt),String>{crate::RESERVED.fetch_add(1,crate::Ordering::SeqCst);Ok((crate::shell3::MatrixTarget,AppReceipt))}
  pub fn observe_completion(_:&crate::shell3::MatrixTarget){}
  pub fn discard(_:&crate::shell3::MatrixTarget){crate::DISCARDED.fetch_add(1,crate::Ordering::SeqCst);}
 }
 pub mod lane{
  pub enum LaneRole{Worker}pub struct LaneProfile{pub role:LaneRole,pub placement:crate::r::spawn_spec::SpawnPlacement}
  pub struct LaneLease;impl Drop for LaneLease{fn drop(&mut self){crate::LEASED.store(false,crate::Ordering::SeqCst)}}
  pub struct LaneTarget{pub slot:u32,pub spawner:crate::Spawner,pub lease:LaneLease}
  pub struct Error;impl Error{pub fn as_str(&self)->&str{"carrier busy"}}
  pub fn pick_carrier_lane(_:LaneProfile)->Result<LaneTarget,Error>{if crate::BUSY.load(crate::Ordering::SeqCst)||crate::LEASED.swap(true,crate::Ordering::SeqCst){Err(Error)}else{Ok(LaneTarget{slot:6,spawner:crate::Spawner,lease:LaneLease})}}
 }
 pub mod launch_timing{
  pub struct Trace;pub struct Span;
  impl Trace{pub fn for_archive(_:&str)->Self{Self}pub fn measure<T>(&self,_:&str,f:impl FnOnce()->T)->T{f()}pub fn span(&self,_:&str)->Span{Span}}
  impl Span{pub fn complete(&mut self){assert!(!crate::LEASED.load(crate::Ordering::SeqCst))}}
 }
 pub mod launcher{
 use alloc::{boxed::Box,string::String,vec::Vec};use crate::trueos_time;
 pub struct LaunchRequest{archive:String,bytes:Vec<u8>,frontend:Option<crate::shell3::tui::Frontend>,remote_input_origin:Option<crate::shell3::tui::RemoteInputOrigin>}
 pub trait LaunchObserver:Send+Sync{}
 @ENQUEUE@
 fn launch_task(_:LaunchRequest,_:crate::shell3::MatrixTarget,_:u32,_:Vec<u8>,_:Option<Box<dyn LaunchObserver>>,_:super::launch_timing::Trace,mut queue:super::launch_timing::Span,queue_carrier:super::lane::LaneLease)->Result<crate::Task,()>{
  if crate::POOL_BUSY.load(crate::Ordering::SeqCst){return Err(())}
  Ok(crate::Task(Box::pin(async move{@PREFIX@ std::future::pending::<()>().await})))
 }
 pub fn launch()->Result<super::matrix::AppReceipt,String>{launch_with_origin(false)}
 pub fn launch_with_origin(origin:bool)->Result<super::matrix::AppReceipt,String>{crate::EXPECT_ORIGIN.store(origin,crate::Ordering::SeqCst);enqueue(LaunchRequest{archive:"hull.bp".into(),bytes:vec![1],frontend:Some(crate::shell3::tui::Frontend),remote_input_origin:origin.then_some(crate::shell3::tui::RemoteInputOrigin)},None)}
 }
}
fn reset(){QUEUED.lock().unwrap().clear();for a in [&LEASED,&BUSY,&POOL_BUSY,&ATTACH_FAIL,&EXPECT_ORIGIN,&ORIGIN_BOUND]{a.store(false,Ordering::SeqCst)}for a in [&RESERVED,&DISCARDED,&PARKED]{a.store(0,Ordering::SeqCst)}}
#[test]fn busy_native_carrier_is_rejected_before_route_reservation(){reset();BUSY.store(true,Ordering::SeqCst);assert!(hv::launcher::launch().is_err());assert_eq!(RESERVED.load(Ordering::SeqCst),0);assert!(!LEASED.load(Ordering::SeqCst));assert!(QUEUED.lock().unwrap().is_empty())}
#[test]fn lease_covers_queue_then_releases_on_first_poll(){reset();assert!(hv::launcher::launch().is_ok());assert!(LEASED.load(Ordering::SeqCst));assert_eq!(PARKED.load(Ordering::SeqCst),1);let mut task=QUEUED.lock().unwrap().pop().unwrap();assert!(task.0.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())).is_pending());assert!(!LEASED.load(Ordering::SeqCst));}
#[test]fn failed_task_pool_rolls_back_route_and_lease(){reset();POOL_BUSY.store(true,Ordering::SeqCst);assert!(hv::launcher::launch().is_err());assert_eq!(DISCARDED.load(Ordering::SeqCst),1);assert_eq!(PARKED.load(Ordering::SeqCst),0);assert!(!LEASED.load(Ordering::SeqCst))}
#[test]fn failed_attachment_rolls_back_route_and_lease(){reset();ATTACH_FAIL.store(true,Ordering::SeqCst);assert!(hv::launcher::launch().is_err());assert_eq!(DISCARDED.load(Ordering::SeqCst),1);assert_eq!(PARKED.load(Ordering::SeqCst),0);assert!(!LEASED.load(Ordering::SeqCst))}
#[test]fn remote_origin_is_installed_before_guest_task_can_poll(){reset();assert!(hv::launcher::launch_with_origin(true).is_ok());assert!(ORIGIN_BOUND.load(Ordering::SeqCst));assert_eq!(QUEUED.lock().unwrap().len(),1);}
#[test]fn ordinary_launch_never_acquires_remote_control_origin(){reset();assert!(hv::launcher::launch().is_ok());assert!(!ORIGIN_BOUND.load(Ordering::SeqCst));}
'''.replace('@ENQUEUE@',enqueue).replace('@PREFIX@',prefix)
with tempfile.TemporaryDirectory(prefix='launch-worker-admission-') as d:
 path=Path(d)/'test.rs';path.write_text(source);exe=Path(d)/'test'
 subprocess.run(['rustc','--edition=2024','--test',str(path),'-o',str(exe)],check=True)
 subprocess.run([str(exe),'--test-threads=1'],check=True)
