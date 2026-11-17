#!/usr/bin/env python3
"""Exercise production media keep-alive exchanges with a deterministic TLS peer.

HTTP parsing, connection retention, retry, redirect, identity and cancellation
logic are production code. Only queues, socket events, DNS and the clock are
host fixtures; no kernel build or network connection is required.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "src/r/net/https.rs").read_text()


def between(start, end):
    first = SOURCE.index(start)
    return SOURCE[first:SOURCE.index(end, first + len(start))]


production = "\n".join([
    between("struct JsonPostCancellation {", "#[derive(Clone, Copy, Debug, Eq, PartialEq)]"),
    between("struct JsonPostCancelled;", "fn register_json_post_cancellation("),
    between("struct FetchTarget {", "#[derive(Clone, Copy)]\nstruct HttpsClientQueues"),
    between("fn request_has_header(", "fn parse_fetch_url("),
    between("fn parse_fetch_url(", "fn leak_str("),
    between("fn find_http_header_end(", "fn decode_response_content("),
    between("fn fetch_target_url(", "async fn request_https_bytes("),
    between("fn media_response_allows_reuse(", "pub async fn get_bytes_shared("),
    between("pub(crate) struct MediaRangeClient {", "pub(crate) async fn get_media_bytes_profile_shared("),
    "#[derive(Clone, Debug, Eq, PartialEq)]\n" + between("pub(crate) struct MediaRangeIdentity {", "pub async fn post_protobuf_shared("),
])

stubs = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::sync::Arc;
use core::future::Future;
use core::sync::atomic::{AtomicBool,AtomicU32,Ordering};
use core::task::{Context,Poll};
use std::cell::{Cell,RefCell};
use std::collections::VecDeque;
const HTTPS_REQUEST_CANCELLED:&str="cancelled";
const DEFAULT_USER_AGENT:&str="TRUEOS test";
const HTTPS_EVENT_QUEUE_DEPTH:usize=1024;
const HTTPS_EVENT_DRAIN_MAX:usize=512;
const HTTPS_PROBE_HEADER_MAX:usize=32768;
const HTTPS_RESPONSE_OVERHEAD_MAX:usize=4096;
const HTTPS_IDLE_SLEEP_MS:u64=100;
struct AtomicWaker;
impl AtomicWaker { fn new()->Self{Self} fn register(&self,_:&core::task::Waker){} fn wake(&self){} }
#[derive(Clone,Copy)]struct EmbassyDuration(u64);
impl EmbassyDuration {
 fn from_millis(n:u64)->Self{Self(n)} fn from_micros(n:u64)->Self{Self(n/1000)} fn as_millis(self)->u64{self.0}
}
#[derive(Clone,Copy)]struct Instant(u64);
impl Instant {
 fn now()->Self{Self(CLOCK.with(Cell::get))}
 fn saturating_duration_since(self,other:Self)->EmbassyDuration{EmbassyDuration(self.0.saturating_sub(other.0))}
}
impl core::ops::Add<EmbassyDuration> for Instant {type Output=Self;fn add(self,n:EmbassyDuration)->Self{Self(self.0+n.0)}}
impl PartialEq for Instant {fn eq(&self,other:&Self)->bool{self.0==other.0}}
impl PartialOrd for Instant {fn partial_cmp(&self,other:&Self)->Option<core::cmp::Ordering>{self.0.partial_cmp(&other.0)}}
struct Timer;
struct Sleep(Instant);
impl Timer {fn after(n:EmbassyDuration)->Sleep{Sleep(Instant::now()+n)} fn at(n:Instant)->Sleep{Sleep(n)}}
impl Future for Sleep {
 type Output=();fn poll(self:core::pin::Pin<&mut Self>,_:&mut Context<'_>)->Poll<()>{
  if Instant::now()>=self.0 {Poll::Ready(())}else{Poll::Pending}
 }
}
mod r {pub mod readiness {pub const NET_ANY_CONFIGURED:u32=1;pub const TLS_SOCKET_SERVICE_READY:u32=2;pub async fn wait_for(_:u32){}}}
mod vnet {pub type NetHandle=u32;pub struct EndpointV4{pub addr:[u8;4],pub port:u16}}
#[derive(Clone)]struct TlsClientConfig;
#[derive(Clone)]struct TlsRoots;
struct TlsTimeouts{connect_ms:u32,tls_ms:u32,idle_ms:u32}
struct TlsCancelCompletion;
impl TlsCancelCompletion{fn new()->Self{Self}}
enum TlsCommand {
 OpenTcpConnect{remote:vnet::EndpointV4,server_name:String,cfg:TlsClientConfig,roots:TlsRoots,timeouts:TlsTimeouts},
 Send{handle:u32,data:Vec<u8>},Close{handle:u32},CancelOwner{completion:Arc<TlsCancelCompletion>},
}
enum TlsEvent {Opened{handle:u32},Connected{handle:u32},Data{handle:u32,data:Vec<u8>},Closed{handle:u32},Error{msg:&'static str},TlsError{err:&'static str}}
struct Queue<T>{data:std::sync::Mutex<VecDeque<T>>,generation:AtomicU32}
impl<T> Queue<T>{
 fn new()->Self{Self{data:std::sync::Mutex::new(VecDeque::new()),generation:AtomicU32::new(0)}}
 fn drain(&self,n:usize)->Vec<T>{let mut q=self.data.lock().unwrap();let n=n.min(q.len());let out=q.drain(..n).collect();if n>0{self.generation.fetch_add(1,Ordering::Relaxed);}out}
 fn change_generation(&self)->u32{self.generation.load(Ordering::Relaxed)}
 async fn wait_change_after(&self,observed:u32){core::future::poll_fn(|_|if self.change_generation()!=observed{Poll::Ready(())}else{Poll::Pending}).await}
}
impl Queue<TlsEvent>{fn push(&self,event:TlsEvent){self.data.lock().unwrap().push_back(event);self.generation.fetch_add(1,Ordering::Relaxed);}}
impl Queue<TlsCommand>{
 fn push(&self,command:TlsCommand)->Result<(),()>{
  let events=EVENTS.with(Cell::get).unwrap();
  match command {
   TlsCommand::OpenTcpConnect{..}=>{let handle=PEER.with(|p|{let mut p=p.borrow_mut();p.opens+=1;p.opens as u32});events.push(TlsEvent::Opened{handle});events.push(TlsEvent::Connected{handle});}
   TlsCommand::Send{handle,data}=>{
    let step=PEER.with(|p|{let mut p=p.borrow_mut();p.wires.push(String::from_utf8(data).unwrap());p.steps.pop_front().expect("unexpected extra send")});
    match step {
     Step::Reply(bytes,close)=>{events.push(TlsEvent::Data{handle,data:bytes});if close{events.push(TlsEvent::Closed{handle});}}
     Step::Disconnect=>events.push(TlsEvent::Closed{handle}),
     Step::Pending=>{},
    }
   }
   TlsCommand::Close{..}=>PEER.with(|p|p.borrow_mut().closes+=1),
   TlsCommand::CancelOwner{..}=>{PEER.with(|p|p.borrow_mut().fences+=1);let _=events.drain(usize::MAX);}
  }Ok(())
 }
}
#[derive(Default)]struct Peer{steps:VecDeque<Step>,wires:Vec<String>,opens:usize,closes:usize,fences:usize,allocations:usize,dns:usize}
enum Step{Reply(Vec<u8>,bool),Disconnect,Pending}
thread_local!{
 static CLOCK:Cell<u64>=const{Cell::new(0)};
 static PEER:RefCell<Peer>=RefCell::new(Peer::default());
 static EVENTS:Cell<Option<&'static Queue<TlsEvent>>>=const{Cell::new(None)};
}
#[derive(Clone,Copy)]struct HttpsClientQueues{device_index:usize,cmds:&'static Queue<TlsCommand>,events:&'static Queue<TlsEvent>}
struct HttpsJsonClient{queues:Option<HttpsClientQueues>,tls_config:TlsClientConfig,roots:TlsRoots,resolved:Option<String>}
impl HttpsJsonClient{
 fn new()->Self{Self{queues:None,tls_config:TlsClientConfig,roots:TlsRoots,resolved:None}}
 fn queues_for_device(&mut self,device:usize)->HttpsClientQueues{
  *self.queues.get_or_insert_with(||{PEER.with(|p|p.borrow_mut().allocations+=1);let cmds=Box::leak(Box::new(Queue::new()));let events=Box::leak(Box::new(Queue::new()));EVENTS.with(|e|e.set(Some(events)));HttpsClientQueues{device_index:device,cmds,events}})
 }
 async fn resolve_host(&mut self,_:usize,host:&str,_:u32)->Result<[u8;4],String>{if self.resolved.as_deref()!=Some(host){PEER.with(|p|p.borrow_mut().dns+=1);self.resolved=Some(host.into());}Ok([127,0,0,1])}
}
#[derive(Default)]struct NetProfile;
impl NetProfile{fn resolve_device_index(&self)->Option<usize>{Some(0)}}
async fn fence_https_owner(cmds:&'static Queue<TlsCommand>)->bool{cmds.push(TlsCommand::CancelOwner{completion:Arc::new(TlsCancelCompletion::new())}).unwrap();true}
fn log_download_response(_: &FetchTarget,_:[u8;4],_:&HttpsRequest<'_>,_:&[u8],_:&HttpsJsonResponse,_:Instant,_:Option<u64>,_:Option<u64>){}
fn decode_response_content(response:HttpsJsonResponse,_:usize)->Result<HttpsJsonResponse,String>{Ok(response)}
async fn request_https_response_once_cancellable(_: &FetchTarget,_:&HttpsRequest<'_>,_:u32,_:usize,_:Option<&JsonPostCancellation>)->Result<HttpsJsonResponse,String>{panic!("media client fell back to one-shot transport")}
'''

tests = r'''
#[cfg(test)]mod tests {
 use super::*;
 fn run<T>(future:impl Future<Output=T>)->T{let mut f=Box::pin(future);let mut cx=Context::from_waker(core::task::Waker::noop());for _ in 0..100{if let Poll::Ready(x)=f.as_mut().poll(&mut cx){return x}CLOCK.with(|c|c.set(c.get()+1));}panic!("future stalled")}
 fn setup(steps:impl IntoIterator<Item=Step>){CLOCK.with(|c|c.set(0));PEER.with(|p|*p.borrow_mut()=Peer{steps:steps.into_iter().collect(),..Default::default()});}
 fn reply(offset:u64,body:&[u8],extra:&str,close:bool)->Step{let header=format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/8\r\nETag: \"v1\"\r\n{}\r\n",body.len(),offset,offset+body.len() as u64-1,extra);Step::Reply([header.as_bytes(),body].concat(),close)}
 fn range(client:&mut MediaRangeClient,offset:u64,identity:Option<&MediaRangeIdentity>)->Result<MediaRangeResponse,String>{run(client.get_range("https://media.test/movie.mp4",offset,4,50,&MediaFetchCancellation::new(),None,None,identity))}
 #[test]fn adjacent_ranges_reuse_the_connection_and_queue_owner(){
  setup([reply(0,b"abcd","",false),reply(4,b"efgh","",false)]);let mut client=MediaRangeClient::new();
  let first=range(&mut client,0,None).unwrap();assert_eq!(range(&mut client,4,Some(&first.identity)).unwrap().bytes,b"efgh");
  PEER.with(|p|{let p=p.borrow();assert_eq!((p.opens,p.allocations,p.dns,p.fences),(1,1,1,0));assert!(p.wires.iter().all(|w|w.contains("Connection: keep-alive\r\n")));});
  run(client.close());assert!(client.connection.is_none());PEER.with(|p|assert_eq!(p.borrow().fences,1));
 }
 #[test]fn server_close_header_and_same_batch_close_are_never_retained(){
  for (headers,close) in [("Connection: close\r\n",false),("",true)]{
   setup([reply(0,b"abcd",headers,close),reply(4,b"efgh","",false)]);let mut client=MediaRangeClient::new();let first=range(&mut client,0,None).unwrap();assert!(client.connection.is_none());assert!(range(&mut client,4,Some(&first.identity)).is_ok());
   PEER.with(|p|{let p=p.borrow();assert_eq!((p.opens,p.allocations,p.fences),(2,1,1));});run(client.close());
  }
 }
 #[test]fn idle_server_close_retries_once_with_fresh_connection(){
  setup([reply(0,b"abcd","",false),Step::Disconnect,reply(4,b"efgh","",false)]);let mut client=MediaRangeClient::new();let first=range(&mut client,0,None).unwrap();assert_eq!(range(&mut client,4,Some(&first.identity)).unwrap().bytes,b"efgh");
  PEER.with(|p|{let p=p.borrow();assert_eq!((p.opens,p.wires.len(),p.fences),(2,3,1));assert_eq!(p.wires[1],p.wires[2]);});run(client.close());
 }
 #[test]fn repeated_disconnect_is_bounded_and_fenced(){
  setup([reply(0,b"abcd","",false),Step::Disconnect,Step::Disconnect]);let mut client=MediaRangeClient::new();let first=range(&mut client,0,None).unwrap();assert!(range(&mut client,4,Some(&first.identity)).is_err());assert!(client.connection.is_none());PEER.with(|p|{let p=p.borrow();assert_eq!((p.opens,p.wires.len()),(2,3));assert!(p.fences>=2);});
 }
 #[test]fn changed_range_identity_closes_the_retained_transport(){
  let bad=match reply(4,b"efgh","",false){Step::Reply(bytes,c)=>Step::Reply(String::from_utf8(bytes).unwrap().replace("\"v1\"","\"v2\"").into_bytes(),c),_=>unreachable!()};
  setup([reply(0,b"abcd","",false),bad]);let mut client=MediaRangeClient::new();let first=range(&mut client,0,None).unwrap();assert_eq!(range(&mut client,4,Some(&first.identity)).unwrap_err(),"media representation changed during playback");assert!(client.connection.is_none());PEER.with(|p|assert!(p.borrow().fences>=1));
 }
 #[test]fn timeout_cancels_and_fences_without_reconnect(){
  setup([Step::Pending]);let mut client=MediaRangeClient::new();let result=run(client.get_range("https://media.test/movie.mp4",0,4,3,&MediaFetchCancellation::new(),None,None,None));assert_eq!(result.unwrap_err(),"cancelled");assert!(client.connection.is_none());PEER.with(|p|{let p=p.borrow();assert_eq!(p.opens,1);assert!(p.fences>=1);});
 }
 #[test]fn retention_requires_exact_http11_length_framing(){
  for raw in [b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok".as_slice(),b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nokextra",b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\nok",b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nConnection: close\r\nContent-Length: 2\r\n\r\nok",b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n X-Extra: folded\r\n\r\nok",b"HTTP/1.1 200 OK\r\n\r\nok"]{assert!(!media_response_allows_reuse(raw));}
  assert!(media_response_allows_reuse(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"));
 }
}
'''

if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="media-range-client-") as directory:
        path = Path(directory)
        (path / "test.rs").write_text(stubs + production + tests)
        subprocess.run(["rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "test")], check=True)
        subprocess.run([str(path / "test"), "--test-threads=1"], check=True)
