#!/usr/bin/env python3
"""Exercise real Mio owner mapping and the adapter's paused TCP receive loop."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    production = "\n".join(item("src/mio_compat.rs", name) for name in (
        "rx_owner", "publish_rx_owner", "tcp_receive_paused",
    ))
    adapter = (ROOT / "src/net/adapter.rs").read_text()
    start = adapter.index("            if socket.may_recv() {")
    end = adapter.index("\n            }", start) + len("\n            }")
    receive = adapter[start:end]
    start = adapter.index("            if !socket.is_open()")
    end = adapter.index("\n            }", start) + len("\n            }")
    retirement = adapter[start:end]
    start = adapter.index("        if should_remove {")
    end = adapter.index("\n        }", start) + len("\n        }")
    close = adapter[start:end]
    start = adapter.index("    fn flush_loopback_tcp_tx(")
    end = adapter.index("\n    }", start) + len("\n    }")
    loopback = adapter[start:end]
    harness = r'''
#![allow(dead_code, unused_assignments)]
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
#[path = "@TCP_QUEUE@"] mod tcp_tx_queue;
use tcp_tx_queue::TcpTxQueue;
const MIO_RX_OWNER_SLOTS: usize = 8;
const MIO_RX_OWNER_TOMBSTONE: u64 = u64::MAX;
static MIO_RX_OWNERS: [AtomicU64; MIO_RX_OWNER_SLOTS] =
    [const { AtomicU64::new(0) }; MIO_RX_OWNER_SLOTS];
static MIO_RX_DEVICE_INDEX: AtomicUsize = AtomicUsize::new(2);
static PAUSED: AtomicU64 = AtomicU64::new(0);
static DELIVERY_ALLOWED: AtomicBool = AtomicBool::new(true);
static DELIVERY_BUDGET: AtomicUsize = AtomicUsize::new(usize::MAX);
static SENT_CREDIT: AtomicU64 = AtomicU64::new(0);
static EOF_COUNT: AtomicU64 = AtomicU64::new(0);
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
mod net { pub fn primary_device_index() -> usize { 2 } }
mod hv {
    pub fn guest_pause_requested(vm: u8) -> bool {
        super::PAUSED.load(super::AtomicOrdering::Acquire) & (1u64 << vm) != 0
    }
}
mod mio_compat { pub(crate) use super::tcp_receive_paused; }
mod v { pub mod vnet { pub const MAX_MSG: usize = 4; } }
#[derive(Clone, Copy, Eq, PartialEq)] struct NetHandle(u32);
enum NetEvent {
    TcpData { handle: NetHandle, data: Vec<u8> },
    TcpSent { handle: NetHandle, len: usize },
    TcpReadClosed { handle: NetHandle },
    Closed { handle: NetHandle },
}
std::thread_local! { static EVENTS: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) }; }
std::thread_local! { static DELIVERED: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn push_event(_: &'static str, event: NetEvent) -> bool {
    if !DELIVERY_ALLOWED.load(AtomicOrdering::Acquire) { return false; }
    if DELIVERY_BUDGET.load(AtomicOrdering::Acquire) == 0 { return false; }
    DELIVERY_BUDGET.fetch_sub(1, AtomicOrdering::AcqRel);
    match event {
        NetEvent::TcpData { data, .. } => {
            EVENTS.with(|out| out.borrow_mut().push("data"));
            DELIVERED.with(|out| out.borrow_mut().extend(data));
        }
        NetEvent::TcpSent { len, .. } => { SENT_CREDIT.fetch_add(len as u64, AtomicOrdering::Relaxed); }
        NetEvent::TcpReadClosed { .. } => { EOF_COUNT.fetch_add(1, AtomicOrdering::Relaxed); }
        NetEvent::Closed { .. } => EVENTS.with(|out|out.borrow_mut().push("closed")),
    }
    true
}
struct Socket { bytes: Vec<u8>, active: bool, open: bool }
impl Socket {
    fn is_active(&self) -> bool { self.active }
    fn is_open(&self) -> bool { self.open }
    fn may_recv(&self) -> bool { self.active || self.can_recv() }
    fn can_recv(&self) -> bool { !self.bytes.is_empty() }
    fn recv<T>(&mut self, f: impl FnOnce(&[u8]) -> (usize, T)) -> Result<T, ()> {
        let (consumed, result) = f(&self.bytes);
        self.bytes.drain(..consumed);
        Ok(result)
    }
}
struct Record {
    owner: &'static str, handle: NetHandle,
    tcp_loopback_peer: Option<NetHandle>, tcp_tx: TcpTxQueue,
    tcp_finish_pending: bool, tcp_write_closed: bool,
}
struct Service { device_index: usize, records: Vec<Record> }
impl Service {
    @LOOPBACK@
    fn remove_record(&mut self, handle: NetHandle) {
        self.records.retain(|record| record.handle != handle);
    }
    fn receive_and_retire(&mut self, socket: &mut Socket, handle: NetHandle) -> bool {
        let owner = "mio-test";
        let mut rx_backpressured = false;
        let mut rx_bytes_this_poll = 0usize;
        let mut rx_events_this_poll = 0usize;
        let mut rx_event_drops_this_poll = 0usize;
        let mut should_remove = false;
        @RECEIVE@
        @RETIREMENT@
        @CLOSE@
        false
    }
    fn receive(&self, socket: &mut Socket, handle: NetHandle) -> bool {
        let owner = "mio-test";
        let mut rx_backpressured = false;
        let mut rx_bytes_this_poll = 0usize;
        let mut rx_events_this_poll = 0usize;
        let mut rx_event_drops_this_poll = 0usize;
        @RECEIVE@
        rx_backpressured
    }
}
fn reset() -> std::sync::MutexGuard<'static, ()> {
    let serial = TEST_SERIAL.lock().unwrap();
    for slot in &MIO_RX_OWNERS { slot.store(0, AtomicOrdering::Release); }
    PAUSED.store(0, AtomicOrdering::Release);
    DELIVERY_ALLOWED.store(true, AtomicOrdering::Release);
    DELIVERY_BUDGET.store(usize::MAX, AtomicOrdering::Release);
    EVENTS.with(|out|out.borrow_mut().clear());
    SENT_CREDIT.store(0, AtomicOrdering::Release);
    EOF_COUNT.store(0, AtomicOrdering::Release);
    DELIVERED.with(|out| out.borrow_mut().clear());
    serial
}
#[test]
fn pause_preserves_tcp_bytes_and_resume_delivers_the_exact_stream() {
    let _serial = reset();
    assert!(publish_rx_owner(1, Some(3)));
    let service = Service { device_index: 2, records: Vec::new() };
    let mut socket = Socket { bytes: b"TLS ordered byte stream".to_vec(), active: true, open: true };
    PAUSED.store(1 << 3, AtomicOrdering::Release);
    assert!(service.receive(&mut socket, NetHandle(1)));
    assert_eq!(socket.bytes, b"TLS ordered byte stream");
    DELIVERED.with(|out| assert!(out.borrow().is_empty()));
    PAUSED.store(0, AtomicOrdering::Release);
    assert!(!service.receive(&mut socket, NetHandle(1)));
    assert!(socket.bytes.is_empty());
    DELIVERED.with(|out| assert_eq!(*out.borrow(), b"TLS ordered byte stream"));
}
#[test]
fn close_and_handle_reuse_preserve_hash_collisions_and_new_owner() {
    let _serial = reset();
    assert!(publish_rx_owner(1, Some(1)));
    assert!(publish_rx_owner(9, Some(2)));
    assert!(publish_rx_owner(17, Some(3)));
    assert!(publish_rx_owner(1, None));
    assert_eq!(rx_owner(9), Some(2));
    assert_eq!(rx_owner(17), Some(3));
    assert!(publish_rx_owner(1, Some(4)));
    assert_eq!(rx_owner(1), Some(4));
    PAUSED.store(1 << 1, AtomicOrdering::Release);
    assert!(!tcp_receive_paused(2, 1), "a retired owner's pause must not affect reuse");
    PAUSED.store(1 << 4, AtomicOrdering::Release);
    assert!(tcp_receive_paused(2, 1));
}
#[test]
fn saturation_refuses_only_new_guest_registration_and_preserves_control_traffic() {
    let _serial = reset();
    for handle in 1..=8 { assert!(publish_rx_owner(handle, Some(1))); }
    assert!(!publish_rx_owner(9, Some(1)));
    PAUSED.store(1 << 1, AtomicOrdering::Release);
    assert!(!tcp_receive_paused(2, 9), "unknown control sockets must keep receiving");
    assert!(!tcp_receive_paused(1, 1), "another NIC can use the same numeric handle");
    assert!(publish_rx_owner(4, None));
    assert!(publish_rx_owner(9, Some(2)));
    assert_eq!(rx_owner(9), Some(2));
}
#[test]
fn loopback_retains_bytes_and_sender_credit_across_pause_and_queue_pressure() {
    let _serial = reset();
    assert!(publish_rx_owner(9, Some(3)));
    let mut tx = TcpTxQueue::new();
    tx.push(b"TLS bytes one".to_vec());
    tx.push(b" and two".to_vec());
    let expected = tx.len();
    let mut service = Service { device_index: 2, records: vec![
        Record { owner: "sender", handle: NetHandle(1), tcp_loopback_peer: Some(NetHandle(9)), tcp_tx: tx, tcp_finish_pending: false, tcp_write_closed: false },
        Record { owner: "paused", handle: NetHandle(9), tcp_loopback_peer: Some(NetHandle(1)), tcp_tx: TcpTxQueue::new(), tcp_finish_pending: false, tcp_write_closed: false },
    ] };
    PAUSED.store(1 << 3, AtomicOrdering::Release);
    service.flush_loopback_tcp_tx(0);
    assert_eq!(service.records[0].tcp_tx.len(), expected);
    assert_eq!(SENT_CREDIT.load(AtomicOrdering::Acquire), 0);
    PAUSED.store(0, AtomicOrdering::Release);
    DELIVERY_ALLOWED.store(false, AtomicOrdering::Release);
    service.flush_loopback_tcp_tx(0);
    assert_eq!(service.records[0].tcp_tx.len(), expected, "queue pressure must consume zero bytes");
    DELIVERY_ALLOWED.store(true, AtomicOrdering::Release);
    service.flush_loopback_tcp_tx(0);
    assert!(service.records[0].tcp_tx.is_empty());
    DELIVERED.with(|out| assert_eq!(*out.borrow(), b"TLS bytes one and two"));
    assert_eq!(SENT_CREDIT.load(AtomicOrdering::Acquire) as usize, expected);
}
#[test]
fn finish_waits_for_all_queued_bytes_and_preserves_peer_reply() {
    let _serial = reset();
    assert!(publish_rx_owner(9, Some(3)));
    let mut tx = TcpTxQueue::new();
    tx.push(vec![b'a'; 300]); // More than the bounded 64 four-byte chunks per flush.
    let mut service = Service { device_index: 2, records: vec![
        Record { owner: "sender", handle: NetHandle(1), tcp_loopback_peer: Some(NetHandle(9)),
            tcp_tx: tx, tcp_finish_pending: true, tcp_write_closed: false },
        Record { owner: "peer", handle: NetHandle(9), tcp_loopback_peer: Some(NetHandle(1)),
            tcp_tx: TcpTxQueue::new(), tcp_finish_pending: false, tcp_write_closed: false },
    ] };
    PAUSED.store(1 << 3, AtomicOrdering::Release);
    service.flush_loopback_tcp_tx(0);
    assert_eq!(EOF_COUNT.load(AtomicOrdering::Acquire), 0);
    PAUSED.store(0, AtomicOrdering::Release);
    service.flush_loopback_tcp_tx(0);
    assert!(!service.records[0].tcp_tx.is_empty());
    assert_eq!(EOF_COUNT.load(AtomicOrdering::Acquire), 0);
    service.flush_loopback_tcp_tx(0);
    assert!(service.records[0].tcp_write_closed);
    assert!(!service.records[0].tcp_finish_pending);
    assert_eq!(EOF_COUNT.load(AtomicOrdering::Acquire), 1);
    DELIVERED.with(|out| { assert_eq!(out.borrow().len(), 300); out.borrow_mut().clear(); });
    service.flush_loopback_tcp_tx(0);
    assert_eq!(EOF_COUNT.load(AtomicOrdering::Acquire), 1, "EOF must be idempotent");
    service.records[1].tcp_tx.push(b"reply".to_vec());
    service.flush_loopback_tcp_tx(1);
    DELIVERED.with(|out| assert_eq!(*out.borrow(), b"reply"));
}
#[test]
fn finish_retries_eof_when_event_queue_is_full_even_without_data() {
    let _serial = reset();
    let mut service = Service { device_index: 2, records: vec![
        Record { owner: "sender", handle: NetHandle(1), tcp_loopback_peer: Some(NetHandle(9)),
            tcp_tx: TcpTxQueue::new(), tcp_finish_pending: true, tcp_write_closed: false },
        Record { owner: "peer", handle: NetHandle(9), tcp_loopback_peer: Some(NetHandle(1)),
            tcp_tx: TcpTxQueue::new(), tcp_finish_pending: false, tcp_write_closed: false },
    ] };
    DELIVERY_ALLOWED.store(false, AtomicOrdering::Release);
    service.flush_loopback_tcp_tx(0);
    assert!(service.records[0].tcp_finish_pending);
    assert_eq!(EOF_COUNT.load(AtomicOrdering::Acquire), 0);
    DELIVERY_ALLOWED.store(true, AtomicOrdering::Release);
    service.flush_loopback_tcp_tx(0);
    assert!(service.records[0].tcp_write_closed);
    assert_eq!(EOF_COUNT.load(AtomicOrdering::Acquire), 1);
}
fn physical_service() -> Service {
    Service { device_index: 2, records: vec![
        Record { owner: "mio-test", handle: NetHandle(1), tcp_loopback_peer: None,
            tcp_tx: TcpTxQueue::new(), tcp_finish_pending: false, tcp_write_closed: true },
    ] }
}
#[test]
fn inactive_timewait_drains_final_reply_before_closed_and_retirement() {
    let _serial = reset();
    let mut service = physical_service();
    let mut socket = Socket { bytes: b"reply:307200!".to_vec(), active: false, open: false };
    assert!(!socket.is_active() && socket.may_recv() && socket.can_recv());
    assert!(service.receive_and_retire(&mut socket, NetHandle(1)));
    DELIVERED.with(|out| assert_eq!(*out.borrow(), b"reply:307200!"));
    EVENTS.with(|out| assert_eq!(*out.borrow(), ["data", "data", "data", "data", "closed"]));
    assert!(socket.bytes.is_empty());
    assert!(service.records.is_empty());
}
#[test]
fn inactive_timewait_retains_final_receive_when_paused_or_queue_full() {
    let _serial = reset();
    assert!(publish_rx_owner(1, Some(3)));
    let mut service = physical_service();
    let mut socket = Socket { bytes: b"reply:307200!".to_vec(), active: false, open: false };
    PAUSED.store(1 << 3, AtomicOrdering::Release);
    assert!(!service.receive_and_retire(&mut socket, NetHandle(1)));
    assert_eq!(service.records.len(), 1);
    assert_eq!(socket.bytes, b"reply:307200!");
    PAUSED.store(0, AtomicOrdering::Release);
    DELIVERY_ALLOWED.store(false, AtomicOrdering::Release);
    assert!(!service.receive_and_retire(&mut socket, NetHandle(1)));
    assert_eq!(service.records.len(), 1);
    assert_eq!(socket.bytes, b"reply:307200!");
    EVENTS.with(|out| assert!(out.borrow().is_empty()));
    DELIVERY_ALLOWED.store(true, AtomicOrdering::Release);
    assert!(service.receive_and_retire(&mut socket, NetHandle(1)));
    DELIVERED.with(|out| assert_eq!(*out.borrow(), b"reply:307200!"));
}
#[test]
fn final_data_can_fill_event_queue_and_closed_is_retried_without_replaying_bytes() {
    let _serial = reset();
    let mut service = physical_service();
    let mut socket = Socket { bytes: b"reply:307200!".to_vec(), active: false, open: false };
    DELIVERY_BUDGET.store(4, AtomicOrdering::Release);
    assert!(!service.receive_and_retire(&mut socket, NetHandle(1)));
    assert!(socket.bytes.is_empty());
    assert_eq!(service.records.len(), 1, "retain until Closed can be queued");
    EVENTS.with(|out| assert_eq!(*out.borrow(), ["data", "data", "data", "data"]));
    assert!(!service.receive_and_retire(&mut socket, NetHandle(1)));
    assert_eq!(service.records.len(), 1);
    DELIVERY_BUDGET.store(1, AtomicOrdering::Release);
    assert!(service.receive_and_retire(&mut socket, NetHandle(1)));
    assert!(service.records.is_empty());
    DELIVERED.with(|out| assert_eq!(*out.borrow(), b"reply:307200!"));
    EVENTS.with(|out| assert_eq!(*out.borrow(), ["data", "data", "data", "data", "closed"]));
}
'''
    harness = harness.replace("@RECEIVE@", receive).replace("@LOOPBACK@", loopback)
    harness = harness.replace("@RETIREMENT@", retirement).replace("@CLOSE@", close)
    harness = harness.replace("@TCP_QUEUE@", str(ROOT / "src/net/tcp_tx_queue.rs"))
    with tempfile.TemporaryDirectory(prefix="trueos-mio-pause-") as directory:
        path = Path(directory)
        source = path / "pause.rs"
        source.write_text(harness + production)
        binary = path / "pause"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "--target", "x86_64-unknown-linux-gnu",
             str(source), "-o", str(binary)], check=True, cwd=ROOT,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
