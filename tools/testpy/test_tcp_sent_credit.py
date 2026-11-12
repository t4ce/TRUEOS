#!/usr/bin/env python3
"""Test production TCP transmit-credit retries with bounded event queues."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_net_queue_wake import production_wait_queue
from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
extern crate self as v;
use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::{cell::{Cell, RefCell}, collections::BTreeMap};
@WAIT@
@QUEUE@
#[path="@TCP_QUEUE@"] mod tcp_tx_queue;
use tcp_tx_queue::TcpTxQueue;
#[derive(Clone, Copy, Debug, PartialEq, Eq)] struct NetHandle(u32);
#[derive(Clone, Copy, PartialEq, Eq)] enum SocketKind { Tcp }
#[derive(Debug)] enum NetEvent {
    Error { msg: &'static str },
    TcpSent { handle: NetHandle, len: usize },
    TcpData { handle: NetHandle, data: Vec<u8> },
    TcpReadClosed { handle: NetHandle },
}
thread_local! {
    static EVENTS: RefCell<BTreeMap<&'static str, &'static NetQueue<NetEvent>>> = RefCell::new(BTreeMap::new());
    static PAUSED: Cell<bool> = const { Cell::new(false) };
}
fn events(owner: &'static str) -> &'static NetQueue<NetEvent> {
    EVENTS.with(|events| *events.borrow_mut().entry(owner)
        .or_insert_with(|| NetQueue::new_leaked(owner, 1)))
}
fn push_event(owner: &'static str, event: NetEvent) -> bool { events(owner).push(event).is_ok() }
fn reset() { EVENTS.with(|events| events.borrow_mut().clear()); PAUSED.with(|paused| paused.set(false)); }
mod vnet { pub const MAX_MSG: usize = 4; }
mod access_key_db { pub fn authorize_tcp_connect(_:u8, _:u8, _:u16)->Result<(),()>{Ok(())} }
mod mio_compat { pub fn tcp_receive_paused(_:usize, _:u32)->bool{crate::PAUSED.with(|p|p.get())} }
mod log_os { pub mod flags { pub const NET_LOG_TCP_SEND_FLUSH:bool=false; } }
#[macro_export] macro_rules! log_info { ($($tt:tt)*) => {{}}; }
static LOGTOTCP_SEND_FLUSH_SUPPRESSED:AtomicU64=AtomicU64::new(0);
static LOGTOTCP_SEND_FLUSH_BYTES:AtomicU64=AtomicU64::new(0);
static LOGTOTCP_SEND_FLUSH_LAST_LOG_NS:AtomicU64=AtomicU64::new(0);
fn net_log_once_per_second(_: &AtomicU64)->bool{false}
mod tcp {
    #[derive(Default)] pub struct Socket { pub allowance:usize, pub accepted:Vec<u8>, pub closed:bool }
    impl Socket {
        pub fn can_send(&self)->bool{self.allowance!=0}
        pub fn may_send(&self)->bool{!self.closed}
        pub fn close(&mut self){self.closed=true;}
        pub fn send_slice(&mut self, bytes:&[u8])->Result<usize,()>{
            let len=bytes.len().min(self.allowance);self.accepted.extend_from_slice(&bytes[..len]);self.allowance-=len;Ok(len)
        }
    }
}
#[derive(Default)] struct Sockets { socket:tcp::Socket }
impl Sockets { fn get_mut<T>(&mut self,_:usize)->&mut tcp::Socket{&mut self.socket} }
struct SocketRecord {
    owner:&'static str, handle:NetHandle, kind:SocketKind, socket:usize,
    tcp_tx:TcpTxQueue, tcp_sent_pending:usize, tcp_finish_pending:bool,
    tcp_write_closed:bool, tcp_loopback_peer:Option<NetHandle>, tcp_access:Option<(u8,u8,u16)>,
}
fn record(owner:&'static str, handle:u32)->SocketRecord{
    SocketRecord{owner,handle:NetHandle(handle),kind:SocketKind::Tcp,socket:0,tcp_tx:TcpTxQueue::new(),
        tcp_sent_pending:0,tcp_finish_pending:false,tcp_write_closed:false,tcp_loopback_peer:None,tcp_access:None}
}
struct NetService { device_index:usize, records:Vec<SocketRecord>, sockets:Sockets }
impl NetService {
    @METHODS@
    fn close_loopback_tcp(&mut self,_:NetHandle)->bool{panic!("unexpected revocation")}
}
fn physical()->NetService{reset();NetService{device_index:0,records:vec![record("sender",7)],sockets:Sockets::default()}}
fn credit()->usize {
    match events("sender").pop().expect("missing retained credit") {
        NetEvent::TcpSent{handle:NetHandle(7),len}=>len, other=>panic!("unexpected {other:?}"),
    }
}
#[test]
fn full_event_queue_retains_coalesced_credit_and_eventually_clears_in_flight_exactly_once() {
    let mut service=physical();
    assert!(push_event("sender",NetEvent::Error{msg:"occupied"}));
    service.records[0].tcp_tx.push(b"abc".to_vec());
    service.records[0].tcp_tx.push(b"defg".to_vec());
    service.sockets.socket.allowance=100;
    service.flush_tcp_tx(0);
    assert_eq!(service.sockets.socket.accepted,b"abcdefg");
    assert!(service.records[0].tcp_tx.is_empty());
    assert_eq!(service.records[0].tcp_sent_pending,7);
    service.records[0].tcp_tx.push(b"hijkl".to_vec());
    for _ in 0..4 {service.flush_tcp_tx(0);}
    assert_eq!(service.sockets.socket.accepted,b"abcdefg","accepted more data before retaining credit");
    assert_eq!(service.records[0].tcp_sent_pending,7);
    assert!(matches!(events("sender").pop(),Some(NetEvent::Error{..})));
    service.flush_tcp_tx(0);
    assert_eq!(service.sockets.socket.accepted,b"abcdefghijkl");
    let mut in_flight=12;
    in_flight-=credit();assert_eq!(in_flight,5);
    service.flush_tcp_tx(0);
    in_flight-=credit();assert_eq!(in_flight,0);
    for _ in 0..4{service.flush_tcp_tx(0);}
    assert!(events("sender").is_empty());
    assert_eq!(service.records[0].tcp_sent_pending,0);
}
#[test]
fn partial_socket_acceptance_credit_retries_even_when_socket_cannot_accept_more() {
    let mut service=physical();
    assert!(push_event("sender",NetEvent::Error{msg:"occupied"}));
    service.records[0].tcp_tx.push(b"abcde".to_vec());
    service.sockets.socket.allowance=3;
    service.flush_tcp_tx(0);
    assert_eq!(service.records[0].tcp_sent_pending,3);
    assert_eq!(service.records[0].tcp_tx.len(),2);
    events("sender").pop();
    service.flush_tcp_tx(0);
    assert_eq!(credit(),3);
    assert_eq!(service.sockets.socket.accepted,b"abc");
    service.sockets.socket.allowance=2;
    service.flush_tcp_tx(0);
    assert_eq!(credit(),2);
    assert_eq!(service.sockets.socket.accepted,b"abcde");
    service.flush_tcp_tx(0);
    assert!(events("sender").is_empty());
}
#[test]
fn loopback_retains_credit_without_replaying_peer_bytes_and_retries_before_paused_peer() {
    reset();
    let mut sender=record("sender",7);sender.tcp_loopback_peer=Some(NetHandle(8));
    sender.tcp_tx.push(b"abcdef".to_vec());
    let mut service=NetService{device_index:0,records:vec![sender,record("peer",8)],sockets:Sockets::default()};
    assert!(push_event("sender",NetEvent::Error{msg:"occupied"}));
    assert!(!service.flush_loopback_tcp_tx(0));
    assert_eq!(service.records[0].tcp_sent_pending,4);
    for _ in 0..3{assert!(!service.flush_loopback_tcp_tx(0));}
    match events("peer").pop().unwrap(){NetEvent::TcpData{handle:NetHandle(8),data}=>assert_eq!(data,b"abcd"),other=>panic!("{other:?}")}
    events("sender").pop();
    PAUSED.with(|paused|paused.set(true));
    assert!(!service.flush_loopback_tcp_tx(0));
    assert_eq!(credit(),4,"existing credit was held behind peer pause");
    assert!(events("peer").is_empty());
    PAUSED.with(|paused|paused.set(false));
    assert!(!service.flush_loopback_tcp_tx(0));
    assert_eq!(credit(),2);
    match events("peer").pop().unwrap(){NetEvent::TcpData{data,..}=>assert_eq!(data,b"ef"),other=>panic!("{other:?}")}
    assert!(!service.flush_loopback_tcp_tx(0));
    assert!(events("sender").is_empty());assert!(events("peer").is_empty());
}
'''


def main():
    source = (ROOT / "src/net/adapter.rs").read_text()
    harness = HARNESS.replace("@WAIT@", production_wait_queue())
    harness = harness.replace("@QUEUE@", "\n".join(item(source, signature) for signature in (
        "pub struct NetQueue<T>", "impl<T> NetQueue<T>",
    )))
    harness = harness.replace("@METHODS@", "\n".join(item(source, signature) for signature in (
        "fn flush_tcp_sent(", "fn flush_tcp_tx(", "fn flush_loopback_tcp_tx(",
    )))
    harness = harness.replace("@TCP_QUEUE@", str(ROOT / "src/net/tcp_tx_queue.rs"))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-tcp-sent-credit-") as directory:
        directory = Path(directory)
        (directory / "lib.rs").write_text(harness)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="tcp-sent-credit-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\n')
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/tcp-sent-credit"),
            "--", "--test-threads=1",
        ], cwd=directory, check=True, timeout=120)


if __name__ == "__main__":
    main()
