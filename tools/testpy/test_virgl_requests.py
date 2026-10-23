#!/usr/bin/env python3
"""Exercise the actual virgl request queue's ownership, wakeups and shutdown."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
extern crate alloc;
extern crate self as spin;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
mod wait {
    use std::sync::{Mutex,Condvar,atomic::{AtomicU32,Ordering}};
    pub struct WaitQueue {
        seq:AtomicU32, gate:Mutex<()>, event:Condvar,
        waker:Mutex<Option<std::task::Waker>>,
    }
    impl WaitQueue {
        pub const fn new()->Self {Self {seq:AtomicU32::new(0),gate:Mutex::new(()),
            event:Condvar::new(),waker:Mutex::new(None)}}
        pub fn observe(&self)->u32 {self.seq.load(Ordering::Acquire)}
        pub fn notify_all(&self) {
            let _gate=self.gate.lock().unwrap();
            self.seq.fetch_add(1,Ordering::Release);
            if let Some(waker)=self.waker.lock().unwrap().take(){waker.wake()}
            self.event.notify_all();
        }
        pub async fn wait_after(&self,observed:u32) {
            std::future::poll_fn(|cx| {
                let _gate=self.gate.lock().unwrap();
                if self.observe()!=observed {std::task::Poll::Ready(())}
                else {*self.waker.lock().unwrap()=Some(cx.waker().clone());std::task::Poll::Pending}
            }).await
        }
        pub fn wait_for_event_after_blocking_parked(&self,observed:u32,_:u64) {
            let mut gate=self.gate.lock().unwrap();
            while self.observe()==observed {gate=self.event.wait(gate).unwrap()}
        }
    }
}
mod virtio_gpu_logo {
    pub struct VirtioGpuLogo {pub failed:bool}
    mod virgl {pub struct Compositor;}
    #[path=__SOURCE__] mod requests;
    #[cfg(test)] mod tests {
        use super::*;
        use std::{future::Future,pin::Pin,sync::{Arc,atomic::{AtomicUsize,Ordering}},task::{Context,Poll,Wake,Waker}};
        static SERIAL:std::sync::Mutex<()>=std::sync::Mutex::new(());
        struct WakeCounter(AtomicUsize);
        impl Wake for WakeCounter {fn wake(self:Arc<Self>){self.0.fetch_add(1,Ordering::Relaxed);}}
        fn poll<F:Future>(future:Pin<&mut F>)->Poll<F::Output> {
            future.poll(&mut Context::from_waker(Waker::noop()))
        }
        fn service(gpu:&mut VirtioGpuLogo) {requests::service(&mut virgl::Compositor,gpu)}
        struct Dropped(Arc<AtomicUsize>);
        impl Drop for Dropped {fn drop(&mut self){self.0.fetch_add(1,Ordering::SeqCst);}}

        #[test] fn cancellation_retains_owned_inputs_and_never_publishes_to_destination() {
            let _serial=SERIAL.lock().unwrap();requests::stop();requests::start();
            let inputs=Arc::new(AtomicUsize::new(0));let outputs=Arc::new(AtomicUsize::new(0));
            let destination=Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
            let owned=vec![3u8,7,11];let input_guard=Dropped(inputs.clone());
            let output_counter=outputs.clone();let target=destination.clone();
            let mut caller=Box::pin(async move {
                let (pixels,_output_guard)=requests::dispatch(move |_,_| {
                    assert_eq!(owned,[3,7,11]);drop(input_guard);
                    Ok((owned,Dropped(output_counter)))
                }).await.unwrap();
                *target.lock().unwrap()=pixels;
            });
            assert!(poll(caller.as_mut()).is_pending());drop(caller);
            assert_eq!(inputs.load(Ordering::SeqCst),0);
            service(&mut VirtioGpuLogo {failed:false});
            assert_eq!(inputs.load(Ordering::SeqCst),1);
            assert_eq!(outputs.load(Ordering::SeqCst),1);
            assert!(destination.lock().unwrap().is_empty());requests::stop();
        }

        #[test] fn async_completion_wakes_waiter_and_returns_owned_result() {
            let _serial=SERIAL.lock().unwrap();requests::stop();requests::start();
            let count=Arc::new(WakeCounter(AtomicUsize::new(0)));let waker=Waker::from(count.clone());
            let mut work=Box::pin(requests::dispatch(|_,_|Ok(vec![1u8,2,3])));
            assert!(work.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
            service(&mut VirtioGpuLogo {failed:false});
            assert!(count.0.load(Ordering::Relaxed)>0);
            assert_eq!(poll(work.as_mut()),Poll::Ready(Ok(vec![1,2,3])));requests::stop();
        }

        #[test] fn capacity_is_bounded_and_stop_drains_every_waiter_without_running_operations() {
            let _serial=SERIAL.lock().unwrap();requests::stop();requests::start();
            let calls=Arc::new(AtomicUsize::new(0));let mut waiters=Vec::new();
            for index in 0..32u32 {
                let calls=calls.clone();let mut work=Box::pin(requests::dispatch(move |_,_| {
                    calls.fetch_add(1,Ordering::SeqCst);Ok(index)
                }));
                assert!(poll(work.as_mut()).is_pending());waiters.push(work);
            }
            let mut overflow=Box::pin(requests::dispatch(|_,_|Ok(99u32)));
            assert_eq!(poll(overflow.as_mut()),Poll::Ready(Err("virgl-offscreen-busy")));
            requests::stop();assert!(!requests::available());
            for mut work in waiters {assert_eq!(poll(work.as_mut()),Poll::Ready(Err("virgl-offscreen-device-failed")))}
            assert_eq!(calls.load(Ordering::SeqCst),0);
            let mut stopped=Box::pin(requests::dispatch(|_,_|Ok(7u32)));
            assert_eq!(poll(stopped.as_mut()),Poll::Ready(Err("virgl-offscreen-unavailable")));
        }

        #[test] fn service_bounds_each_turn_and_rejects_work_after_gpu_failure() {
            let _serial=SERIAL.lock().unwrap();requests::stop();requests::start();
            let calls=Arc::new(AtomicUsize::new(0));let mut waiters=Vec::new();
            for index in 0..6u32 {
                let calls=calls.clone();let mut work=Box::pin(requests::dispatch(move |_,_| {
                    calls.fetch_add(1,Ordering::SeqCst);Ok(index)
                }));
                assert!(poll(work.as_mut()).is_pending());waiters.push(work);
            }
            service(&mut VirtioGpuLogo {failed:false});assert_eq!(calls.load(Ordering::SeqCst),4);
            for (index,work) in waiters.iter_mut().enumerate() {
                if index<4 {assert_eq!(poll(work.as_mut()),Poll::Ready(Ok(index as u32)))}
                else {assert!(poll(work.as_mut()).is_pending())}
            }
            service(&mut VirtioGpuLogo {failed:true});assert_eq!(calls.load(Ordering::SeqCst),4);
            for work in &mut waiters[4..] {assert_eq!(poll(work.as_mut()),Poll::Ready(Err("virgl-offscreen-device-failed")))}
            requests::stop();
        }

        #[test] fn blocking_worker_completes_when_another_thread_services_the_queue() {
            let _serial=SERIAL.lock().unwrap();requests::stop();requests::start();
            let (sent,received)=std::sync::mpsc::channel();
            let worker=std::thread::spawn(move || {
                sent.send(requests::dispatch_blocking(|_,_|Ok(vec![13u8,17]))).unwrap();
            });
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
            let result=loop {
                service(&mut VirtioGpuLogo {failed:false});
                if let Ok(value)=received.try_recv(){break value}
                assert!(std::time::Instant::now()<deadline,"blocking dispatch never completed");
                std::thread::yield_now();
            };
            assert_eq!(result,Ok(vec![13,17]));worker.join().unwrap();requests::stop();
        }
    }
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-virgl-requests-") as directory:
        harness = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        harness.write_text(HARNESS.replace(
            "__SOURCE__", json.dumps(str(ROOT / "src/virtio_gpu_logo/requests.rs"))))
        subprocess.run([os.environ.get("RUSTC", "rustc"), "--edition=2024", "--test",
                        str(harness), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True, timeout=15)


if __name__ == "__main__":
    main()
