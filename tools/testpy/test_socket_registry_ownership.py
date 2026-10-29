#!/usr/bin/env python3
"""Exercise production socket registries with guest allocation tracking."""

import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "src/r/net/socket_cabi.rs"

HARNESS = r'''#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as v;
extern crate self as trueos_vm;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
thread_local! {
    static OWNER: Cell<Option<u8>> = const { Cell::new(None) };
    static HOST_DEPTH: Cell<usize> = const { Cell::new(0) };
}
static GUEST_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
struct TrackingAllocator;
unsafe impl std::alloc::GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        if OWNER.with(|owner| owner.get().is_some()) && HOST_DEPTH.with(|depth| depth.get() == 0) {
            GUEST_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}
#[global_allocator] static ALLOCATOR: TrackingAllocator = TrackingAllocator;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[macro_export] macro_rules! log { ($($tt:tt)*) => {} }
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T {
        struct Restore(usize);
        impl Drop for Restore { fn drop(&mut self) { super::HOST_DEPTH.with(|d| d.set(self.0)); } }
        let _restore = Restore(super::HOST_DEPTH.with(|d| d.replace(d.get() + 1)));
        f()
    }
}
mod hv {
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { super::OWNER.with(|v| v.get()) }
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { None }
}
mod net { pub fn device_count() -> usize { 1 } }
mod wait {
    pub fn spin_until_timeout(_: u64, mut f: impl FnMut() -> bool) -> bool {
        for _ in 0..4 { if f() { return true; } }
        false
    }
    pub fn spin_step() { panic!("unexpected blocking wait"); }
    pub fn platform_wake_all_blueprint_io_waiters() -> usize { 0 }
}
#[path = @VNET@] pub mod vnet;
mod vmcall {
    pub const STATUS_OK: u64 = 0;
    pub const PAYLOAD_CAP: usize = 4096;
    @CONSTANTS@
    pub fn call(_: u32, _: u64, _: u64) -> (u64, u64) { panic!("host worker must not vmcall") }
    pub fn call_with_payload(_: u32, _: u64, _: u64, _: &[u8], _: &mut [u8]) -> (u64, u64) {
        panic!("host worker must not vmcall")
    }
}
static TUN_FAIL: AtomicBool = AtomicBool::new(false);
mod blueprint_net_broker {
    use crate::vnet::{self as api, Command, Event, NetHandle, SocketKind};
    pub struct VNetBridge { events: crate::Mutex<std::collections::VecDeque<Event>> }
    impl VNetBridge {
        pub fn open_primary() -> Option<Self> {
            Some(Self { events: crate::Mutex::new(std::collections::VecDeque::new()) })
        }
        pub fn submit(&self, command: Command) -> Result<(), ()> {
            let mut events = self.events.lock();
            let handle = NetHandle(1);
            match command {
                Command::OpenTcpConnect { .. } | Command::OpenTcpConnectV6 { .. } => {
                    events.push_back(Event::Opened { handle, kind: SocketKind::Tcp });
                    events.push_back(Event::TcpData { handle, data: api::ByteBuf::from_slice_trunc(&[42; 256]) });
                }
                Command::OpenTun { .. } => {
                    if crate::TUN_FAIL.load(std::sync::atomic::Ordering::Relaxed) {
                        events.push_back(Event::Error { msg: "refused" });
                    } else {
                        events.push_back(Event::Opened { handle, kind: SocketKind::Tun });
                        for _ in 0..8 {
                            events.push_back(Event::IpPacket { handle, packet: api::ByteBuf::from_slice_trunc(&[7; 256]) });
                        }
                    }
                }
                Command::Close { .. } => events.push_back(Event::Closed { handle }),
                _ => {},
            }
            Ok(())
        }
        pub fn pop_event(&self) -> Option<Event> { self.events.lock().pop_front() }
    }
}
#[path = @SOURCE@] mod socket_cabi;

#[test]
fn peer_registry_survives_retirement_and_reuse_without_guest_allocations() {
    use socket_cabi::*;
    OWNER.with(|v| v.set(Some(0)));
    let control = std::hint::black_box(vec![1u8; 32]);
    assert!(GUEST_ALLOCATIONS.load(Ordering::Relaxed) > 0);
    drop(control);
    GUEST_ALLOCATIONS.store(0, Ordering::Relaxed);

    let sockets: [i32; 192] = std::array::from_fn(|i| {
        OWNER.with(|v| v.set(Some((i % 2) as u8)));
        socket_tcp_open_host(2, 1, 6)
    });
    OWNER.with(|v| v.set(Some(0)));
    assert_eq!(socket_tcp_close_host(sockets[1] as u32), -9);
    assert_eq!(socket_tcp_set_nonblocking_host(sockets[0] as u32, 1), 0);
    assert_eq!(socket_tcp_connect_v4_host(sockets[0] as u32, 0x7f000001, 80u16.to_be(), 0), 0);
    let mut bytes = [0; 512];
    assert_eq!(socket_tcp_recv_host(sockets[0] as u32, &mut bytes, 0, 1, 0), 256);
    assert_eq!(&bytes[..256], &[42; 256]);

    let tun = tun_open_host(0, 0, [0; 16], 0, 1500);
    assert!(tun > 0);
    OWNER.with(|v| v.set(Some(1)));
    assert_eq!(tun_close_host(tun as u32), -9);
    OWNER.with(|v| v.set(Some(0)));
    assert_eq!(tun_recv_host(tun as u32, &mut bytes), Ok(Some(256)));
    assert_eq!(&bytes[..256], &[7; 256]);
    assert_eq!(tun_close_host(tun as u32), 0);
    TUN_FAIL.store(true, Ordering::Relaxed);
    assert_eq!(tun_open_host(0, 0, [0; 16], 0, 1500), -111);

    assert_eq!(close_sockets_for_vm(0), 96);
    OWNER.with(|v| v.set(Some(1)));
    assert_eq!(socket_tcp_set_nonblocking_host(sockets[1] as u32, 1), 0);
    OWNER.with(|v| v.set(Some(0)));
    for _ in 0..192 { assert!(socket_tcp_open_host(2, 1, 6) > 0); }
    assert_eq!(close_sockets_for_vm(0), 192);
    OWNER.with(|v| v.set(Some(1)));
    assert_eq!(close_sockets_for_vm(1), 96);
    assert_eq!(HOST_DEPTH.with(|v| v.get()), 0);
    OWNER.with(|v| v.set(None));
    assert_eq!(GUEST_ALLOCATIONS.load(Ordering::Relaxed), 0, "persistent host socket work allocated in a guest domain");
}
'''


def main():
    operations = sorted(set(re.findall(r"trueos_vm::vmcall::(OP_[A-Z0-9_]+)", SOURCE.read_text())))
    constants = "\n".join(f"pub const {name}: u32 = {i};" for i, name in enumerate(operations, 1))
    harness = HARNESS.replace("@SOURCE@", json.dumps(str(SOURCE))).replace(
        "@VNET@", json.dumps(str(ROOT / "crates/trueos-v/src/vnet.rs"))
    ).replace("@CONSTANTS@", constants)
    with tempfile.TemporaryDirectory(prefix="trueos-socket-ownership-") as temporary:
        directory = Path(temporary)
        source = directory / "tests.rs"
        source.write_text(harness)
        executable = directory / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], check=True, timeout=12)
        subprocess.run([str(executable)], check=True, timeout=12)


if __name__ == "__main__":
    main()
