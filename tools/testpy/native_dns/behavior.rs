//! Complete SDK native DNS client, with the real kernel value adapter inserted.
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::{cell::Cell, sync::Mutex};
#[path = "../native_tokio/boundary.rs"] mod boundary;
static SUBMIT_MODE: AtomicI32 = AtomicI32::new(0);
static DNS_CODE: AtomicU32 = AtomicU32::new(0);
static SUBMITS: AtomicUsize = AtomicUsize::new(0);
static HOST_CALLS: AtomicUsize = AtomicUsize::new(0);
static GUEST_CALLS: AtomicUsize = AtomicUsize::new(0);
static NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());
thread_local! {
    static WORKER: Cell<bool> = const { Cell::new(false) };
    static HULL: Cell<bool> = const { Cell::new(false) };
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        crate::HULL.with(|v| v.get().then_some(1))
    }
}
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_service_lane_submit_job(job: Box<dyn FnOnce() + Send + 'static>) -> i32 {
    SUBMITS.fetch_add(1, Ordering::SeqCst);
    let mode = SUBMIT_MODE.load(Ordering::SeqCst);
    if mode != 0 {
        drop(job);
        return if mode == 999 { 0 } else { mode };
    }
    std::thread::spawn(move || {
        WORKER.with(|v| v.set(true));
        job();
        boundary::cleanup_tls();
    });
    0
}
mod kernel {
    use super::*;
    // @KERNEL_VALUE_ADAPTER@
    fn resolve_ipv4_for_sync_abi_host(host: &str) -> Result<[u8; 4], DnsResolveError> {
        assert!(WORKER.with(Cell::get), "DNS network work ran outside its explicit worker");
        HOST_CALLS.fetch_add(1, Ordering::SeqCst);
        NAMES.lock().unwrap().push(host.to_owned());
        match DNS_CODE.load(Ordering::SeqCst) {
            0 => Ok([192, 0, 2, 10]),
            1 => Err(DnsResolveError::Runtime),
            2 => Err(DnsResolveError::BadName),
            3 => Err(DnsResolveError::NoNic),
            4 => Err(DnsResolveError::Timeout),
            5 => Err(DnsResolveError::NoAnswer),
            _ => unreachable!(),
        }
    }
    fn resolve_ipv4_for_sync_abi_guest_vmcall(_: &str) -> Result<[u8; 4], DnsResolveError> {
        GUEST_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok([198, 51, 100, 7])
    }
}
fn clear() {
    SUBMIT_MODE.store(0, Ordering::SeqCst);
    DNS_CODE.store(0, Ordering::SeqCst);
    SUBMITS.store(0, Ordering::SeqCst);
    HOST_CALLS.store(0, Ordering::SeqCst);
    GUEST_CALLS.store(0, Ordering::SeqCst);
    NAMES.lock().unwrap().clear();
}
fn main() {
    use trueos::platform::io::ErrorKind;
    let runtime = trueos::runtime::current_thread().build().unwrap();
    clear();
    for name in ["127.0.0.1", "::1", "2001:db8::42"] {
        let addresses = runtime.block_on(trueos::net::resolve_host(name, 443)).unwrap();
        assert_eq!(addresses, vec![core::net::SocketAddr::new(name.parse().unwrap(), 443)]);
    }
    assert_eq!(SUBMITS.load(Ordering::SeqCst), 0);
    assert_eq!(HOST_CALLS.load(Ordering::SeqCst), 0);
    let addresses = runtime.block_on(trueos::net::resolve_host("example.test", 22)).unwrap();
    assert_eq!(addresses, [core::net::SocketAddr::from(([192, 0, 2, 10], 22))]);
    assert_eq!(&*NAMES.lock().unwrap(), &["example.test"]);
    assert_eq!(SUBMITS.load(Ordering::SeqCst), 1);
    for (code, kind) in [(1, ErrorKind::Other), (2, ErrorKind::InvalidInput), (3, ErrorKind::NotFound), (4, ErrorKind::TimedOut), (5, ErrorKind::NotFound)] {
        clear();
        DNS_CODE.store(code, Ordering::SeqCst);
        assert_eq!(runtime.block_on(trueos::net::resolve_host("error.test", 80)).unwrap_err().kind(), kind);
        assert_eq!(HOST_CALLS.load(Ordering::SeqCst), 1);
    }
    for (status, kind) in [(-2, ErrorKind::WouldBlock), (-5, ErrorKind::InvalidInput), (-6, ErrorKind::Other), (-12, ErrorKind::Other)] {
        clear();
        SUBMIT_MODE.store(status, Ordering::SeqCst);
        assert_eq!(runtime.block_on(trueos::net::resolve_host("rejected.test", 80)).unwrap_err().kind(), kind);
        assert_eq!(HOST_CALLS.load(Ordering::SeqCst), 0);
    }
    clear();
    SUBMIT_MODE.store(999, Ordering::SeqCst);
    assert_eq!(runtime.block_on(trueos::net::resolve_host("dropped.test", 80)).unwrap_err().kind(), ErrorKind::Other);
    assert_eq!(HOST_CALLS.load(Ordering::SeqCst), 0);
    clear();
    HULL.with(|v| v.set(true));
    assert_eq!(kernel::trueos_platform_dns_resolve_ipv4("guest.test"), Ok([198, 51, 100, 7]));
    assert_eq!(HOST_CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(GUEST_CALLS.load(Ordering::SeqCst), 1);
    HULL.with(|v| v.set(false));
    drop(runtime);
    boundary::cleanup_tls();
    println!("Native DNS: numeric v4/v6, worker ownership, five resolver errors, four admission errors, dropped completion and guest routing passed");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_write(_:u32,_:*const u8,_:usize){}
