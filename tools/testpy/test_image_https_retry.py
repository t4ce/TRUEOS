#!/usr/bin/env python3
"""Run the production image GET/retry path with deterministic host transport.

No sockets, kernel build or boot. The actual response parser, URL admission,
redirect resolver, retry policy, retry wait and image GET loop are compiled
verbatim from production. Only TLS/preview transport, timer and waker are mocks.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "src/r/net/https.rs").read_text()
ADMISSION = (ROOT / "src/r/services/hull_image_policy.rs").read_text()
DIAGNOSTICS = (ROOT / "src/r/net/download_diagnostics.rs").read_text()
IMAGE_POLICY = ROOT / "src/r/net/image_retry.rs"


def between(source, start, end):
    first = source.index(start)
    return source[first:source.index(end, first + len(start))]


PRODUCTION = "\n".join([
    between(SOURCE, "struct JsonPostCancellation {", "fn register_json_post_cancellation("),
    between(SOURCE, "struct FetchTarget {", "#[derive(Clone, Copy)]\nstruct HttpsClientQueues"),
    between(SOURCE, "fn request_has_header(", "fn leak_str("),
    between(SOURCE, "fn find_http_header_end(", "fn decode_response_content("),
    between(SOURCE, "fn success_body(", "async fn fetch_https_bytes("),
    between(SOURCE, "fn fetch_target_url(", "async fn request_https_bytes("),
    between(SOURCE, "static IMAGE_HOST_BACKOFF", "pub async fn get_bytes_bearer_shared("),
])
CANONICAL_URL = between(ADMISSION, "pub(crate) fn canonical_url(", "pub(super) fn valid_page_host(")
PUBLIC_URL = between(DIAGNOSTICS, "fn clipped(", "fn header_value")

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
const HTTPS_PROBE_HEADER_MAX:usize=32768;
const HTTPS_RESPONSE_OVERHEAD_MAX:usize=4096;
#[macro_export]macro_rules! log_info {($($t:tt)*)=>{};}
// Evaluate the production flat JSON field expressions without a serde build.
// Debug strings preserve exact integers and distinguish string values.
#[macro_export]macro_rules! log_important {
    (target:$target:literal; $format:literal, serde_json::json!({$($key:literal:$value:expr),* $(,)?}))=>{{
        let fields=[$(($key,format!("{:?}",$value))),*].into_iter().collect();
        crate::RETRY_LOGS.with(|logs|logs.borrow_mut().push(fields));
    }};
    ($($t:tt)*)=>{};
}

struct AtomicWaker(std::sync::Mutex<Option<core::task::Waker>>);
impl AtomicWaker {
    fn new()->Self {Self(std::sync::Mutex::new(None))}
    fn register(&self,waker:&core::task::Waker) {*self.0.lock().unwrap()=Some(waker.clone());}
    fn wake(&self) {if let Some(waker)=self.0.lock().unwrap().take(){waker.wake();}}
}
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd)]struct EmbassyDuration(u64);
impl EmbassyDuration {
    fn from_millis(value:u64)->Self {Self(value)}
    fn as_millis(self)->u64 {self.0}
}
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd)]struct Instant(u64);
impl Instant {
    fn now()->Self {Self(CLOCK.with(Cell::get))}
    fn as_millis(self)->u64 {self.0}
    fn saturating_duration_since(self,other:Self)->EmbassyDuration {EmbassyDuration(self.0.saturating_sub(other.0))}
}
impl core::ops::Add<EmbassyDuration> for Instant {
    type Output=Self;
    fn add(self,duration:EmbassyDuration)->Self {Self(self.0.saturating_add(duration.0))}
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
fn monotonic_ms()->u64 {CLOCK.with(Cell::get)}
mod chronos {
    pub fn best_effort_unix_time_seconds()->Option<u64> {crate::UNIX_TIME.with(core::cell::Cell::get)}
}
thread_local! {
    static CLOCK:Cell<u64>=const {Cell::new(0)};
    static UNIX_TIME:Cell<Option<u64>>=const {Cell::new(None)};
    static RETRY_LOGS:RefCell<Vec<std::collections::BTreeMap<&'static str,String>>>=const {RefCell::new(Vec::new())};
    static TRANSPORT:RefCell<Transport>=RefCell::new(Transport::default());
}
#[derive(Default)]struct Transport {
    steps:VecDeque<Step>,requests:Vec<RequestRecord>,
    cancelled_polls:usize,fence_retired:bool,dropped_before_fence:bool,
}
struct RequestRecord {url:String,wire:String,timeout:u64,max_bytes:usize,started:u64}
enum Step {
    Reply {wire:Vec<u8>,elapsed:u64,cancel:bool},
    DelayedReply {wire:Vec<u8>,ready_at:u64},
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
    target:&FetchTarget,request:&HttpsRequest<'_>,timeout:u32,max_bytes:usize,
    cancellation:Option<&JsonPostCancellation>,
)->Result<HttpsJsonResponse,String> {
    let wire=String::from_utf8(build_http_request(target,request)?).unwrap();
    let step=TRANSPORT.with(|t| {
        let mut t=t.borrow_mut();
        t.requests.push(RequestRecord{url:fetch_target_url(target),wire,timeout:u64::from(timeout),max_bytes,started:monotonic_ms()});
        t.steps.pop_front().expect("unexpected extra image request")
    });
    match step {
        Step::Reply{wire,elapsed,cancel}=> {
            CLOCK.with(|clock|clock.set(clock.get().saturating_add(elapsed)));
            if cancel {cancellation.unwrap().cancel();}
            http_response_from_bytes(&wire,max_bytes)
        }
        Step::DelayedReply{wire,ready_at}=> {
            core::future::poll_fn(|cx| {
                if cancellation.is_some_and(|cancel|cancel.poll_cancelled(cx)) {
                    return Poll::Ready(Err(String::from(HTTPS_REQUEST_CANCELLED)));
                }
                if monotonic_ms()>=ready_at {Poll::Ready(http_response_from_bytes(&wire,max_bytes))}
                else {Poll::Pending}
            }).await
        }
        Step::Error(error)=>Err(error.into()),
        Step::WaitForCancellation=> {
            let cancellation=cancellation.expect("image TLS request requires cancellation fence");
            let mut guard=FenceGuard{needs_retirement:true};
            core::future::poll_fn(|cx| {
                if cancellation.poll_cancelled(cx) {
                    let retired=TRANSPORT.with(|t| {
                        let mut t=t.borrow_mut();t.cancelled_polls+=1;
                        if t.cancelled_polls==3 {t.fence_retired=true;true} else {false}
                    });
                    if retired {guard.needs_retirement=false;return Poll::Ready(Err("cancelled".into()));}
                }
                cx.waker().wake_by_ref();Poll::Pending
            }).await
        }
    }
}
mod surfer {pub mod html_shack {
    pub struct Response {pub bytes:Vec<u8>}
    pub async fn fetch_bytes_via_pool(url:String,timeout:u64,max_bytes:usize)->Result<Response,String> {
        let step=crate::TRANSPORT.with(|t| {
            let mut t=t.borrow_mut();
            t.requests.push(crate::RequestRecord{url,wire:String::new(),timeout,max_bytes,started:crate::monotonic_ms()});
            t.steps.pop_front().expect("unexpected extra preview request")
        });
        match step {
            crate::Step::Reply{wire,elapsed,cancel:false}=> {
                crate::CLOCK.with(|clock|clock.set(clock.get()+elapsed));
                crate::success_body(crate::http_response_from_bytes(&wire,max_bytes)?).map(|bytes|Response{bytes})
            }
            crate::Step::Error(error)=>Err(error.into()),
            _=>panic!("unsupported preview transport step"),
        }
    }
}}
'''

TESTS = r'''
#[cfg(test)]mod tests {
    use super::*;
    fn response(status:u16,headers:&str)->Step {
        Step::Reply{wire:format!("HTTP/1.1 {status} Test\r\nContent-Length: 3\r\n{headers}\r\npng").into_bytes(),elapsed:0,cancel:false}
    }
    fn delayed_response(status:u16,headers:&str,ready_at:u64)->Step {
        let Step::Reply{wire,..}=response(status,headers) else {unreachable!()};
        Step::DelayedReply{wire,ready_at}
    }
    fn setup(steps:impl IntoIterator<Item=Step>) {
        CLOCK.with(|clock|clock.set(0));
        UNIX_TIME.with(|clock|clock.set(None));
        RETRY_LOGS.with(|logs|logs.borrow_mut().clear());
        *IMAGE_HOST_BACKOFF.lock()=image_retry::HostBackoffTable::new();
        TRANSPORT.with(|t|*t.borrow_mut()=Transport{steps:steps.into_iter().collect(),..Default::default()});
    }
    fn run<T>(future:impl Future<Output=T>)->T {
        let mut future=Box::pin(future);
        let mut cx=Context::from_waker(core::task::Waker::noop());
        for _ in 0..120_000 {
            if let Poll::Ready(output)=future.as_mut().poll(&mut cx) {return output;}
            CLOCK.with(|clock|clock.set(clock.get()+1));
        }
        panic!("image future exceeded bounded deadline/cancellation fixture");
    }
    fn fetch(url:&str,timeout:u32)->Result<Vec<u8>,String> {
        run(get_image_bytes_shared(url,timeout,4096,&MediaFetchCancellation::new()))
    }
    fn records(f:impl FnOnce(&[RequestRecord])) {TRANSPORT.with(|t|f(&t.borrow().requests));}
    #[test]fn ordinary_image_success_uses_production_header_serialization() {
        setup([response(200,"")]);
        assert_eq!(fetch("https://ASSET.test:443/image.png?q=1#ignore",5000).unwrap(),b"png");
        records(|requests| {
            assert_eq!(requests.len(),1);let request=&requests[0];
            assert_eq!(request.url,"https://asset.test:443/image.png?q=1");
            assert_eq!(request.max_bytes,4096);
            assert!(request.wire.starts_with("GET /image.png?q=1 HTTP/1.1\r\n"));
            assert!(request.wire.contains("Accept-Encoding: identity\r\n"));
            assert!(request.wire.contains("User-Agent: TRUEOS Hull raster/1\r\n"));
        });
    }
    #[test]fn caller_budget_is_clamped_to_one_through_fifteen_thousand_milliseconds() {
        for (timeout,expected) in [(0,1),(1,1),(5000,5000),(30_000,15_000),(u32::MAX,15_000)] {
            setup([response(200,"")]);
            assert_eq!(fetch("https://asset.test/image.png",timeout).unwrap(),b"png");
            records(|requests|assert_eq!(requests[0].timeout,expected));
        }
    }
    #[test]fn forbidden_and_other_final_statuses_do_not_retry() {
        for status in [400,401,403,404,500,503] {
            setup([response(status,"Retry-After: 1\r\n")]);
            assert_eq!(fetch("https://asset.test/image.png",5000).unwrap_err(),format!("http status {status}"));
            records(|requests|assert_eq!(requests.len(),1));
        }
    }
    #[test]fn transport_errors_are_not_429_retries() {
        setup([Step::Error("tls unknown issuer")]);
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap_err(),"tls unknown issuer");
        records(|requests|assert_eq!(requests.len(),1));
    }
    #[test]fn ordinary_and_relative_redirects_keep_query_and_bounds() {
        for status in [301,302,303,307,308] {
            setup([response(status,"Location: ../final.png?token=a%2Fb#drop\r\n"),response(200,"")]);
            assert_eq!(fetch("https://asset.test/directory/image.png",5000).unwrap(),b"png");
            records(|requests| {
                assert_eq!(requests.len(),2);
                assert_eq!(requests[1].url,"https://asset.test:443/final.png?token=a%2Fb");
                assert!(requests.iter().all(|request|request.max_bytes==4096));
            });
        }
    }
    #[test]fn redirect_loops_missing_locations_and_hop_limit_are_bounded() {
        setup([response(302,"Location: /other.png\r\n"),response(302,"Location: /image.png\r\n")]);
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap_err(),"redirect loop");
        records(|requests|assert_eq!(requests.len(),2));
        setup([response(302,"")]);
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap_err(),"redirect missing location");
        records(|requests|assert_eq!(requests.len(),1));
        setup((1..=6).map(|n|response(302,&format!("Location: /hop{n}.png\r\n"))));
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap_err(),"too many redirects");
        records(|requests|assert_eq!(requests.len(),6));
    }
    #[test]fn admission_is_rechecked_before_each_redirect_request() {
        for url in ["http://asset.test/image.png","https://user:secret@asset.test/image.png","https://asset.test/image.png\nBad: yes"] {
            setup([]);assert!(fetch(url,5000).is_err());records(|requests|assert!(requests.is_empty()));
        }
        for location in ["http://asset.test/image.png","https://user:secret@asset.test/image.png","javascript:evil","https://asset.test:0/image.png"] {
            setup([response(302,&format!("Location: {location}\r\n"))]);
            assert!(fetch("https://asset.test/start.png",5000).is_err());
            records(|requests|assert_eq!(requests.len(),1));
        }
    }
    #[test]fn cancellation_before_start_and_between_redirects_prevents_more_io() {
        setup([]);let cancellation=MediaFetchCancellation::new();cancellation.cancel();
        assert_eq!(run(get_image_bytes_shared("https://asset.test/image.png",5000,4096,&cancellation)).unwrap_err(),"cancelled");
        records(|requests|assert!(requests.is_empty()));
        let mut step=response(302,"Location: https://cdn.test/final.png\r\n");
        if let Step::Reply{cancel,..}=&mut step {*cancel=true;}
        setup([step]);
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap_err(),"cancelled");
        records(|requests|assert_eq!(requests.len(),1));
    }
    #[test]fn active_request_timeout_waits_for_tls_owner_fence_to_retire() {
        setup([Step::WaitForCancellation]);let cancellation=MediaFetchCancellation::new();
        assert_eq!(run(get_image_bytes_shared("https://asset.test/image.png",3,4096,&cancellation)).unwrap_err(),"cancelled");
        assert!(cancellation.0.is_cancelled());
        TRANSPORT.with(|t| {
            let t=t.borrow();assert_eq!(t.cancelled_polls,3);assert!(t.fence_retired);assert!(!t.dropped_before_fence);
        });
    }
    #[test]fn explicit_http_loopback_preview_uses_admitted_pool_transport() {
        setup([response(200,"")]);
        assert_eq!(fetch("http://127.0.0.1:49180/image.png",5000).unwrap(),b"png");
        records(|requests| {
            assert_eq!(requests.len(),1);assert_eq!(requests[0].url,"http://127.0.0.1:49180/image.png");
            assert_eq!(requests[0].max_bytes,4096);assert!(requests[0].timeout>0);
        });
    }
    #[test]fn rate_limit_integer_header_retries_without_resetting_total_deadline() {
        let mut limited=response(429,"Retry-After: 1\r\n");
        if let Step::Reply{elapsed,..}=&mut limited {*elapsed=250;}
        setup([limited,response(200,"")]);
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap(),b"png");
        records(|requests| {
            assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,1250]);
            assert_eq!(requests.iter().map(|r|r.timeout).collect::<Vec<_>>(),[5000,3750]);
        });
    }
    #[test]fn retry_marker_accounts_for_time_between_response_and_wait_reservation() {
        setup([]);
        IMAGE_HOST_BACKOFF.lock().record_rate_limit("asset.test:443",0,1000);
        CLOCK.with(|clock|clock.set(1));
        run(wait_image_host_slot("asset.test:443","https://asset.test/image.png",Instant(5000),
            5000,Some((1,1000,0)),&MediaFetchCancellation::new())).unwrap();
        RETRY_LOGS.with(|logs| {
            let logs=logs.borrow();assert_eq!(logs.len(),1);let marker=&logs[0];
            assert_eq!(marker.get("event").map(String::as_str),Some("\"retry\""));
            let number=|key|marker.get(key).unwrap().parse::<u64>().unwrap();
            assert_eq!(number("delay_ms"),999);
            assert_eq!(number("elapsed_since_response_ms"),1);
            assert_eq!(number("retry_after_ms"),1000);
            assert_eq!(number("delay_ms")+number("elapsed_since_response_ms"),number("retry_after_ms"));
            assert_eq!(number("elapsed_ms"),1);assert_eq!(number("remaining_ms"),4999);
        });
        assert_eq!(monotonic_ms(),1000);
        records(|requests|assert!(requests.is_empty()));
    }
    #[test]fn response_framing_preserves_retry_after_and_date_metadata() {
        for framing in ["Content-Length: 0\r\n","Transfer-Encoding: chunked\r\n",""] {
            let body=if framing.starts_with("Transfer-Encoding") {"0\r\n\r\n"} else {""};
            let wire=format!("HTTP/1.1 429 Limited\r\n{framing}rEtRy-AfTeR: 2\r\ndAtE: Wed, 21 Oct 2015 07:28:00 GMT\r\n\r\n{body}");
            let response=http_response_from_bytes(wire.as_bytes(),4096).unwrap();
            assert_eq!(response.retry_after.as_deref(),Some("2"));
            assert_eq!(response.date.as_deref(),Some("Wed, 21 Oct 2015 07:28:00 GMT"));
        }
        let response=complete_http_head_response(b"HTTP/1.1 429 Limited\r\nRetry-After: 2\r\nDate: Wed, 21 Oct 2015 07:28:00 GMT\r\n\r\n").unwrap().unwrap();
        assert_eq!(response.retry_after.as_deref(),Some("2"));
        assert_eq!(response.date.as_deref(),Some("Wed, 21 Oct 2015 07:28:00 GMT"));
    }
    #[test]fn response_date_anchors_http_date_retry_after_when_wall_clock_is_unknown() {
        setup([response(429,"rEtRy-AfTeR: Wed, 21 Oct 2015 07:28:03 GMT\r\ndAtE: Wed, 21 Oct 2015 07:28:00 GMT\r\n"),response(200,"")]);
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap(),b"png");
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,3000]));
    }
    #[test]fn trusted_wall_clock_takes_precedence_over_response_date() {
        setup([response(429,"Retry-After: Wed, 21 Oct 2015 07:28:04 GMT\r\nDate: Wed, 21 Oct 2015 07:28:00 GMT\r\n"),response(200,"")]);
        UNIX_TIME.with(|clock|clock.set(Some(1_445_412_482)));
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap(),b"png");
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,2000]));
    }
    #[test]fn missing_malformed_and_zero_retry_after_are_delayed_without_busy_retry() {
        for headers in ["","Retry-After: nonsense\r\n","Retry-After: -1\r\n","Retry-After: 0.25\r\n","Retry-After: 0\r\n"] {
            setup([response(429,headers),response(200,"")]);
            assert_eq!(fetch("https://asset.test/image.png",5000).unwrap(),b"png");
            records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,1000]));
        }
    }
    #[test]fn unanchored_http_date_does_not_retry_early() {
        setup([response(429,"Retry-After: Wed, 21 Oct 2015 07:28:03 GMT\r\n")]);
        assert!(fetch("https://asset.test/image.png",5000).is_err());
        records(|requests|assert_eq!(requests.len(),1));
    }
    #[test]fn huge_integer_retry_after_obeys_deadline_without_truncation_or_extra_io() {
        for value in ["99999999999999999999999999999999999999","18446744073709551615","600"] {
            setup([response(429,&format!("Retry-After: {value}\r\n"))]);
            assert!(fetch("https://asset.test/image.png",1500).is_err());
            records(|requests|assert_eq!(requests.len(),1));
        }
    }
    #[test]fn at_most_two_rate_limit_retries_are_sent() {
        setup((0..3).map(|_|response(429,"Retry-After: 1\r\n")));
        assert!(fetch("https://asset.test/image.png",10_000).unwrap_err().contains("429"));
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,1000,2000]));
    }
    #[test]fn rate_limit_waits_and_retries_share_the_original_deadline() {
        setup([response(429,"Retry-After: 1\r\n"),response(429,"Retry-After: 1\r\n")]);
        assert!(fetch("https://asset.test/image.png",1500).is_err());
        records(|requests| {
            assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,1000]);
            assert_eq!(requests.iter().map(|r|r.timeout).collect::<Vec<_>>(),[1500,500]);
        });
    }
    #[test]fn retry_timer_is_pending_and_never_sends_before_its_slot() {
        setup([response(429,"Retry-After: 1\r\n"),response(200,"")]);
        let cancellation=MediaFetchCancellation::new();
        let mut future=Box::pin(get_image_bytes_shared("https://asset.test/image.png",5000,4096,&cancellation));
        let mut cx=Context::from_waker(core::task::Waker::noop());
        assert!(future.as_mut().poll(&mut cx).is_pending());
        records(|requests|assert_eq!(requests.len(),1));
        CLOCK.with(|clock|clock.set(999));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        records(|requests|assert_eq!(requests.len(),1));
        CLOCK.with(|clock|clock.set(1000));
        assert_eq!(future.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        records(|requests|assert_eq!(requests.len(),2));
    }
    #[test]fn cancellation_wakes_pending_retry_and_does_not_send_another_request() {
        use std::sync::{Arc,atomic::AtomicUsize};
        struct WakeCount(AtomicUsize);
        impl std::task::Wake for WakeCount {
            fn wake(self:Arc<Self>) {self.0.fetch_add(1,Ordering::Relaxed);}
            fn wake_by_ref(self:&Arc<Self>) {self.0.fetch_add(1,Ordering::Relaxed);}
        }
        setup([response(429,"Retry-After: 1\r\n")]);
        let count=Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker=core::task::Waker::from(count.clone());let mut cx=Context::from_waker(&waker);
        let cancellation=MediaFetchCancellation::new();
        let mut future=Box::pin(get_image_bytes_shared("https://asset.test/image.png",5000,4096,&cancellation));
        assert!(future.as_mut().poll(&mut cx).is_pending());
        cancellation.cancel();
        assert!(count.0.load(Ordering::Relaxed)>0,"retry wait failed to register cancellation waker");
        assert_eq!(future.as_mut().poll(&mut cx),Poll::Ready(Err("cancelled".into())));
        CLOCK.with(|clock|clock.set(5000));
        records(|requests|assert_eq!(requests.len(),1));
    }
    #[test]fn rate_limit_retries_do_not_consume_redirect_hops_or_revisit_detection() {
        setup([response(429,"Retry-After: 1\r\n"),
            response(302,"Location: https://cdn.test/final.png\r\n"),
            response(429,"Retry-After: 1\r\n"),response(200,"")]);
        assert_eq!(fetch("https://asset.test/image.png",5000).unwrap(),b"png");
        records(|requests| {
            assert_eq!(requests.len(),4);
            assert_eq!(requests[0].url,requests[1].url);assert_eq!(requests[2].url,requests[3].url);
            assert!(requests[2].url.contains("cdn.test"));
        });
    }
    #[test]fn two_retry_limit_applies_across_the_entire_redirect_chain() {
        setup([response(429,"Retry-After: 1\r\n"),response(429,"Retry-After: 1\r\n"),
            response(302,"Location: https://cdn.test/final.png\r\n"),response(429,"Retry-After: 1\r\n")]);
        assert!(fetch("https://asset.test/image.png",10_000).unwrap_err().contains("429"));
        records(|requests|assert_eq!(requests.len(),4));
    }
    #[test]fn simultaneous_requests_share_host_slots_and_other_hosts_stay_independent() {
        setup([response(429,"Retry-After: 1\r\n"),response(200,""),response(200,""),response(200,"")]);
        let first_cancel=MediaFetchCancellation::new();let second_cancel=MediaFetchCancellation::new();
        let other_cancel=MediaFetchCancellation::new();
        let mut first=Box::pin(get_image_bytes_shared("https://asset.test/first.png",5000,4096,&first_cancel));
        let mut second=Box::pin(get_image_bytes_shared("https://ASSET.test:443/second.png",5000,4096,&second_cancel));
        let mut other=Box::pin(get_image_bytes_shared("https://other.test/third.png",5000,4096,&other_cancel));
        let mut cx=Context::from_waker(core::task::Waker::noop());
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert!(second.as_mut().poll(&mut cx).is_pending());
        records(|requests|assert_eq!(requests.len(),1));
        assert_eq!(other.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        records(|requests|assert_eq!(requests.len(),2));
        CLOCK.with(|clock|clock.set(1000));
        assert_eq!(first.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        assert!(second.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(1999));
        assert!(second.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(2000));
        assert_eq!(second.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,0,1000,2000]));
    }
    #[test]fn distinct_ports_do_not_share_rate_limit_cooldown() {
        setup([response(429,"Retry-After: 1\r\n"),response(200,""),response(200,"")]);
        let first_cancel=MediaFetchCancellation::new();let other_cancel=MediaFetchCancellation::new();
        let mut first=Box::pin(get_image_bytes_shared("https://asset.test/first.png",5000,4096,&first_cancel));
        let mut other=Box::pin(get_image_bytes_shared("https://asset.test:8443/other.png",5000,4096,&other_cancel));
        let mut cx=Context::from_waker(core::task::Waker::noop());
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert_eq!(other.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        CLOCK.with(|clock|clock.set(1000));
        assert_eq!(first.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,0,1000]));
    }
    #[test]fn request_whose_shared_slot_exceeds_deadline_consumes_no_slot() {
        setup([response(429,"Retry-After: 1\r\n"),response(200,""),response(200,"")]);
        let first_cancel=MediaFetchCancellation::new();let short_cancel=MediaFetchCancellation::new();
        let third_cancel=MediaFetchCancellation::new();
        let mut first=Box::pin(get_image_bytes_shared("https://asset.test/first.png",5000,4096,&first_cancel));
        let mut short=Box::pin(get_image_bytes_shared("https://asset.test/short.png",500,4096,&short_cancel));
        let mut third=Box::pin(get_image_bytes_shared("https://asset.test/third.png",5000,4096,&third_cancel));
        let mut cx=Context::from_waker(core::task::Waker::noop());
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(short.as_mut().poll(&mut cx),Poll::Ready(Err(_))));
        assert!(third.as_mut().poll(&mut cx).is_pending());
        records(|requests|assert_eq!(requests.len(),1));
        CLOCK.with(|clock|clock.set(1000));
        assert_eq!(first.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        assert!(third.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(2000));
        assert_eq!(third.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,1000,2000]));
    }
    #[test]fn later_inflight_429_extends_an_already_reserved_host_wait() {
        setup([delayed_response(429,"Retry-After: 1\r\n",100),
            delayed_response(429,"Retry-After: 10\r\n",500),response(200,""),response(200,"")]);
        let first_cancel=MediaFetchCancellation::new();let second_cancel=MediaFetchCancellation::new();
        let mut first=Box::pin(get_image_bytes_shared("https://asset.test/first.png",15_000,4096,&first_cancel));
        let mut second=Box::pin(get_image_bytes_shared("https://asset.test/second.png",15_000,4096,&second_cancel));
        let mut cx=Context::from_waker(core::task::Waker::noop());
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert!(second.as_mut().poll(&mut cx).is_pending());
        records(|requests|assert_eq!(requests.len(),2));
        CLOCK.with(|clock|clock.set(100));
        assert!(first.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(500));
        assert!(second.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(1100));
        assert!(first.as_mut().poll(&mut cx).is_pending(),"an old slot must not bypass a later host cooldown");
        records(|requests|assert_eq!(requests.len(),2));
        CLOCK.with(|clock|clock.set(10_499));
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert!(second.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(10_500));
        assert_eq!(second.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        assert!(first.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(11_500));
        assert_eq!(first.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,0,10_500,11_500]));
        RETRY_LOGS.with(|logs| {
            let logs=logs.borrow();
            assert_eq!(logs.iter().filter(|log|log.get("event").map(String::as_str)==Some("\"retry\"")).count(),2);
            let extension=logs.iter().find(|log|log.get("event").map(String::as_str)==Some("\"image_backoff\"")).unwrap();
            assert_eq!(extension.get("reason").map(String::as_str),Some("\"cooldown-extended\""));
            assert_eq!(extension.get("delay_ms").map(String::as_str),Some("10400"));
            assert!(!extension.contains_key("attempt"),"cooldown extension must not count as another HTTP retry");
        });
    }
    #[test]fn later_cooldown_extension_preserves_each_original_deadline() {
        setup([delayed_response(429,"Retry-After: 1\r\n",100),
            delayed_response(429,"Retry-After: 10\r\n",500),response(200,"")]);
        let first_cancel=MediaFetchCancellation::new();let second_cancel=MediaFetchCancellation::new();
        let mut first=Box::pin(get_image_bytes_shared("https://asset.test/first.png",5000,4096,&first_cancel));
        let mut second=Box::pin(get_image_bytes_shared("https://asset.test/second.png",15_000,4096,&second_cancel));
        let mut cx=Context::from_waker(core::task::Waker::noop());
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert!(second.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(100));assert!(first.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(500));assert!(second.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(1100));
        assert!(matches!(first.as_mut().poll(&mut cx),Poll::Ready(Err(_))),"extended host cooldown cannot reset or exceed the first request budget");
        records(|requests|assert_eq!(requests.len(),2));
        CLOCK.with(|clock|clock.set(10_500));
        assert_eq!(second.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,0,10_500]));
    }
    #[test]fn cancellation_wakes_a_wait_that_was_extended_by_another_inflight_429() {
        use std::sync::{Arc,atomic::AtomicUsize};
        struct WakeCount(AtomicUsize);
        impl std::task::Wake for WakeCount {
            fn wake(self:Arc<Self>) {self.0.fetch_add(1,Ordering::Relaxed);}
            fn wake_by_ref(self:&Arc<Self>) {self.0.fetch_add(1,Ordering::Relaxed);}
        }
        setup([delayed_response(429,"Retry-After: 1\r\n",100),
            delayed_response(429,"Retry-After: 10\r\n",500),response(200,"")]);
        let count=Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker=core::task::Waker::from(count.clone());let mut cx=Context::from_waker(&waker);
        let first_cancel=MediaFetchCancellation::new();let second_cancel=MediaFetchCancellation::new();
        let mut first=Box::pin(get_image_bytes_shared("https://asset.test/first.png",15_000,4096,&first_cancel));
        let mut second=Box::pin(get_image_bytes_shared("https://asset.test/second.png",15_000,4096,&second_cancel));
        assert!(first.as_mut().poll(&mut cx).is_pending());
        assert!(second.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(100));assert!(first.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(500));assert!(second.as_mut().poll(&mut cx).is_pending());
        CLOCK.with(|clock|clock.set(1100));assert!(first.as_mut().poll(&mut cx).is_pending());
        count.0.store(0,Ordering::Relaxed);first_cancel.cancel();
        assert!(count.0.load(Ordering::Relaxed)>0,"extended retry wait did not register cancellation waker");
        assert_eq!(first.as_mut().poll(&mut cx),Poll::Ready(Err("cancelled".into())));
        records(|requests|assert_eq!(requests.len(),2));
        CLOCK.with(|clock|clock.set(10_500));
        assert_eq!(second.as_mut().poll(&mut cx),Poll::Ready(Ok(b"png".to_vec())));
        records(|requests|assert_eq!(requests.iter().map(|r|r.started).collect::<Vec<_>>(),[0,0,10_500]));
    }
}
'''


if __name__ == "__main__":
    admission = "mod r {pub mod services {pub mod hull_image_policy {\n" + "\n".join([
        "const MAX_URL_BYTES:usize=4096; const ERR_INVALID:i32=-1; const ERR_UNSUPPORTED:i32=-8;",
        CANONICAL_URL,
        "}}}",
    ])
    with tempfile.TemporaryDirectory(prefix="image-https-retry-") as directory:
        path = Path(directory)
        policy = f'#[path = "{IMAGE_POLICY}"] mod image_retry;\n'
        diagnostics = "mod download_diagnostics {\n" + PUBLIC_URL + "}\n"
        (path / "test.rs").write_text(STUBS + policy + admission + diagnostics + PRODUCTION + TESTS)
        subprocess.run([
            "rustc", "--edition=2024", "--crate-type=rlib", "--crate-name=httpdate",
            str(ROOT / "vendor/httpdate-1.0.3/src/lib.rs"), "-o", str(path / "libhttpdate.rlib"),
        ], check=True)
        subprocess.run([
            "rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "test"),
            "--extern", f"httpdate={path / 'libhttpdate.rlib'}",
        ], check=True)
        subprocess.run([str(path / "test"), "--test-threads=1"], check=True)
