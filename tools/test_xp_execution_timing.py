#!/usr/bin/env python3
"""Exercise the production carrier timing/handoff code with synthetic guest work."""
from pathlib import Path
import argparse
import subprocess
import tempfile
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--allocations', action='store_true', help='check allocation-free warmed carrier submissions')
args = parser.parse_args()
ROOT = Path(__file__).resolve().parents[1]
SDK = ROOT.parent / 'TRUEOS-Blueprints/api/src'
source = (SDK / 'x86.rs').read_text()
start = source.index('#[derive(Clone, Copy, Debug, Default)]\npub struct CarrierTiming')
end = source.index('\n#[cfg(test)]\nmod tests', start)
code = '''#![allow(dead_code,unexpected_cfgs)]
extern crate alloc;
use alloc::sync::Arc;
mod v { pub mod vx86 {
#[derive(Debug,Default)] pub struct Exit;
pub fn context_run(_:u64)->Result<Exit,i32>{Ok(Exit)}
pub fn context_resume(_:u64)->Result<Exit,i32>{Ok(Exit)}
}}
#[derive(Debug,PartialEq)] enum Error { CarrierUnavailable, CarrierLost, Kernel(i32) }
impl Error { fn from_kernel(e:i32)->Self {Self::Kernel(e)} }
type Exit = v::vx86::Exit;
'''
code += f'#[path="{SDK / "worker.rs"}"] mod worker;\n'
code += source[start:end]
code += '''
fn carrier_pair() -> (ExecutionCarrier, CarrierEndpoint) {
    let (requests, receiver) = tokio::sync::mpsc::channel(1);
    let completion = Arc::new(Completion::new());
    let endpoint = CarrierEndpoint { requests: receiver, completion: Arc::clone(&completion) };
    (ExecutionCarrier { requests, submit_lock: tokio::sync::Mutex::new(()), completion }, endpoint)
}
#[tokio::test]
async fn native_time_is_separate_from_request_queue_time() {
    let (carrier, endpoint) = carrier_pair();
    let task = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        carrier_requests(endpoint, |_, _| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            Ok(v::vx86::Exit)
        }, tokio::time::Instant::now).await;
    });
    let (_, t) = carrier.execute_measured(1, false).await.unwrap();
    assert!(t.request_ns >= 20_000_000, "{:?}", t);
    assert!(t.native_ns >= 10_000_000, "{:?}", t);
    drop(carrier);
    tokio::time::timeout(std::time::Duration::from_secs(1), task).await.unwrap().unwrap();
}
#[tokio::test]
async fn measurement_preserves_errors_and_abandoned_requests() {
    let (carrier, endpoint) = carrier_pair();
    let generation = carrier.completion.start().unwrap();
    carrier.requests.send(CarrierRequest { handle: 99, resume: false, measured: true, generation }).await.unwrap();
    carrier.completion.cancel(generation);
    let task = tokio::spawn(carrier_requests(endpoint, |handle, _| {
        assert_ne!(handle, 99);
        Err(-125)
    }, || panic!("unmeasured or abandoned request sampled the clock")));
    assert_eq!(carrier.execute(1, true).await.unwrap_err(), Error::Kernel(-125));
    drop(carrier);
    tokio::time::timeout(std::time::Duration::from_secs(1), task).await.unwrap().unwrap();
}
'''
# Allocation accounting runs alone: other test tasks must not contribute.
if args.allocations:
    code += r'''
use std::sync::atomic::{AtomicUsize,Ordering};
static ALLOCATIONS:AtomicUsize=AtomicUsize::new(0);
struct Counter;
unsafe impl std::alloc::GlobalAlloc for Counter {
 unsafe fn alloc(&self,l:std::alloc::Layout)->*mut u8 {ALLOCATIONS.fetch_add(1,Ordering::Relaxed); unsafe{std::alloc::System.alloc(l)}}
 unsafe fn dealloc(&self,p:*mut u8,l:std::alloc::Layout){unsafe{std::alloc::System.dealloc(p,l)}}
 unsafe fn realloc(&self,p:*mut u8,l:std::alloc::Layout,n:usize)->*mut u8 {ALLOCATIONS.fetch_add(1,Ordering::Relaxed);unsafe{std::alloc::System.realloc(p,l,n)}}
}
#[global_allocator] static COUNTER:Counter=Counter;
#[tokio::test]
async fn warmed_carrier_allocation_count() {
 let (carrier,endpoint)=carrier_pair();
 let task=tokio::spawn(carrier_requests(endpoint,|_,_|Ok(v::vx86::Exit),tokio::time::Instant::now));
 for measured in [false, true] {
  for _ in 0..128 {carrier.submit(1,true,measured).await.unwrap();}
  let before=ALLOCATIONS.load(Ordering::Relaxed);
  for _ in 0..4096 {carrier.submit(1,true,measured).await.unwrap();}
  let allocations=ALLOCATIONS.load(Ordering::Relaxed)-before;
  println!("WARM_CARRIER samples=4096 measured={measured} allocations={allocations}");
  assert_eq!(allocations,0,"warmed submit must not allocate a reply channel");
 }
 drop(carrier); task.await.unwrap();
}
'''

with tempfile.TemporaryDirectory(prefix='xp-timing-') as d:
    root = Path(d); (root/'src').mkdir()
    (root/'src/lib.rs').write_text(code)
    (root/'Cargo.toml').write_text('[package]\nname="xp-timing-tests"\nversion="0.1.0"\nedition="2024"\n[dependencies]\ntokio={version="=1.52.3",features=["sync","rt","macros","time"]}\n[workspace]\n')
    command = ['cargo','test','--offline','--target','x86_64-unknown-linux-gnu','--target-dir',str(ROOT/'bld/xp-timing-host-tests')]
    if args.allocations:
        command += ['warmed_carrier_allocation_count', '--', '--nocapture']
    subprocess.run(command,cwd=root,check=True)
