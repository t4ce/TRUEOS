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
    start = adapter.index("            if socket.is_active() && socket.may_recv() {")
    end = adapter.index("\n            }", start) + len("\n            }")
    receive = adapter[start:end]
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
static SENT_CREDIT: AtomicU64 = AtomicU64::new(0);
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
}
std::thread_local! { static DELIVERED: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn push_event(_: &'static str, event: NetEvent) -> bool {
    if !DELIVERY_ALLOWED.load(AtomicOrdering::Acquire) { return false; }
    match event {
        NetEvent::TcpData { data, .. } => DELIVERED.with(|out| out.borrow_mut().extend(data)),
        NetEvent::TcpSent { len, .. } => { SENT_CREDIT.fetch_add(len as u64, AtomicOrdering::Relaxed); }
    }
    true
}
struct Socket { bytes: Vec<u8> }
impl Socket {
    fn is_active(&self) -> bool { true }
    fn may_recv(&self) -> bool { true }
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
}
struct Service { device_index: usize, records: Vec<Record> }
impl Service {
    @LOOPBACK@
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
    SENT_CREDIT.store(0, AtomicOrdering::Release);
    DELIVERED.with(|out| out.borrow_mut().clear());
    serial
}
#[test]
fn pause_preserves_tcp_bytes_and_resume_delivers_the_exact_stream() {
    let _serial = reset();
    assert!(publish_rx_owner(1, Some(3)));
    let service = Service { device_index: 2, records: Vec::new() };
    let mut socket = Socket { bytes: b"TLS ordered byte stream".to_vec() };
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
        Record { owner: "sender", handle: NetHandle(1), tcp_loopback_peer: Some(NetHandle(9)), tcp_tx: tx },
        Record { owner: "paused", handle: NetHandle(9), tcp_loopback_peer: Some(NetHandle(1)), tcp_tx: TcpTxQueue::new() },
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
'''
    harness = harness.replace("@RECEIVE@", receive).replace("@LOOPBACK@", loopback)
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
