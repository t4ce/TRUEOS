#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as v;
extern crate self as trueos_vm;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::{Poll, Waker};

pub struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self { Self(StdMutex::new(value)) }
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
pub mod vnetfs { #[path="@SDK@"] pub mod http; }
pub mod allocators { pub fn with_host_alloc_domain_strong<T>(f:impl FnOnce()->T)->T { f() } }
pub mod remote_work_wake { pub fn wake_cpu_for_remote_work(_:usize) {} }
pub mod hv {
    use super::*;
    pub static GENERATION: AtomicU64=AtomicU64::new(7);
    pub fn vm_run_generation(id:u8)->Option<u64> { (id<2).then(||GENERATION.load(Ordering::SeqCst)) }
    pub fn current_hull_guest_context_vm_id()->Option<u8> { None }
}
pub mod native_abi {
    pub fn abi_read_bytes<'a>(p:*const u8,n:usize)->Option<&'a[u8]> { Some(unsafe{std::slice::from_raw_parts(p,n)}) }
    pub fn abi_write_bytes<'a>(p:*mut u8,n:usize)->Option<&'a mut[u8]> { if n==0 {Some(&mut[])} else {Some(unsafe{std::slice::from_raw_parts_mut(p,n)})} }
}
pub mod vmcall {
    pub const OP_BP_HTTP_REQUEST_START_V1:u32=0x250;
    pub const OP_BP_HTTP_REQUEST_HEAD_V1:u32=0x251;
    pub const OP_BP_HTTP_REQUEST_READ_V1:u32=0x252;
    pub const OP_BP_HTTP_REQUEST_DISCARD_V1:u32=0x253;
    pub const STATUS_OK:u32=0;
    pub static RESULT:crate::Mutex<(u32,u64,usize)>=crate::Mutex::new((1,0,0));
    pub fn call_with_payload_reported_len(_:u32,_:u64,_:u64,_:&[u8],_:&mut[u8])->(u32,u64,usize) {*RESULT.lock()}
}
pub mod wait {
    use super::*;
    pub struct WaitQueue { seq:AtomicUsize, waiters:StdMutex<Vec<Waker>> }
    impl WaitQueue {
        pub const fn new()->Self {Self{seq:AtomicUsize::new(0),waiters:StdMutex::new(Vec::new())}}
        pub fn observe(&self)->u32 {self.seq.load(Ordering::SeqCst) as u32}
        pub async fn wait_after(&self,observed:u32) {
            std::future::poll_fn(|cx|{
                let mut waiters=self.waiters.lock().unwrap();
                if self.observe()!=observed {return Poll::Ready(());}
                if !waiters.iter().any(|w|w.will_wake(cx.waker())) {waiters.push(cx.waker().clone());}
                Poll::Pending
            }).await
        }
        pub fn notify_all(&self)->usize {
            self.seq.fetch_add(1,Ordering::SeqCst);
            let waiters=std::mem::take(&mut *self.waiters.lock().unwrap());let n=waiters.len();
            for waker in waiters {waker.wake();} n
        }
        pub fn notify_one(&self)->bool {self.notify_all()!=0}
    }
}
pub mod r {
    #[path="/home/t4ce/Repositories/TRUEOS/src/r/cabi_codes.rs"] pub mod cabi_codes;
    pub mod net {
        #[path="@KERNEL@"] pub mod http_request_cabi;
        pub mod request_stream {
            pub async fn start_exchange(_:crate::vnetfs::http::RequestOwned,_:super::http_request_cabi::ResponseSink) {}
        }
    }
}
mod vcabi {
    use super::*;
    pub static DISCARDS:AtomicUsize=AtomicUsize::new(0);
    pub struct Mock {pub start:i32,pub query:isize,pub head_read:isize,pub read:isize,pub discard:i32,pub head:Vec<u8>,pub last_cap:usize}
    impl Default for Mock {fn default()->Self {Self{start:17,query:-116,head_read:-116,read:-116,discard:0,head:Vec::new(),last_cap:0}}}
    pub static MOCK:Mutex<Option<Mock>>=Mutex::new(None);
    fn mocked<T>(f:impl FnOnce(&mut Mock)->T)->Option<T> {MOCK.lock().as_mut().map(f)}
    pub unsafe fn trueos_cabi_net_http_request_start_v1(p:*const u8,n:usize)->i32 {mocked(|m|m.start).unwrap_or_else(||unsafe{r::net::http_request_cabi::trueos_cabi_net_http_request_start_v1(p,n)})}
    pub unsafe fn trueos_cabi_net_http_request_head_v1(id:u32,p:*mut u8,n:usize)->isize {
        if let Some(value)=mocked(|m|{if n==0 {m.query} else {let copied=m.head.len().min(n);unsafe{std::ptr::copy_nonoverlapping(m.head.as_ptr(),p,copied)};m.head_read}}) {return value;}
        unsafe{r::net::http_request_cabi::trueos_cabi_net_http_request_head_v1(id,p,n)}
    }
    pub unsafe fn trueos_cabi_net_http_request_read_v1(id:u32,p:*mut u8,n:usize)->isize {if let Some(value)=mocked(|m|{m.last_cap=n;m.read}) {return value;} unsafe{r::net::http_request_cabi::trueos_cabi_net_http_request_read_v1(id,p,n)}}
    pub fn trueos_cabi_net_http_request_discard_v1(id:u32)->i32 {DISCARDS.fetch_add(1,Ordering::SeqCst);mocked(|m|m.discard).unwrap_or_else(||r::net::http_request_cabi::trueos_cabi_net_http_request_discard_v1(id))}
}

// The state tests are appended inside the actual production module, allowing
// direct producer/registry control without adding a product testing API.
