#!/usr/bin/env python3
"""Run production media GET/redirect and browser fetch context on the host.

No sockets, kernel build or boot. URL parsing, redirect resolution, header
serialization, deadline/cancellation policy, fetch loops and ABI helpers come
verbatim from https.rs; the VM admission arm comes from vmcall.rs. Clock, waker,
TLS transport, shared VM page and task submission are deterministic mocks.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "src/r/net/https.rs").read_text()
VMCALL = (ROOT / "src/hv/vmcall.rs").read_text()


def between(start, end):
    first = SOURCE.index(start)
    return SOURCE[first:SOURCE.index(end, first + len(start))]


PRODUCTION = "\n".join([
    between("struct JsonPostCancellation {", "#[derive(Clone, Copy, Debug, Eq, PartialEq)]"),
    between("struct FetchTarget {", "#[derive(Clone, Copy)]\nstruct HttpsClientQueues"),
    between("fn request_has_header(", "fn parse_fetch_url("),
    between("fn parse_fetch_url(", "fn leak_str("),
    between("fn success_body(", "async fn fetch_https_bytes("),
    between("fn fetch_target_url(", "async fn request_https_bytes("),
    between("pub(crate) async fn get_media_bytes_profile_shared(", "pub async fn post_protobuf_shared("),
    between("async fn fetch_https_bytes_with_context(", "fn fetch_target_url("),
    between("unsafe fn abi_str", "unsafe fn optional_abi_string"),
    between("fn guest_vmcall_op_id(", "fn cabi_net_fetch_post_json_bytes_start_guest("),
    between("pub(crate) fn cabi_net_fetch_bytes_start_with_context_host(", "pub(crate) fn cabi_net_fetch_post_json_bytes_start_host("),
    between("#[derive(Default)]\nstruct CabiNetFetchBytesResult", "/// Decoded `OP_BP_FETCH_POST_JSON_BYTES_START`"),
    between('pub unsafe extern "C" fn trueos_cabi_net_fetch_bytes_start_with_referrer(', "pub(crate) fn fetch_bytes_request_context("),
    SOURCE[SOURCE.index("pub(crate) fn fetch_bytes_request_context("):],
])

VM_CONSTANTS = "\n".join(
    line for line in VMCALL.splitlines()
    if any(line.startswith(f"pub const {name}:") for name in [
        "COMM_PAGE_BYTES", "PAYLOAD_CAP", "OP_BP_FETCH_BYTES_START", "STATUS_OK", "STATUS_BAD_ARG",
    ])
)
VM_START = VMCALL[VMCALL.index("        OP_BP_FETCH_BYTES_START => {"):
                  VMCALL.index("        OP_BP_FETCH_POST_JSON_BYTES_START => {")]
VM_DISPATCH = r'''
fn dispatch_start(vm_id:u8,arg0:u64,req_len:u32)->DispatchOutcome {
    let seq=7;
    match OP_BP_FETCH_BYTES_START {
''' + VM_START + r'''
        _=>unreachable!(),
    }
}
'''

STUBS = r'''
#![allow(dead_code)]
extern crate alloc;
use core::future::Future;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
const HTTPS_REQUEST_CANCELLED:&str="cancelled";
const DEFAULT_USER_AGENT:&str="TRUEOS net-fetch";
#[macro_export]macro_rules! log_info {($($t:tt)*)=>{};}
mod log_os {pub mod flags {pub const HTTP_FETCH_DIAG_PROFILE_ENABLED:bool=false;}}

struct AtomicWaker(std::sync::Mutex<Option<core::task::Waker>>);
impl AtomicWaker {
    fn new()->Self {Self(std::sync::Mutex::new(None))}
    fn register(&self, waker:&core::task::Waker) {*self.0.lock().unwrap()=Some(waker.clone());}
    fn wake(&self) {if let Some(waker)=self.0.lock().unwrap().take(){waker.wake();}}
}
#[derive(Clone,Copy)]struct EmbassyDuration(u64);
impl EmbassyDuration {
    fn from_millis(value:u64)->Self {Self(value)}
    fn as_millis(self)->u64 {self.0}
}
#[derive(Clone,Copy)]struct Instant(u64);
impl Instant {
    fn now()->Self {Self(CLOCK.with(Cell::get))}
    fn saturating_duration_since(self,other:Self)->EmbassyDuration {EmbassyDuration(self.0.saturating_sub(other.0))}
}
impl core::ops::Add<EmbassyDuration> for Instant {
    type Output=Self;
    fn add(self,duration:EmbassyDuration)->Self {Self(self.0+duration.0)}
}
struct Timer;
struct Sleep(Instant);
impl Timer {fn after(duration:EmbassyDuration)->Sleep {Sleep(Instant::now()+duration)}}
impl Future for Sleep {
    type Output=();
    fn poll(self:core::pin::Pin<&mut Self>,_:&mut Context<'_>)->Poll<()> {
        if CLOCK.with(Cell::get)>=self.0.0 {Poll::Ready(())} else {Poll::Pending}
    }
}
thread_local! {
    static CLOCK:Cell<u64>=const {Cell::new(0)};
    static TRANSPORT:RefCell<Transport>=RefCell::new(Transport::default());
}
#[derive(Default)]struct Transport {
    steps:VecDeque<Step>,
    requests:Vec<RequestRecord>,
    happy_calls:usize,
    ordinary_calls:usize,
    cancelled_polls:usize,
    fence_retired:bool,
    dropped_before_fence:bool,
}
struct RequestRecord {url:String,wire:String,timeout:u32,max_bytes:usize}
enum Step {
    Reply {status:u16,location:Option<String>,body:Vec<u8>,elapsed:u64,cancel:bool},
    Error(&'static str),
    WaitForCancellation,
}
struct FenceGuard {needs_retirement:bool}
impl Drop for FenceGuard {
    fn drop(&mut self) {
        if self.needs_retirement {TRANSPORT.with(|t|t.borrow_mut().dropped_before_fence=true);}
    }
}
async fn request_https_response_once_cancellable(
    target:&FetchTarget, request:&HttpsRequest<'_>, timeout:u32, max_bytes:usize,
    cancellation:Option<&JsonPostCancellation>,
)->Result<HttpsJsonResponse,String> {
    // Serialize production headers before a mock socket is opened.
    let wire=String::from_utf8(build_http_request(target,request)?).unwrap();
    let step=TRANSPORT.with(|t| {
        let mut t=t.borrow_mut();
        t.requests.push(RequestRecord{url:fetch_target_url(target),wire,timeout,max_bytes});
        t.steps.pop_front().expect("unexpected extra media request")
    });
    match step {
        Step::Reply {status,location,body,elapsed,cancel}=> {
            CLOCK.with(|clock|clock.set(clock.get()+elapsed));
            if cancel {cancellation.unwrap().cancel();}
            Ok(HttpsJsonResponse{status,location,body,content_encoding:None,content_type:None,content_disposition:None,retry_after:None,date:None})
        }
        Step::Error(error)=>Err(error.into()),
        Step::WaitForCancellation=> {
            let cancellation=cancellation.expect("media TLS request must receive its cancellation fence");
            let mut guard=FenceGuard{needs_retirement:true};
            core::future::poll_fn(|cx| {
                if cancellation.poll_cancelled(cx) {
                    let retired=TRANSPORT.with(|t| {
                        let mut t=t.borrow_mut();t.cancelled_polls+=1;
                        if t.cancelled_polls==3 {t.fence_retired=true;true} else {false}
                    });
                    if retired {guard.needs_retirement=false;return Poll::Ready(Err("cancelled".into()));}
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            }).await
        }
    }
}
async fn request_https_response_once(
    target:&FetchTarget,request:&HttpsRequest<'_>,timeout:u32,max_bytes:usize,
)->Result<HttpsJsonResponse,String> {
    TRANSPORT.with(|t|t.borrow_mut().ordinary_calls+=1);
    request_https_response_once_cancellable(target,request,timeout,max_bytes,None).await
}
async fn request_https_happy_eyeballs(
    target:&FetchTarget,request:&HttpsRequest<'_>,timeout:u32,max_bytes:usize,
)->Result<HttpsJsonResponse,String> {
    TRANSPORT.with(|t|t.borrow_mut().happy_calls+=1);
    request_https_response_once_cancellable(target,request,timeout,max_bytes,None).await
}

struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
static CABI_NET_FETCH_SEQ:core::sync::atomic::AtomicU32=core::sync::atomic::AtomicU32::new(1);
static CABI_NET_FETCH_BYTES_RESULTS:Mutex<alloc::collections::BTreeMap<u32,CabiNetFetchBytesResult>>=
    Mutex::new(alloc::collections::BTreeMap::new());
struct SpawnRecord {url:String,timeout:u32,max_bytes:usize,happy:bool,referrer:Option<String>}
thread_local! {
    static SPAWNED:RefCell<Vec<SpawnRecord>>=const {RefCell::new(Vec::new())};
    static GUEST:Cell<bool>=const {Cell::new(false)};
    static VM_HOST:std::cell::UnsafeCell<HostPage>=std::cell::UnsafeCell::new(HostPage{payload:[0;PAYLOAD_CAP]});
    static VM_RESPONSE:Cell<(u32,u64)>=const {Cell::new((STATUS_BAD_ARG,0))};
    static VM_CALLS:RefCell<Vec<(u32,u64,u64,Vec<u8>)>>=const {RefCell::new(Vec::new())};
}
struct HostPage {payload:[u8;PAYLOAD_CAP]}
enum DispatchOutcome {Resume}
fn host_ptr(_:u8)->Option<*mut HostPage> {Some(VM_HOST.with(|page|page.get()))}
fn write_response(_:u8,_:u32,status:u32,value:u64,_:u32) {VM_RESPONSE.with(|v|v.set((status,value)));}
fn spawn_fetch_bytes(_:u32,url:String,timeout:u32,max_bytes:usize,happy:bool,referrer:Option<String>) {
    SPAWNED.with(|s|s.borrow_mut().push(SpawnRecord{url,timeout,max_bytes,happy,referrer}));
}
mod hv {pub fn current_hull_guest_context_vm_id()->Option<u8> {crate::GUEST.with(|v|v.get().then_some(1))}}
mod r {pub mod net {pub mod https {
    pub(crate) use crate::{fetch_bytes_request_context,cabi_net_fetch_bytes_start_with_context_host};
}}}
mod trueos_vm {pub mod vmcall {
    pub use crate::{PAYLOAD_CAP,OP_BP_FETCH_BYTES_START,STATUS_OK};
    pub fn call_with_payload(op:u32,a:u64,b:u64,payload:&[u8],out:&mut[u8])->(u32,u64) {
        assert_eq!(op,OP_BP_FETCH_BYTES_START);assert!(out.is_empty());assert!(payload.len()<=PAYLOAD_CAP);
        crate::VM_CALLS.with(|v|v.borrow_mut().push((op,a,b,payload.to_vec())));
        crate::VM_HOST.with(|page|unsafe{(&mut (*page.get()).payload)[..payload.len()].copy_from_slice(payload);});
        crate::dispatch_start(1,a,payload.len() as u32);
        crate::VM_RESPONSE.with(|v|v.get())
    }
}}
'''

TESTS = r'''
#[cfg(test)]mod tests {
    use super::*;
    fn reply(status:u16,location:Option<&str>)->Step {
        Step::Reply{status,location:location.map(String::from),body:vec![1,2,3],elapsed:0,cancel:false}
    }
    fn setup(steps:impl IntoIterator<Item=Step>) {
        CLOCK.with(|clock|clock.set(0));
        TRANSPORT.with(|t|*t.borrow_mut()=Transport{steps:steps.into_iter().collect(),..Default::default()});
    }
    fn run<T>(future:impl Future<Output=T>)->T {
        let mut future=Box::pin(future);
        let mut cx=Context::from_waker(core::task::Waker::noop());
        for _ in 0..100 {
            if let Poll::Ready(output)=future.as_mut().poll(&mut cx) {return output;}
            CLOCK.with(|clock|clock.set(clock.get()+1));
        }
        panic!("media future did not settle under its deadline/cancellation fence");
    }
    fn fetch(url:&str,referrer:Option<&str>)->Result<Vec<u8>,String> {
        run(get_media_bytes_profile_shared(url,"media-range",50,4096,&MediaFetchCancellation::new(),Some("TRUEOS test-agent"),referrer))
    }
    fn header<'a>(wire:&'a str,name:&str)->Option<&'a str> {
        wire.lines().find_map(|line|line.split_once(": ").filter(|(key,_)|key.eq_ignore_ascii_case(name)).map(|(_,value)|value))
    }
    #[test]fn follows_cross_host_302_preserving_signature_and_reducing_referrer() {
        setup([reply(302,Some("https://cdn.test/movie.mp4?token=a%2Fb&expires=999#drop")),reply(206,None)]);
        assert_eq!(fetch("https://page.test/get_file/?token=original",Some("https://page.test/watch?id=7#private")).unwrap(),[1,2,3]);
        TRANSPORT.with(|t| {
            let t=t.borrow();assert_eq!(t.requests.len(),2);
            assert_eq!(t.requests[1].url,"https://cdn.test:443/movie.mp4?token=a%2Fb&expires=999");
            assert_eq!(header(&t.requests[0].wire,"Referer"),Some("https://page.test/watch?id=7"));
            assert_eq!(header(&t.requests[1].wire,"Referer"),Some("https://page.test/"));
            for request in &t.requests {
                assert_eq!(request.max_bytes,4096);
                assert_eq!(header(&request.wire,"Range"),Some("bytes=0-"));
                assert_eq!(header(&request.wire,"User-Agent"),Some("TRUEOS test-agent"));
                assert_eq!(request.wire.lines().filter(|l|l.starts_with("Referer:")).count(),1);
            }
        });
    }
    #[test]fn follows_standard_redirect_statuses_and_relative_locations() {
        for status in [301,302,303,307,308] {
            setup([reply(status,Some("../video.mp4?signature=x%2Fy#ignored")),reply(200,None)]);
            assert!(fetch("https://page.test/player/start",None).is_ok());
            TRANSPORT.with(|t| {
                let t=t.borrow();assert_eq!(t.requests.len(),2);
                assert_eq!(t.requests[1].url,"https://page.test:443/video.mp4?signature=x%2Fy");
                assert!(t.requests.iter().all(|r|header(&r.wire,"Referer").is_none()));
            });
        }
        setup([reply(302,Some("//cdn.test/movie.mp4")),reply(206,None)]);
        assert!(fetch("https://page.test/start",None).is_ok());
        TRANSPORT.with(|t|assert_eq!(t.borrow().requests[1].url,"https://cdn.test:443/movie.mp4"));
    }
    #[test]fn completed_get_metadata_keeps_the_final_host_directory_and_query() {
        setup([reply(302,Some("../moved/start")),reply(307,Some("https://cdn.test/assets/page.html?token=a%2Fb#local")),reply(200,None)]);
        let target=parse_fetch_url("https://old.test/original/index.html").unwrap();
        let response=run(fetch_https_response_bytes_with_context(&target,50,4096,true,None)).unwrap();
        assert_eq!(response.final_url,"https://cdn.test/assets/page.html?token=a%2Fb");
        assert_eq!(response.body,[1,2,3]);
        TRANSPORT.with(|t|assert_eq!(t.borrow().requests.last().unwrap().url,"https://cdn.test:443/assets/page.html?token=a%2Fb"));
    }
    #[test]fn nonredirected_get_metadata_and_legacy_body_wrapper_are_consistent() {
        let target=parse_fetch_url("https://page.test:8443/directory/page.html?q=a%2Fb").unwrap();
        setup([reply(200,None)]);
        let response=run(fetch_https_response_bytes_with_context(&target,50,4096,false,None)).unwrap();
        assert_eq!(response.final_url,"https://page.test:8443/directory/page.html?q=a%2Fb");
        setup([reply(200,None)]);
        assert_eq!(run(fetch_https_bytes_with_context(&target,50,4096,false,None)).unwrap(),response.body);
    }
    #[test]fn forwards_profile_headers_on_each_hop() {
        for profile in ["media-range","media-norange","plain-range","plain-norange"] {
            setup([reply(302,Some("/final")),reply(206,None)]);
            run(get_media_bytes_profile_shared("https://page.test/start",profile,50,1234,&MediaFetchCancellation::new(),None,None)).unwrap();
            TRANSPORT.with(|t|for request in &t.borrow().requests {
                assert_eq!(request.max_bytes,1234);
                assert_eq!(header(&request.wire,"Range").is_some(),!profile.contains("norange"));
                assert_eq!(header(&request.wire,"Accept")==Some("*/*"),profile.starts_with("plain"));
                assert_eq!(header(&request.wire,"Accept-Encoding"),Some("identity"));
            });
        }
    }
    #[test]fn cross_port_referrer_is_origin_only_and_never_carries_credentials() {
        setup([reply(206,None)]);
        fetch("https://page.test:8443/movie",Some("https://page.test:9443/watch?id=secret#fragment")).unwrap();
        TRANSPORT.with(|t|assert_eq!(header(&t.borrow().requests[0].wire,"Referer"),Some("https://page.test:9443/")));
        for referrer in ["https://user:password@page.test/watch","http://page.test/watch","https://page.test/\r\nX-Bad: yes"] {
            setup([]);assert!(fetch("https://cdn.test/movie",Some(referrer)).is_err());
            TRANSPORT.with(|t|assert!(t.borrow().requests.is_empty()));
        }
    }
    #[test]fn unsafe_initial_and_redirect_urls_do_not_reach_transport() {
        for address in ["http://page.test/movie","https://user:secret@page.test/movie","https://page.test/a b"] {
            setup([]);assert!(fetch(address,None).is_err());
            TRANSPORT.with(|t|assert!(t.borrow().requests.is_empty()));
        }
        for location in ["http://cdn.test/movie","https://user:secret@cdn.test/movie","javascript:alert(1)","/movie\r\nX-Bad: yes"] {
            setup([reply(302,Some(location))]);assert!(fetch("https://page.test/start",None).is_err());
            TRANSPORT.with(|t|assert_eq!(t.borrow().requests.len(),1));
        }
    }
    #[test]fn loops_missing_locations_and_hop_limit_are_bounded() {
        setup([reply(302,Some("/b")),reply(302,Some("/start"))]);
        assert_eq!(fetch("https://page.test:443/start",None).unwrap_err(),"redirect loop");
        TRANSPORT.with(|t|assert_eq!(t.borrow().requests.len(),2));
        setup([reply(302,None)]);
        assert_eq!(fetch("https://page.test/start",None).unwrap_err(),"redirect missing location");
        let steps=(1..=6).map(|n|reply(302,Some(&format!("/hop{n}"))));
        setup(steps);assert_eq!(fetch("https://page.test/start",None).unwrap_err(),"too many redirects");
        TRANSPORT.with(|t|assert_eq!(t.borrow().requests.len(),6));
    }
    #[test]fn redirects_share_one_deadline_and_do_not_reset_timeout() {
        setup([
            Step::Reply{status:302,location:Some("/second".into()),body:vec![],elapsed:30,cancel:false},
            Step::Reply{status:302,location:Some("/third".into()),body:vec![],elapsed:20,cancel:false},
        ]);
        assert_eq!(fetch("https://page.test/start",None).unwrap_err(),"timeout");
        TRANSPORT.with(|t|assert_eq!(t.borrow().requests.iter().map(|r|r.timeout).collect::<Vec<_>>(),[50,20]));
    }
    #[test]fn cancellation_before_start_and_between_redirects_prevents_new_io() {
        setup([]);let cancellation=MediaFetchCancellation::new();cancellation.cancel();
        assert_eq!(run(get_media_bytes_profile_shared("https://page.test/start","media-range",50,4096,&cancellation,None,None)).unwrap_err(),"cancelled");
        TRANSPORT.with(|t|assert!(t.borrow().requests.is_empty()));
        setup([Step::Reply{status:302,location:Some("https://cdn.test/movie".into()),body:vec![],elapsed:0,cancel:true}]);
        assert_eq!(fetch("https://page.test/start",None).unwrap_err(),"cancelled");
        TRANSPORT.with(|t|assert_eq!(t.borrow().requests.len(),1));
    }
    #[test]fn deadline_waits_for_tls_cancellation_fence_to_retire() {
        setup([Step::WaitForCancellation]);let cancellation=MediaFetchCancellation::new();
        assert_eq!(run(get_media_bytes_profile_shared("https://page.test/movie","media-range",3,4096,&cancellation,None,None)).unwrap_err(),"cancelled");
        assert!(cancellation.0.is_cancelled());
        TRANSPORT.with(|t| {
            let t=t.borrow();assert_eq!(t.cancelled_polls,3);assert!(t.fence_retired);assert!(!t.dropped_before_fence);
        });
    }
    #[test]fn transport_errors_and_final_http_failures_propagate() {
        setup([reply(302,Some("https://cdn.test/movie")),Step::Error("too large content_length=5000 max=4096")]);
        assert_eq!(fetch("https://page.test/start",None).unwrap_err(),"too large content_length=5000 max=4096");
        setup([reply(404,None)]);
        assert_eq!(fetch("https://page.test/start",None).unwrap_err(),"http status 404");
    }
    fn context_fetch(url:&str,referrer:Option<&str>,happy:bool)->Result<Vec<u8>,String> {
        let target=parse_fetch_url(url).map_err(String::from)?;
        run(fetch_https_bytes_with_context(&target,50,4096,happy,referrer))
    }
    #[test]fn subdocument_context_redirects_send_one_referrer_and_race_only_first_connection() {
        setup([reply(302,Some("https://cdn.test/player/start?signature=a%2Fb")),reply(302,Some("../final")),reply(200,None)]);
        context_fetch("https://page.test/embed",Some("https://page.test/watch?private=1#drop"),true).unwrap();
        TRANSPORT.with(|t| {
            let t=t.borrow();assert_eq!(t.happy_calls,1);assert_eq!(t.ordinary_calls,2);
            assert_eq!(t.requests[1].url,"https://cdn.test:443/player/start?signature=a%2Fb");
            assert_eq!(t.requests[2].url,"https://cdn.test:443/final");
            assert_eq!(header(&t.requests[0].wire,"Referer"),Some("https://page.test/watch?private=1"));
            for request in &t.requests[1..] {
                assert_eq!(header(&request.wire,"Referer"),Some("https://page.test/"));
                assert_eq!(request.wire.lines().filter(|l|l.starts_with("Referer:")).count(),1);
                assert_eq!(request.max_bytes,4096);
            }
        });
    }
    #[test]fn subdocument_context_rejects_unsafe_redirects_before_opening_destination() {
        for location in ["https://user@cdn.test/movie","https://cdn.test/path\tother","http://cdn.test/movie"] {
            setup([reply(302,Some(location))]);
            assert!(context_fetch("https://page.test/embed",Some("https://page.test/watch"),true).is_err());
            TRANSPORT.with(|t|assert_eq!(t.borrow().requests.len(),1));
        }
    }
    #[test]fn subdocument_context_deadline_hop_limit_and_empty_retry_are_bounded() {
        setup([
            Step::Reply{status:302,location:Some("/next".into()),body:vec![],elapsed:30,cancel:false},
            Step::Reply{status:302,location:Some("/last".into()),body:vec![],elapsed:20,cancel:false},
        ]);
        assert_eq!(context_fetch("https://page.test/embed",Some("https://page.test/watch"),true).unwrap_err(),"timeout");
        TRANSPORT.with(|t|assert_eq!(t.borrow().requests.iter().map(|r|r.timeout).collect::<Vec<_>>(),[50,20]));
        setup((1..=11).map(|n|reply(302,Some(&format!("/hop{n}")))));
        assert_eq!(context_fetch("https://page.test/embed",Some("https://page.test/watch"),true).unwrap_err(),"too many redirects");
        TRANSPORT.with(|t|assert_eq!(t.borrow().requests.len(),11));
        setup([Step::Error("bad response len=0 empty connection"),reply(302,Some("https://cdn.test/final")),reply(200,None)]);
        context_fetch("https://page.test/embed",Some("https://page.test/watch"),true).unwrap();
        TRANSPORT.with(|t| {
            let t=t.borrow();assert_eq!((t.happy_calls,t.ordinary_calls),(1,2));
            assert_eq!(header(&t.requests[1].wire,"Referer"),Some("https://page.test/watch"));
            assert_eq!(header(&t.requests[2].wire,"Referer"),Some("https://page.test/"));
        });
    }
    #[test]fn request_context_decodes_all_four_flags_without_changing_legacy_urls() {
        for happy in [false,true] {
            let flags=u64::from(happy);
            assert_eq!(fetch_bytes_request_context(b"https://page.test/start",flags),Some(("https://page.test/start",None,happy)));
            // Existing URL-only callers keep the HTTP path and no new context.
            assert_eq!(fetch_bytes_request_context(b"http://page.test/start",flags),Some(("http://page.test/start",None,happy)));
            assert_eq!(fetch_bytes_request_context(b"https://page.test/start\0https://parent.test/watch",flags|2),Some(("https://page.test/start",Some("https://parent.test/watch"),happy)));
        }
        for flags in [4,5,8,u64::MAX] {assert!(fetch_bytes_request_context(b"https://page.test/start",flags).is_none());}
    }
    #[test]fn request_context_rejects_malformed_encoding_and_unsafe_context() {
        for flags in 0..=3 {
            for bad in [&b""[..],&b"\xff"[..],&b" \t"[..]] {assert!(fetch_bytes_request_context(bad,flags).is_none());}
        }
        for flags in [2,3] {
            for bad in [
                "https://page.test/start", "\0https://parent.test/watch", "https://page.test/start\0",
                "https://page.test/start\0https://parent.test/watch\0extra",
                "http://page.test/start\0https://parent.test/watch",
                "https://page.test/start\0http://parent.test/watch",
                "https://user@page.test/start\0https://parent.test/watch",
                "https://page.test/start\0https://secret@parent.test/watch",
                "https://page.test/a\r\nBad: yes\0https://parent.test/watch",
                "https://page.test/start\0https://parent.test/a\u{1}",
                "https://page.test:99999/start\0https://parent.test/watch",
            ] {assert!(fetch_bytes_request_context(bad.as_bytes(),flags).is_none(),"accepted {bad:?} flags={flags}");}
        }
        assert!(fetch_bytes_request_context(b"https://page.test/start\0https://parent.test/watch",0).is_none());
        assert!(fetch_bytes_request_context(b"https://page.test/start\0https://parent.test/watch",1).is_none());
    }
    fn reset_cabi(guest:bool) {
        GUEST.with(|v|v.set(guest));SPAWNED.with(|v|v.borrow_mut().clear());VM_CALLS.with(|v|v.borrow_mut().clear());
        CABI_NET_FETCH_BYTES_RESULTS.lock().clear();VM_RESPONSE.with(|v|v.set((STATUS_BAD_ARG,0)));
    }
    fn cabi(url:&[u8],referrer:&[u8])->u32 {
        unsafe{trueos_cabi_net_fetch_bytes_start_with_referrer(url.as_ptr(),url.len(),referrer.as_ptr(),referrer.len())}
    }
    #[test]fn exported_referrer_cabi_runs_on_host_and_round_trips_actual_guest_dispatch() {
        for guest in [false,true] {
            reset_cabi(guest);
            assert!(cabi(b"https://cdn.test/player?signature=keep",b"https://page.test/watch?id=1")>0);
            SPAWNED.with(|v| {
                let v=v.borrow();assert_eq!(v.len(),1);let request=&v[0];
                assert_eq!(request.url,"https://cdn.test/player?signature=keep");
                assert_eq!(request.referrer.as_deref(),Some("https://page.test/watch?id=1"));
                assert_eq!(request.timeout,45_000);assert_eq!(request.max_bytes,8*1024*1024);assert!(request.happy);
            });
            VM_CALLS.with(|v| {
                let v=v.borrow();assert_eq!(v.len(),usize::from(guest));
                if guest {assert_eq!(v[0],(OP_BP_FETCH_BYTES_START,3,0,b"https://cdn.test/player?signature=keep\0https://page.test/watch?id=1".to_vec()));}
            });
        }
    }
    #[test]fn exported_referrer_cabi_rejects_bad_pointers_utf8_and_untrusted_context_before_dispatch() {
        for guest in [false,true] {
            reset_cabi(guest);
            let good=b"https://page.test/watch";
            assert_eq!(unsafe{trueos_cabi_net_fetch_bytes_start_with_referrer(core::ptr::null(),1,good.as_ptr(),good.len())},0);
            assert_eq!(unsafe{trueos_cabi_net_fetch_bytes_start_with_referrer(good.as_ptr(),good.len(),core::ptr::null(),1)},0);
            for bad in [&b""[..],&b"\xff"[..],&b"http://page.test/watch"[..],&b"https://user@page.test/watch"[..],&b"https://page.test/\0other"[..],&b"https://page.test/a\n"[..]] {
                assert_eq!(cabi(bad,good),0);assert_eq!(cabi(good,bad),0);
            }
            VM_CALLS.with(|v|assert!(v.borrow().is_empty()));SPAWNED.with(|v|assert!(v.borrow().is_empty()));
        }
    }
    #[test]fn guest_packer_and_actual_vm_start_arm_preserve_legacy_and_context_flags() {
        for happy in [false,true] {for referrer in [None,Some("https://parent.test/watch")] {
            reset_cabi(true);
            assert!(cabi_net_fetch_bytes_start_with_context_guest("https://page.test/start",happy,referrer)>0);
            VM_CALLS.with(|v|assert_eq!(v.borrow()[0].1,u64::from(happy)|if referrer.is_some(){2}else{0}));
            SPAWNED.with(|v| {let v=v.borrow();assert_eq!(v.len(),1);assert_eq!(v[0].happy,happy);assert_eq!(v[0].referrer.as_deref(),referrer);});
        }}
        reset_cabi(true);
        for (flags,payload) in [(4,&b"https://page.test/start"[..]),(3,&b"https://page.test/start"[..]),(1,&b"https://page.test/start\0bad"[..])] {
            assert_eq!(trueos_vm::vmcall::call_with_payload(OP_BP_FETCH_BYTES_START,flags,0,payload,&mut[]),(STATUS_BAD_ARG,0));
        }
        SPAWNED.with(|v|assert!(v.borrow().is_empty()));
        VM_CALLS.with(|v|v.borrow_mut().clear());
        assert_eq!(cabi_net_fetch_bytes_start_with_context_guest(&"x".repeat(PAYLOAD_CAP+1),true,None),0);
        VM_CALLS.with(|v|assert!(v.borrow().is_empty()));
    }
}
'''


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="media-https-redirects-") as directory:
        path = Path(directory)
        (path / "test.rs").write_text(STUBS + VM_CONSTANTS + PRODUCTION + VM_DISPATCH + TESTS)
        subprocess.run([
            "rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "test"),
        ], check=True)
        subprocess.run([str(path / "test"), "--test-threads=1"], check=True)
