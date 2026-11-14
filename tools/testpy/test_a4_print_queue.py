#!/usr/bin/env python3
"""Execute production A4 PWG validation and print admission on the host.

The kernel's clock, log sink, mutex and VNet socket adapters are host fixtures.
Production admission, validation and IPP client modules execute unchanged.
No live network connection or physical printing is performed.
"""

from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_time;
extern crate self as v;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self { Self(CLOCK.with(|clock| clock.get())) }
    pub fn as_millis(&self) -> u64 { self.0 }
}
thread_local! { static CLOCK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
pub struct Duration(u64);
impl Duration { pub fn from_millis(value:u64)->Self { Self(value) } }
impl core::ops::Add<Duration> for Instant {
    type Output=Self;
    fn add(self,rhs:Duration)->Self { Self(self.0+rhs.0) }
}
pub struct Timer;
impl Timer {
    pub async fn after(duration:Duration) { CLOCK.with(|clock| clock.set(clock.get()+duration.0)); }
}
pub mod vnet {
    pub const MAX_MSG:usize=32*1024;
    #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub struct NetHandle(pub u32);
    #[derive(Clone,Copy,Debug)] pub struct EndpointV4 { pub addr:[u8;4], pub port:u16 }
    impl EndpointV4 { pub fn new(addr:[u8;4],port:u16)->Self { Self{addr,port} } }
    pub enum Command { OpenTcpConnect{remote:EndpointV4}, Close{handle:NetHandle} }
    pub enum Event {
        TcpEstablished{handle:NetHandle}, TcpSent{handle:NetHandle,len:u16},
        TcpData{handle:NetHandle,data:Vec<u8>}, Closed{handle:NetHandle}, Error{msg:&'static str}
    }
}
mod log_os { pub fn print2d_job_state(_:u32, _: &str, _: &str) {} }
mod r {
    pub mod net {
        pub(crate) use crate::fixture::VNet;
        #[path="__PWG__"] pub mod pwg_raster;
        #[path="__IPP__"] pub mod ipp_print;
    }
    #[path="__QUEUE__"] pub mod print2d;
}
mod fixture {
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};
    use crate::vnet::{self,Command,Event,NetHandle};
    #[derive(Default)]
    struct State {
        events:VecDeque<Event>, wire:Vec<u8>, requests:Vec<Vec<u8>>,
        backpressure:usize, accepted:usize, fail_after:Option<usize>, no_credit:bool,
        closes:usize, response_before_credit:bool,
    }
    thread_local! { static CURRENT: RefCell<Rc<RefCell<State>>> = RefCell::new(Rc::new(RefCell::new(State::default()))); }
    pub struct VNet(Rc<RefCell<State>>);
    impl VNet {
        pub fn open_primary()->Option<Self> { Some(CURRENT.with(|state| Self(Rc::clone(&state.borrow())))) }
        pub fn submit(&self,command:Command)->Result<(),()> {
            let mut state=self.0.borrow_mut();
            match command {
                Command::OpenTcpConnect{..}=>state.events.push_back(Event::TcpEstablished{handle:NetHandle(1)}),
                Command::Close{..}=>{state.closes+=1;},
            }
            Ok(())
        }
        pub fn pop_event(&self)->Option<Event> { self.0.borrow_mut().events.pop_front() }
        pub fn try_send_tcp_chunk(&self,handle:NetHandle,data:&[u8])->Result<(),()> {
            let mut state=self.0.borrow_mut();
            assert!(data.len()<=vnet::MAX_MSG);
            if state.backpressure!=0 {state.backpressure-=1;return Err(());}
            // This fixture deliberately admits only one command at a time.
            if state.events.iter().any(|event|matches!(event,Event::TcpSent{..})) {return Err(());}
            state.accepted+=data.len();
            state.wire.extend_from_slice(data);
            if state.fail_after.is_some_and(|limit|state.accepted>limit) {
                state.events.push_back(Event::Error{msg:"transport lost after accepted prefix"});return Ok(());
            }
            if state.no_credit {return Ok(());}
            let first=(data.len()/2).max(1);
            state.events.push_back(Event::TcpSent{handle,len:first as u16});
            if first<data.len(){state.events.push_back(Event::TcpSent{handle,len:(data.len()-first)as u16});}
            let end=state.wire.windows(4).position(|bytes|bytes==b"\r\n\r\n").map(|at|at+4);
            if let Some(end)=end {
                let headers=std::str::from_utf8(&state.wire[..end]).unwrap();
                let length:usize=headers.lines().find_map(|line|line.strip_prefix("Content-Length: ")).unwrap().trim().parse().unwrap();
                if state.wire.len()==end+length {
                    let operation=u16::from_be_bytes([state.wire[end+2],state.wire[end+3]]);
                    let mut body=vec![2,0,0,0,0,0,0,1,2];
                    let (name,value)=if operation==2 {(b"job-id".as_slice(),17i32)}else{(b"job-state".as_slice(),9i32)};
                    body.push(0x21);body.extend_from_slice(&(name.len()as u16).to_be_bytes());body.extend_from_slice(name);
                    body.extend_from_slice(&4u16.to_be_bytes());body.extend_from_slice(&value.to_be_bytes());body.push(3);
                    let mut response=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",body.len()).into_bytes();response.extend_from_slice(&body);
                    let wire=std::mem::take(&mut state.wire);state.requests.push(wire);
                    let response=Event::TcpData{handle,data:response};
                    if state.response_before_credit {state.events.push_front(response);}else{state.events.push_back(response);}
                }
            }
            Ok(())
        }
    }
    fn setup(backpressure:usize,fail_after:Option<usize>,no_credit:bool)->Rc<RefCell<State>> {
        crate::CLOCK.with(|clock|clock.set(0));
        let state=Rc::new(RefCell::new(State{backpressure,fail_after,no_credit,..State::default()}));
        CURRENT.with(|current|*current.borrow_mut()=Rc::clone(&state));state
    }
    fn run<F:core::future::Future>(future:F)->F::Output {
        let mut future=std::pin::pin!(future);
        match future.as_mut().poll(&mut core::task::Context::from_waker(core::task::Waker::noop())) {
            core::task::Poll::Ready(result)=>result,
            core::task::Poll::Pending=>panic!("all fixture waits are ready"),
        }
    }
    #[test] fn native_ipp_streams_large_documents_with_backpressure_and_exact_credit() {
        let state=setup(3,None,false);
        let client=crate::r::net::ipp_print::IppClient::new("ipp://192.0.2.1:631/ipp/print").unwrap();
        let bytes:Vec<_>=(0..3*vnet::MAX_MSG+17).map(|n|(n%251)as u8).collect();
        let job=run(client.print_job(5,"image/pwg-raster",&bytes)).unwrap();assert_eq!(job.id,17);
        assert_eq!(run(client.job_state(5,&job)).unwrap(),crate::r::net::ipp_print::RemoteJobState::Completed);
        let state=state.borrow();assert_eq!(state.requests.len(),2);assert_eq!(state.closes,2);
        assert!(state.requests[0].ends_with(&bytes));
    }
    #[test] fn native_ipp_never_retries_an_accepted_prefix_after_transport_failure() {
        let state=setup(0,Some(2000),false);
        let client=crate::r::net::ipp_print::IppClient::new("ipp://192.0.2.1:631/ipp/print").unwrap();
        let error=run(client.print_job(5,"image/pwg-raster",&vec![0;2*vnet::MAX_MSG])).err().unwrap();
        assert_eq!(error,crate::r::net::ipp_print::IppError::Transport);assert!(!error.submission_retryable());
        let state=state.borrow();assert_eq!(state.closes,1);assert!(state.accepted<2*vnet::MAX_MSG);
    }
    #[test] fn native_ipp_preserves_response_racing_final_send_credit() {
        let state=setup(0,None,false);state.borrow_mut().response_before_credit=true;
        let client=crate::r::net::ipp_print::IppClient::new("ipp://192.0.2.1:631/ipp/print").unwrap();
        let job=run(client.print_job(5,"image/pwg-raster",b"payload")).unwrap();
        assert_eq!(job.id,17);assert_eq!(state.borrow().requests.len(),1);
    }
    #[test] fn native_ipp_times_out_without_replaying_unacknowledged_bytes() {
        let state=setup(0,None,true);
        let client=crate::r::net::ipp_print::IppClient::new("ipp://192.0.2.1:631/ipp/print").unwrap();
        let error=run(client.print_job(5,"image/pwg-raster",b"payload")).err().unwrap();
        assert_eq!(error,crate::r::net::ipp_print::IppError::ResponseTimeout);assert!(!error.submission_retryable());
        let state=state.borrow();assert_eq!(state.closes,1);assert!(state.accepted<300);
    }
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-a4-queue-") as directory:
        directory = Path(directory)
        source = directory / "actual.rs"
        source.write_text(
            HARNESS.replace("__PWG__", str(ROOT / "src/r/net/pwg_raster.rs"))
            .replace("__QUEUE__", str(ROOT / "src/r/print2d.rs"))
            .replace("__IPP__", str(ROOT / "src/r/net/ipp_print.rs"))
        )
        for profile, optimization in [("debug", "0"), ("optimized", "3")]:
            binary = directory / profile
            subprocess.run([
                "rustc", "+nightly-2026-07-10", "--edition=2024", "--test",
                "-C", f"opt-level={optimization}", str(source), "-o", str(binary),
            ], cwd="/var/tmp", check=True)
            print(f"a4-print-queue profile={profile}", flush=True)
            subprocess.run([str(binary)], cwd="/var/tmp", check=True)


if __name__ == "__main__":
    main()
