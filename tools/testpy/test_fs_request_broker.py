#!/usr/bin/env python3
"""Exercise the production BSP broker pump with a held stream write lease.

Only request execution and wait notifications are mocked. A competing writer
must remain pending without blocking the chunk and finish/abort that release
the lease. Poll the actual service future, including its idle wake handling.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/r/fs/request_broker.rs").read_text()
    pump = source[source.index("type ActiveRequest ="):]
    pump = pump.replace("#[trueos_executor::task]\n", "")
    pump = pump.replace("crate::log_info!", "log_info!")
    harness = r'''
use std::boxed::Box;
use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex as StdMutex};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
macro_rules! log_info { ($($args:tt)*) => {} }
struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self { Self(StdMutex::new(value)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
static BROKER_ONLINE:AtomicBool=AtomicBool::new(false);
static BROKER_REQUESTS:Mutex<VecDeque<Request>>=Mutex::new(VecDeque::new());
struct WaitQueue { seq:AtomicU32, waker:Mutex<Option<Waker>> }
impl WaitQueue {
    fn notify(&self) {
        self.seq.fetch_add(1,Ordering::Release);
        let waker=self.waker.lock().take();
        if let Some(waker)=waker { waker.wake(); }
    }
    async fn wait_for_event(&self) {
        let observed=self.seq.load(Ordering::Acquire);
        poll_fn(|cx| {
            *self.waker.lock()=Some(cx.waker().clone());
            if self.seq.load(Ordering::Acquire)!=observed { Poll::Ready(()) }
            else { Poll::Pending }
        }).await
    }
}
static BROKER_WAIT:WaitQueue=WaitQueue { seq:AtomicU32::new(0), waker:Mutex::new(None) };
enum Request { Begin, Directory, Chunk, Finish, Abort, Read }
#[derive(Default)]
struct State { lease:bool, chunks:usize, directory_done:bool, reads:usize, waiting:Option<Waker> }
static STATE:Mutex<State>=Mutex::new(State { lease:false, chunks:0, directory_done:false, reads:0, waiting:None });
async fn process_request(request:Request) {
    match request {
        Request::Begin => { STATE.lock().lease=true; }
        Request::Directory => {
            poll_fn(|cx| {
                let mut state=STATE.lock();
                if state.lease { state.waiting=Some(cx.waker().clone()); Poll::Pending }
                else { state.directory_done=true; Poll::Ready(()) }
            }).await;
        }
        Request::Chunk => { let mut s=STATE.lock(); assert!(s.lease); s.chunks+=1; }
        Request::Finish | Request::Abort => {
            let waker={ let mut s=STATE.lock(); assert!(s.lease); s.lease=false; s.waiting.take() };
            if let Some(waker)=waker { waker.wake(); }
        }
        Request::Read => { STATE.lock().reads+=1; }
    }
}
fn queue_retired_write_aborts() {}
#[derive(Default)] struct WakeCount(AtomicUsize);
impl Wake for WakeCount { fn wake(self:Arc<Self>) { self.0.fetch_add(1,Ordering::Relaxed); } }
fn enqueue(request:Request) { BROKER_REQUESTS.lock().push_back(request); BROKER_WAIT.notify(); }
fn check_release(release:Request) {
    *STATE.lock()=State::default();
    BROKER_REQUESTS.lock().clear();
    let count=Arc::new(WakeCount::default());
    let waker=Waker::from(count.clone());
    let mut cx=Context::from_waker(&waker);
    let mut service=Box::pin(service_task());
    assert!(service.as_mut().poll(&mut cx).is_pending());
    enqueue(Request::Begin);
    assert!(count.0.load(Ordering::Relaxed)>0,"idle broker must wake on arrival");
    assert!(service.as_mut().poll(&mut cx).is_pending());
    assert!(STATE.lock().lease);
    enqueue(Request::Directory);
    assert!(service.as_mut().poll(&mut cx).is_pending());
    assert!(!STATE.lock().directory_done);
    enqueue(Request::Read);
    enqueue(Request::Chunk);
    assert!(service.as_mut().poll(&mut cx).is_pending());
    assert_eq!(STATE.lock().reads,1,"read must pass a waiting writer");
    assert_eq!(STATE.lock().chunks,1,"stream must pass a waiting writer");
    assert!(!STATE.lock().directory_done);
    enqueue(release);
    assert!(service.as_mut().poll(&mut cx).is_pending());
    assert!(service.as_mut().poll(&mut cx).is_pending());
    assert!(STATE.lock().directory_done,"write release must unblock competing writer");
}
#[test] fn finish_passes_a_writer_waiting_for_its_stream() { check_release(Request::Finish); }
#[test] fn abort_passes_a_writer_waiting_for_its_stream() { check_release(Request::Abort); }
'''
    with tempfile.TemporaryDirectory(prefix="trueos-fs-broker-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(harness + pump)
        subprocess.run(["rustc", "--edition=2024", "--test", "--target",
                        "x86_64-unknown-linux-gnu", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
