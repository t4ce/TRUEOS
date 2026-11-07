#!/usr/bin/env python3
"""Exercise production native Mio ownership, readiness and network wake edges.

The fixture compiles extracted production functions unchanged. Its boundaries
provide socket storage, execution identities, a guest-call bridge and event
queues; readiness masks, authorization, counters, wake selection, generation
advancement and adapter event publication execute the actual source bodies.
This is a host contract test, not a NIC, VMX or end-to-end timing measurement.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
TOOLCHAIN = "nightly-2026-07-10"


def item(source: str, declaration: str) -> str:
    """Keep a complete item, matching its own closing indentation."""
    match = re.search(rf"(?m)^(?P<indent>[ \t]*)(?:{declaration})", source)
    if match is None:
        raise ValueError(f"production item not found: {declaration}")
    start = match.start()
    end = source.index("\n" + match["indent"] + "}", match.end())
    return source[start:end + len(match["indent"]) + 2]


def function(source: str, name: str) -> str:
    return item(source, rf'(?:pub(?:\(crate\))? )?(?:const )?(?:extern "C" )?fn {name}\(')


def constant(source: str, name: str) -> str:
    match = re.search(rf"(?m)^(?:pub(?:\(crate\))? )?const {name}:.*;$", source)
    if match is None:
        raise ValueError(f"production constant not found: {name}")
    return match[0]


HARNESS = r'''#![allow(dead_code)]
extern crate alloc;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::task::{Wake, Waker};
#[path = @VNET@] mod api;

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
    pub fn try_lock(&self) -> Option<std::sync::MutexGuard<'_, T>> { self.0.try_lock().ok() }
}
thread_local! {
    static THREAD_VM: Cell<Option<u8>> = const { Cell::new(None) };
    static BROKER_VM: Cell<Option<u8>> = const { Cell::new(None) };
    static HULL_VM: Cell<Option<u8>> = const { Cell::new(None) };
    static THREAD_QUERIES: Cell<usize> = const { Cell::new(0) };
    static BROKER_QUERIES: Cell<usize> = const { Cell::new(0) };
    static HULL_QUERIES: Cell<usize> = const { Cell::new(0) };
    static GUEST_CALLS: Cell<usize> = const { Cell::new(0) };
    static GUEST_FAIL: Cell<bool> = const { Cell::new(false) };
}
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() }
}
mod allcaps { pub mod hv { pub const CODEX_PROGRESS_PROBES_ENABLED: bool = true; } }
mod r { pub mod threads {
    pub fn current_vm_id() -> Option<u8> {
        crate::THREAD_QUERIES.with(|n| n.set(n.get() + 1));
        crate::THREAD_VM.with(|v| v.get())
    }
} }
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        crate::HULL_QUERIES.with(|n| n.set(n.get() + 1));
        crate::HULL_VM.with(|v| v.get())
    }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> {
        crate::BROKER_QUERIES.with(|n| n.set(n.get() + 1));
        crate::BROKER_VM.with(|v| v.get())
    }
    pub mod vmcall {
        pub const OP_BP_MIO_SOCKET_POLL_READY: u32 = 59;
        pub const STATUS_OK: u64 = 0;
        pub fn guest_call(op: u32, socket: u64, interests: u64) -> (u64, u64) {
            assert_eq!(op, OP_BP_MIO_SOCKET_POLL_READY);
            crate::GUEST_CALLS.with(|n| n.set(n.get() + 1));
            if crate::GUEST_FAIL.with(|v| v.get()) { return (1, 255); }
            let owner = crate::HULL_VM.with(|v| v.get()).expect("Hull identity");
            match crate::mio::mio_socket_poll_ready_for_vm_host(owner, socket as u32, interests as u8) {
                Some(ready) => (STATUS_OK, u64::from(ready)),
                None => (1, 0),
            }
        }
    }
}

mod mio {
    use super::*;
    #[path = @PROGRESS@] pub(super) mod progress;
    @MIO_CONSTANTS@
    #[derive(Clone, Copy, PartialEq, Eq)]
    @SOCKET_KIND@
    #[derive(Clone, Copy)]
    @COMPAT_ADDR@
    @SOCKET_STATE@
    struct MioCompat {
        sockets: Vec<MioSocketState>,
        progress: [progress::Counters; 4],
        kicks: usize,
    }
    impl MioCompat {
        fn new() -> Self { Self { sockets: Vec::new(), progress: [progress::Counters::new(); 4], kicks: 0 } }
        // The network service is an instrumented boundary. Authorization and
        // readiness below are the production implementation.
        fn kick_net(&mut self) { self.kicks += 1; }
        @SOCKET_LOOKUP@
        @READY_MASK@
    }
    thread_local! { static COMPAT: RefCell<MioCompat> = RefCell::new(MioCompat::new()); }
    fn with_compat<T>(f: impl FnOnce(&mut MioCompat) -> T) -> T {
        COMPAT.with(|v| f(&mut v.borrow_mut()))
    }
    @MIO_FUNCTIONS@

    fn socket(id: u32, owner_vm: Option<u8>, kind: MioSocketKind) -> MioSocketState {
        MioSocketState {
            id, owner_vm, progress_generation: 7, kind, handle: Some(api::NetHandle(id)),
            listen_handles: Vec::new(), local: None, peer: None, listen_port: None,
            connected: true, closed: false, read_shutdown: false, write_shutdown: false,
            peer_eof: false, error: STATUS_OK, tx_in_flight: 0, rx_stream: VecDeque::new(),
            rx_dgrams: VecDeque::new(), accept_queue: VecDeque::new(),
        }
    }
    fn reset() {
        with_compat(|c| {
            *c = MioCompat::new();
            c.sockets.push(socket(100, None, MioSocketKind::TcpStream));
            c.sockets.push(socket(101, Some(1), MioSocketKind::TcpStream));
            c.sockets.push(socket(102, Some(2), MioSocketKind::TcpStream));
        });
        THREAD_VM.with(|v| v.set(None)); BROKER_VM.with(|v| v.set(None)); HULL_VM.with(|v| v.set(None));
        THREAD_QUERIES.with(|v| v.set(0)); BROKER_QUERIES.with(|v| v.set(0));
        HULL_QUERIES.with(|v| v.set(0)); GUEST_CALLS.with(|v| v.set(0)); GUEST_FAIL.with(|v| v.set(false));
    }

    #[test]
    fn host_and_guest_namespaces_do_not_expose_each_others_socket_readiness() {
        reset();
        assert_eq!(trueos_mio_socket_poll_ready(100, READY_WRITABLE), i32::from(READY_WRITABLE));
        assert_eq!(trueos_mio_socket_poll_ready(101, READY_WRITABLE), STATUS_NOT_FOUND);
        assert_eq!(trueos_mio_socket_poll_ready(102, READY_WRITABLE), STATUS_NOT_FOUND);
        BROKER_VM.with(|v| v.set(Some(1)));
        assert_eq!(trueos_mio_socket_poll_ready(101, READY_WRITABLE), i32::from(READY_WRITABLE));
        for id in [100, 102, 999] {
            assert_eq!(trueos_mio_socket_poll_ready(id, READY_WRITABLE), STATUS_NOT_FOUND);
        }
        with_compat(|c| {
            assert_eq!(c.progress[1].ready_probes, 1);
            assert!(!c.progress[2].observed, "unauthorized query touched another owner's counters");
        });
    }

    #[test]
    fn logical_thread_identity_precedes_a_different_ambient_broker_context() {
        reset(); THREAD_VM.with(|v| v.set(Some(1))); BROKER_VM.with(|v| v.set(Some(2)));
        assert_eq!(trueos_mio_socket_poll_ready(101, READY_WRITABLE), i32::from(READY_WRITABLE));
        assert_eq!(trueos_mio_socket_poll_ready(102, READY_WRITABLE), STATUS_NOT_FOUND);
        assert_eq!(trueos_mio_socket_poll_ready(100, READY_WRITABLE), STATUS_NOT_FOUND);
        assert_eq!(THREAD_QUERIES.with(|v| v.get()), 3);
        assert_eq!(BROKER_QUERIES.with(|v| v.get()), 0, "fallback identity was queried despite a logical thread owner");
        THREAD_VM.with(|v| v.set(None));
        assert_eq!(trueos_mio_socket_poll_ready(102, READY_WRITABLE), i32::from(READY_WRITABLE));
        assert_eq!(BROKER_QUERIES.with(|v| v.get()), 1);
    }

    #[test]
    fn hull_uses_the_existing_guest_bridge_with_its_own_namespace_and_transport_status() {
        reset(); HULL_VM.with(|v| v.set(Some(2)));
        THREAD_VM.with(|v| v.set(Some(1))); BROKER_VM.with(|v| v.set(Some(1)));
        assert_eq!(trueos_mio_socket_poll_ready(102, READY_WRITABLE), i32::from(READY_WRITABLE));
        assert_eq!(trueos_mio_socket_poll_ready(101, READY_WRITABLE), STATUS_NOT_FOUND);
        assert_eq!(trueos_mio_socket_poll_ready(100, READY_WRITABLE), STATUS_NOT_FOUND);
        GUEST_FAIL.with(|v| v.set(true));
        assert_eq!(trueos_mio_socket_poll_ready(102, READY_WRITABLE), STATUS_NOT_FOUND);
        assert_eq!(GUEST_CALLS.with(|v| v.get()), 4);
        assert_eq!(THREAD_QUERIES.with(|v| v.get()), 0);
        assert_eq!(BROKER_QUERIES.with(|v| v.get()), 0);
    }

    #[test]
    fn all_invalid_interest_masks_are_rejected_before_identity_or_registry_queries() {
        reset(); HULL_VM.with(|v| v.set(Some(2)));
        for interest in 0u8..=255 {
            if matches!(interest, 1 | 2 | 3) { continue; }
            assert_eq!(trueos_mio_socket_poll_ready(102, interest), -4, "interest={interest}");
        }
        assert_eq!(HULL_QUERIES.with(|v| v.get()), 0);
        assert_eq!(THREAD_QUERIES.with(|v| v.get()), 0);
        assert_eq!(BROKER_QUERIES.with(|v| v.get()), 0);
        assert_eq!(GUEST_CALLS.with(|v| v.get()), 0);
        with_compat(|c| assert_eq!(c.kicks, 0));
    }

    #[test]
    fn tcp_readiness_preserves_empty_results_backpressure_shutdown_and_error_bits() {
        reset(); THREAD_VM.with(|v| v.set(Some(1)));
        assert_eq!(trueos_mio_socket_poll_ready(101, READY_READABLE), 0, "not ready is not not-found");
        with_compat(|c| c.sockets[1].rx_stream.push_back(42));
        assert_eq!(trueos_mio_socket_poll_ready(101, 3), 3);
        with_compat(|c| c.sockets[1].tx_in_flight = MIO_TCP_TX_WINDOW_BYTES);
        assert_eq!(trueos_mio_socket_poll_ready(101, 3), 1);
        with_compat(|c| {
            c.sockets[1].rx_stream.clear(); c.sockets[1].tx_in_flight = 0;
            c.sockets[1].read_shutdown = true;
        });
        assert_eq!(trueos_mio_socket_poll_ready(101, 3), i32::from(READY_READABLE | READY_WRITABLE | READY_READ_CLOSED));
        with_compat(|c| { c.sockets[1].read_shutdown = false; c.sockets[1].write_shutdown = true; });
        assert_eq!(trueos_mio_socket_poll_ready(101, 3), i32::from(READY_WRITE_CLOSED));
        with_compat(|c| { c.sockets[1].write_shutdown = false; c.sockets[1].peer_eof = true; });
        assert_eq!(trueos_mio_socket_poll_ready(101, READY_WRITABLE), i32::from(READY_WRITABLE | READY_READ_CLOSED));
        with_compat(|c| { c.sockets[1].error = STATUS_IO; c.sockets[1].closed = true; });
        assert_eq!(trueos_mio_socket_poll_ready(101, 3), i32::from(READY_READABLE | READY_ERROR | READY_READ_CLOSED | READY_WRITE_CLOSED));
        with_compat(|c| {
            let p = c.progress[1];
            assert_eq!((p.generation, p.ready_probes, p.readable_hits), (7, 7, 4));
            assert_eq!((p.read_calls, p.read_bytes), (0, 0), "readiness consumed stream data");
        });
    }

    #[test]
    fn listeners_and_udp_do_not_increment_tcp_counters_or_cross_generation_boundaries() {
        reset(); THREAD_VM.with(|v| v.set(Some(1)));
        with_compat(|c| {
            let mut listener = socket(103, Some(1), MioSocketKind::TcpListener);
            listener.accept_queue.push_back(200);
            c.sockets.push(listener);
            let mut udp = socket(104, Some(1), MioSocketKind::Udp);
            udp.rx_dgrams.push_back((CompatAddr::V4 { addr: [127,0,0,1], port: 7 }, vec![8]));
            c.sockets.push(udp);
        });
        assert_eq!(trueos_mio_socket_poll_ready(103, 3), i32::from(READY_READABLE));
        assert_eq!(trueos_mio_socket_poll_ready(104, 3), 3);
        with_compat(|c| {
            assert!(!c.progress[1].observed);
            c.progress[1].for_generation(9).unwrap().readiness(true);
        });
        assert_eq!(trueos_mio_socket_poll_ready(101, READY_WRITABLE), i32::from(READY_WRITABLE));
        with_compat(|c| assert_eq!((c.progress[1].generation, c.progress[1].ready_probes), (9, 1)));
    }
}

mod wait {
    use super::*;
    @WAIT_CONSTANTS@
    @WAIT_QUEUE@
    @QUEUED_WAKER@
    impl WaitQueue {
        @WAIT_NEW@
        @WAIT_NOTIFY_ALL@
    }
    static PLATFORM_WAIT_QUEUES: Mutex<BTreeMap<(u16, u64), Arc<WaitQueue>>> = Mutex::new(BTreeMap::new());
    @WAIT_FUNCTIONS@
    pub fn reset() { PLATFORM_WAIT_QUEUES.lock().clear(); }
    pub fn queue(vm: Option<u8>, key: u64) -> Arc<WaitQueue> {
        platform_wait_queue(vm.map_or(PLATFORM_WAIT_HOST_SCOPE, platform_wait_vm_scope), key)
    }
    pub fn observe(queue: &WaitQueue) -> u32 { queue.seq.load(Ordering::Acquire) }
    pub fn add_waiter(queue: &WaitQueue, waker: Waker) {
        let id = queue.next_waiter.fetch_add(1, Ordering::Relaxed);
        queue.wakers.lock().push(QueuedWaker { waiter: id, waker });
    }
    pub fn registry_len() -> usize { PLATFORM_WAIT_QUEUES.lock().len() }
    pub fn registry_unlocked() -> bool { PLATFORM_WAIT_QUEUES.try_lock().is_some() }
}

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::SeqCst); }
}
#[test]
fn io_wake_advances_host_and_all_guest_io_generations_but_leaves_other_keys_untouched() {
    wait::reset(); let key = wait::BLUEPRINT_IO_WAIT_KEY;
    let host = wait::queue(None, key); let guest1 = wait::queue(Some(1), key);
    let guest2_empty = wait::queue(Some(2), key);
    let host_other = wait::queue(None, key + 1); let guest_other = wait::queue(Some(1), key + 1);
    let host_wakes = Arc::new(WakeCount::default()); let guest_wakes = Arc::new(WakeCount::default());
    let other_wakes = Arc::new(WakeCount::default());
    for _ in 0..2 { wait::add_waiter(&host, Waker::from(host_wakes.clone())); }
    wait::add_waiter(&guest1, Waker::from(guest_wakes.clone()));
    wait::add_waiter(&host_other, Waker::from(other_wakes.clone()));
    wait::add_waiter(&guest_other, Waker::from(other_wakes.clone()));
    assert_eq!(wait::platform_wake_all_blueprint_io_waiters(), 3, "return actual waker count");
    assert_eq!((wait::observe(&host), wait::observe(&guest1), wait::observe(&guest2_empty)), (1,1,1));
    assert_eq!((wait::observe(&host_other), wait::observe(&guest_other)), (0,0));
    assert_eq!((host_wakes.0.load(Ordering::SeqCst), guest_wakes.0.load(Ordering::SeqCst), other_wakes.0.load(Ordering::SeqCst)), (2,1,0));
    assert_eq!(wait::platform_wake_all_blueprint_io_waiters(), 0);
    assert_eq!((wait::observe(&host), wait::observe(&guest1), wait::observe(&guest2_empty)), (2,2,2));
    assert_eq!(wait::registry_len(), 5, "wake scan created unrelated queues");
    wait::reset(); assert_eq!(wait::platform_wake_all_blueprint_io_waiters(), 0);
    assert_eq!(wait::registry_len(), 0);
}

mod adapter {
    use super::*;
    use api::Event as NetEvent;
    pub struct EventQueue { pub events: Mutex<VecDeque<NetEvent>>, capacity: usize }
    impl EventQueue {
        pub fn new(capacity: usize) -> Self { Self { events: Mutex::new(VecDeque::new()), capacity } }
        pub fn push(&self, event: NetEvent) -> Result<(), NetEvent> {
            let mut events = self.events.lock();
            if events.len() >= self.capacity { return Err(event); }
            events.push_back(event); Ok(())
        }
    }
    struct AppQueues { name: &'static str, events: Arc<EventQueue> }
    static APP_QUEUES: Mutex<Vec<AppQueues>> = Mutex::new(Vec::new());
    pub fn register(name: &'static str, queue: Arc<EventQueue>) {
        APP_QUEUES.lock().push(AppQueues { name, events: queue });
    }
    pub fn reset() { APP_QUEUES.lock().clear(); }
    @PUSH_EVENT@
    pub fn publish(target: &'static str, event: NetEvent) -> bool { push_event(target, event) }
}

struct PublishedBeforeWake {
    queue: Arc<adapter::EventQueue>, expected: api::Event, wakes: AtomicUsize,
}
impl Wake for PublishedBeforeWake {
    fn wake(self: Arc<Self>) {
        assert_eq!(self.queue.events.lock().back(), Some(&self.expected), "wake ran before event publication");
        assert!(wait::registry_unlocked(), "wake ran while wait registry lock was retained");
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn adapter_publishes_open_error_transmit_and_existing_io_events_before_waking_parked_readers() {
    use api::{ByteBuf, EndpointV4, EndpointV6, Event, NetHandle, SocketKind};
    let h = NetHandle(9);
    let events = [
        Event::Opened { handle: h, kind: SocketKind::Tcp },
        Event::Error { msg: "connect failed" },
        Event::TcpSent { handle: h, len: 7 },
        Event::TcpData { handle: h, data: ByteBuf::from_slice_trunc(b"payload") },
        Event::TcpReadClosed { handle: h },
        Event::TcpEstablished { handle: h, peer: None, peer6: None },
        Event::Closed { handle: h },
        Event::UdpPacket { handle: h, from: EndpointV4::new([127,0,0,1], 7), data: ByteBuf::new() },
        Event::UdpPacketV6 { handle: h, from: EndpointV6::new([0;16], 7), data: ByteBuf::new() },
        Event::IpPacket { handle: h, packet: ByteBuf::new() },
    ];
    for event in events {
        wait::reset(); adapter::reset();
        let queue = Arc::new(adapter::EventQueue::new(1)); adapter::register("native-mio", queue.clone());
        let host = wait::queue(None, wait::BLUEPRINT_IO_WAIT_KEY);
        let guest = wait::queue(Some(1), wait::BLUEPRINT_IO_WAIT_KEY);
        let wake = Arc::new(PublishedBeforeWake { queue: queue.clone(), expected: event, wakes: AtomicUsize::new(0) });
        wait::add_waiter(&host, Waker::from(wake.clone()));
        wait::add_waiter(&guest, Waker::from(wake.clone()));
        assert!(adapter::publish("native-mio", event));
        assert_eq!(wake.wakes.load(Ordering::SeqCst), 2);
        assert_eq!((wait::observe(&host), wait::observe(&guest)), (1,1));
        assert_eq!(queue.events.lock().len(), 1);
    }
}

#[test]
fn adapter_full_or_missing_target_never_wakes_and_non_io_event_remains_non_waking() {
    use api::{ByteBuf, Event, NetHandle};
    wait::reset(); adapter::reset();
    let host = wait::queue(None, wait::BLUEPRINT_IO_WAIT_KEY);
    let count = Arc::new(WakeCount::default()); wait::add_waiter(&host, Waker::from(count.clone()));
    let queue = Arc::new(adapter::EventQueue::new(1)); adapter::register("native-mio", queue.clone());
    let ping = Event::IcmpReply { from: [127,0,0,1], seq: 1, rtt_ms: 1, data: ByteBuf::new() };
    assert!(adapter::publish("native-mio", ping));
    assert_eq!(wait::observe(&host), 0);
    assert!(!adapter::publish("native-mio", Event::TcpSent { handle: NetHandle(9), len: 1 }));
    assert!(!adapter::publish("missing", Event::Error { msg: "missing target" }));
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
    assert_eq!(wait::observe(&host), 0);
    assert_eq!(queue.events.lock().front(), Some(&ping));
    assert_eq!(queue.events.lock().len(), 1);
}
'''


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path,
                        default=ROOT / "bld/no-unix-buildins-20261107/native-mio-readiness")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    sources = {name: (ROOT / path).read_text() for name, path in {
        "mio": "src/mio_compat.rs", "wait": "src/wait.rs", "adapter": "src/net/adapter.rs",
    }.items()}
    extracted = {}

    def keep(name: str, body: str) -> str:
        extracted[name] = body
        return body

    substitutions = {
        "@VNET@": json.dumps(str(ROOT / "crates/trueos-v/src/vnet.rs")),
        "@PROGRESS@": json.dumps(str(ROOT / "src/mio_compat/progress.rs")),
        "@MIO_CONSTANTS@": "\n".join(keep(name, constant(sources["mio"], name)) for name in (
            "STATUS_OK", "STATUS_INVALID_INPUT", "STATUS_NOT_FOUND", "STATUS_IO",
            "MIO_TCP_TX_WINDOW_BYTES", "READY_READABLE", "READY_WRITABLE", "READY_ERROR",
            "READY_READ_CLOSED", "READY_WRITE_CLOSED")),
        "@SOCKET_KIND@": keep("MioSocketKind", item(sources["mio"], "enum MioSocketKind")),
        "@COMPAT_ADDR@": keep("CompatAddr", item(sources["mio"], "enum CompatAddr")),
        "@SOCKET_STATE@": keep("MioSocketState", item(sources["mio"], "struct MioSocketState")),
        "@SOCKET_LOOKUP@": keep("socket", function(sources["mio"], "socket")),
        "@READY_MASK@": keep("ready_mask", function(sources["mio"], "ready_mask")),
        "@MIO_FUNCTIONS@": "\n".join(keep(name, function(sources["mio"], name)) for name in (
            "compat_progress", "vm_guest_vmcall_active", "current_owner_vm",
            "mio_socket_poll_ready", "mio_socket_poll_ready_host", "mio_socket_poll_ready_for_vm_host",
            "mio_socket_poll_ready_owned_host", "trueos_mio_socket_poll_ready")),
        "@WAIT_CONSTANTS@": "\n".join(keep(name, constant(sources["wait"], name)) for name in (
            "PLATFORM_WAIT_HOST_SCOPE", "BLUEPRINT_IO_WAIT_KEY")),
        "@WAIT_QUEUE@": keep("WaitQueue", item(sources["wait"], "pub struct WaitQueue")),
        "@QUEUED_WAKER@": keep("QueuedWaker", item(sources["wait"], "struct QueuedWaker")),
        "@WAIT_NEW@": keep("WaitQueue::new", function(sources["wait"].split("impl WaitQueue {", 1)[1], "new")),
        "@WAIT_NOTIFY_ALL@": keep("WaitQueue::notify_all", function(sources["wait"], "notify_all")),
        "@WAIT_FUNCTIONS@": "\n".join(keep(name, function(sources["wait"], name)) for name in (
            "platform_wait_queue", "platform_wait_vm_scope", "platform_wake_all_blueprint_io_waiters")),
        "@PUSH_EVENT@": keep("push_event", function(sources["adapter"], "push_event")),
    }
    harness = HARNESS
    for placeholder, value in substitutions.items():
        harness = harness.replace(placeholder, value)
    if re.search(r"@[A-Z_]+@", harness):
        raise ValueError("unexpanded fixture placeholder")
    source = output / "harness.rs"
    source.write_text(harness)
    rustc = ["rustup", "run", TOOLCHAIN, "rustc"]
    runs = []
    log_path = output / "tests.log"
    with log_path.open("w") as log:
        for optimization in (0, 3):
            executable = output / f"tests-o{optimization}"
            compile_command = [*rustc, "--edition=2024", "--test", str(source),
                               "-C", f"opt-level={optimization}", "-o", str(executable)]
            log.write("COMMAND " + json.dumps(compile_command) + "\n")
            log.flush()
            subprocess.run(compile_command, check=True, stdout=log, stderr=subprocess.STDOUT, timeout=60)
            run_command = [str(executable), "--test-threads=1"]
            log.write("COMMAND " + json.dumps(run_command) + "\n")
            log.flush()
            env = {**os.environ, "RUST_MIN_STACK": str(16 * 1024 * 1024)}
            result = subprocess.run(run_command, check=True, text=True, stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT, timeout=30, env=env)
            log.write(result.stdout)
            passed = re.search(r"test result: ok\. (\d+) passed", result.stdout)
            if passed is None:
                raise ValueError("test completion count missing")
            runs.append({"optimization": optimization, "passed": int(passed[1]),
                         "compile_command": compile_command, "run_command": run_command})
    receipt = {
        "utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": __doc__, "toolchain": TOOLCHAIN, "runs": runs,
        "extracted_items": {name: hashlib.sha256(body.encode()).hexdigest() for name, body in extracted.items()},
        "source_files": {str(ROOT / path): hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
                         for path in ("src/mio_compat.rs", "src/mio_compat/progress.rs", "src/wait.rs",
                                      "src/net/adapter.rs", "crates/trueos-v/src/vnet.rs")},
        "fixture": str(source), "fixture_sha256": hashlib.sha256(harness.encode()).hexdigest(),
        "log": str(log_path), "limitations": [
            "Host boundary fixture; no physical NIC, VMX instruction, scheduler or timing measurement.",
            "Guest-call fixture forwards to the production owned-host adapter; actual VMCall dispatch is outside this test.",
            "Socket registry and adapter queue storage are fixtures; authorization, mask, progress, wake filtering and notification bodies are production.",
        ],
    }
    receipt_path = output / "receipt.json"
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"runs": runs, "receipt": str(receipt_path), "log": str(log_path)}, indent=2))


if __name__ == "__main__":
    main()
