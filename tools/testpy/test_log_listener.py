#!/usr/bin/env python3
"""Poll the production TCP log task with host queues and a controlled clock.

No sockets are opened. Only its network/clock boundaries, retained log bytes,
snapshot providers and log sink are replaced; the async task is unchanged.
"""

import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def task_item():
    source = (ROOT / "src/log_os.rs").read_text()
    start = source.index("pub async fn logtotcp_task()")
    opening = source.index("{", start)
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use std::{collections::VecDeque, future::Future, pin::Pin, sync::{Mutex, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}}, task::{Context, Poll, Waker}};

static CLOCK: AtomicU64 = AtomicU64::new(0);
static READY: AtomicBool = AtomicBool::new(true);
static REJECT_OPEN: AtomicBool = AtomicBool::new(false);
static REJECT_SEND: AtomicBool = AtomicBool::new(false);
static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LOG_BYTES: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());
static DRAIN_CALLS: AtomicUsize = AtomicUsize::new(0);
static SNAPSHOTS: AtomicUsize = AtomicUsize::new(0);
fn record(args: core::fmt::Arguments<'_>) { RECORDS.lock().unwrap().push(format!("{args}")); }
#[macro_export] macro_rules! log { ($($args:tt)*) => { crate::record(format_args!($($args)*)) }; }
#[macro_export] macro_rules! log_warn { (target: $target:expr; $($args:tt)*) => { crate::record(format_args!($($args)*)) }; }
#[macro_export] macro_rules! log_important { (target: $target:expr; $($args:tt)*) => { crate::record(format_args!($($args)*)) }; }

#[derive(Clone, Copy)] pub struct Duration(u64);
impl Duration { pub fn from_millis(value: u64) -> Self { Self(value) } }
#[derive(Clone, Copy)] pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self { Self(CLOCK.load(Ordering::Relaxed)) }
    pub fn as_millis(self) -> u64 { self.0 }
}
pub struct Timer { deadline: u64 }
impl Timer { pub fn after(duration: Duration) -> Self { Self { deadline: Instant::now().as_millis() + duration.0 } } }
impl Future for Timer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if CLOCK.load(Ordering::Relaxed) >= self.deadline { Poll::Ready(()) } else { Poll::Pending }
    }
}

mod net {
pub mod wifi {
    pub fn diagnostic_snapshot_text() -> String { String::from("read-only Wi-Fi fixture snapshot") }
}
pub mod adapter {
    use super::super::*;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct NetHandle(pub u32);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum SocketKind { Tcp, Udp }
    #[derive(Clone, Debug)] pub enum NetCommand {
        OpenTcpListen { port: u16 },
        SendTcp { handle: NetHandle, data: Vec<u8> },
    }
    #[derive(Clone, Debug)] pub enum NetEvent {
        Opened { handle: NetHandle, kind: SocketKind },
        TcpEstablished { handle: NetHandle },
        Closed { handle: NetHandle },
        Error { msg: &'static str },
        TcpSent { handle: NetHandle, len: usize },
    }
    pub struct NetQueue<T> { capacity: usize, inner: Mutex<VecDeque<T>> }
    impl<T: 'static> NetQueue<T> {
        pub fn new_leaked(_: &'static str, capacity: usize) -> &'static Self {
            Box::leak(Box::new(Self { capacity, inner: Mutex::new(VecDeque::new()) }))
        }
        pub fn push(&self, item: T) -> Result<(), ()> {
            if let Some(command) = (&item as &dyn core::any::Any).downcast_ref::<NetCommand>() {
                match command {
                    NetCommand::OpenTcpListen { .. } if REJECT_OPEN.load(Ordering::Relaxed) => return Err(()),
                    NetCommand::SendTcp { .. } if REJECT_SEND.load(Ordering::Relaxed) => return Err(()),
                    _ => {},
                }
            }
            let mut items = self.inner.lock().unwrap();
            if items.len() >= self.capacity { return Err(()); }
            items.push_back(item);
            Ok(())
        }
        pub fn drain(&self, max: usize) -> Vec<T> {
            let mut items = self.inner.lock().unwrap();
            let count = max.min(items.len());
            items.drain(..count).collect()
        }
    }
    pub fn register_app_queues(owner: &'static str, commands: &'static NetQueue<NetCommand>, events: &'static NetQueue<NetEvent>) {
        assert_eq!(owner, "logtotcp");
        let mut registry = crate::QUEUES.lock().unwrap();
        assert!(registry.is_none(), "task must register once");
        *registry = Some((commands, events));
    }
    pub fn take_log_backlog_dropped_bytes() -> u64 { 0 }
} }
use net::adapter::{NetCommand, NetEvent, NetHandle, NetQueue, SocketKind};
type Queues = (&'static NetQueue<NetCommand>, &'static NetQueue<NetEvent>);
static QUEUES: Mutex<Option<Queues>> = Mutex::new(None);

mod r {
    pub mod readiness {
        pub const NET_ANY_CONFIGURED: u32 = 1;
        pub async fn wait_for(_: u32) {
            core::future::poll_fn(|_| if crate::READY.load(crate::Ordering::Relaxed) { crate::Poll::Ready(()) } else { crate::Poll::Pending }).await
        }
    }
    pub mod net {
        pub mod ports { pub const LOGTOTCP_TCP_PORT: u16 = 1; }
        pub mod vlayer {
            pub fn usb_snapshot_text_host() -> String {
                crate::SNAPSHOTS.fetch_add(1, crate::Ordering::Relaxed);
                String::from("read-only USB fixture snapshot")
            }
        }
    }
}
mod intel { pub fn laptop_diagnostic_snapshot_text() -> String { String::from("read-only GPU fixture snapshot") } }
mod production {
    pub mod flags { pub const LAPTOP_BRINGUP_DIAG_PROFILE_ENABLED: bool = true; }
    pub mod logtotcp {
        use alloc::vec::Vec;
        fn drain_bytes(max: usize) -> Vec<u8> {
            crate::DRAIN_CALLS.fetch_add(1, crate::Ordering::Relaxed);
            let mut ring = crate::LOG_BYTES.lock().unwrap();
            let count = max.min(ring.len());
            ring.drain(..count).collect()
        }
        @TASK@
    }
}

fn reset() {
    CLOCK.store(0, Ordering::Relaxed);
    READY.store(true, Ordering::Relaxed);
    REJECT_OPEN.store(false, Ordering::Relaxed);
    REJECT_SEND.store(false, Ordering::Relaxed);
    DRAIN_CALLS.store(0, Ordering::Relaxed);
    SNAPSHOTS.store(0, Ordering::Relaxed);
    RECORDS.lock().unwrap().clear();
    LOG_BYTES.lock().unwrap().clear();
    *QUEUES.lock().unwrap() = None;
}
fn tick(future: Pin<&mut impl Future<Output = ()>>, now: u64) {
    assert!(now >= CLOCK.load(Ordering::Relaxed), "test clock must advance monotonically");
    CLOCK.store(now, Ordering::Relaxed);
    assert!(future.poll(&mut Context::from_waker(Waker::noop())).is_pending());
}
fn commands() -> Vec<NetCommand> { QUEUES.lock().unwrap().as_ref().unwrap().0.drain(128) }
fn event(event: NetEvent) { QUEUES.lock().unwrap().as_ref().unwrap().1.push(event).unwrap(); }
fn opened(handle: u32) { event(NetEvent::Opened { handle: NetHandle(handle), kind: SocketKind::Tcp }); }
fn connected(handle: u32) { event(NetEvent::TcpEstablished { handle: NetHandle(handle) }); }
fn closed(handle: u32) { event(NetEvent::Closed { handle: NetHandle(handle) }); }
fn error(msg: &'static str) { event(NetEvent::Error { msg }); }
fn only_open(commands: Vec<NetCommand>) {
    assert!(matches!(commands.as_slice(), [NetCommand::OpenTcpListen { port: 1 }]), "expected exactly one listen command: {commands:?}");
}
fn listening_records() -> usize { RECORDS.lock().unwrap().iter().filter(|record| record.contains("listening on tcp")).count() }

#[test]
fn configured_before_link_up_retries_after_real_error_and_only_confirms_opened() {
    reset();
    READY.store(false, Ordering::Relaxed);
    let mut task = Box::pin(production::logtotcp::logtotcp_task());
    tick(task.as_mut(), 0);
    assert!(QUEUES.lock().unwrap().is_none());
    READY.store(true, Ordering::Relaxed);
    tick(task.as_mut(), 10);
    only_open(commands());
    assert_eq!(listening_records(), 0, "queue admission is not a listening socket");
    error("tcp listen link down");
    tick(task.as_mut(), 20);
    assert!(commands().is_empty());
    tick(task.as_mut(), 1019);
    assert!(commands().is_empty());
    tick(task.as_mut(), 1029);
    only_open(commands());
    error("tcp listen link down");
    tick(task.as_mut(), 1039);
    assert_eq!(listening_records(), 0);
    tick(task.as_mut(), 2039);
    only_open(commands());
    opened(1);
    tick(task.as_mut(), 2049);
    assert!(commands().is_empty());
    assert_eq!(listening_records(), 1);
    assert!(RECORDS.lock().unwrap().iter().any(|line| line.contains("confirmed=1")));
    tick(task.as_mut(), 10000);
    assert!(commands().is_empty());
}

#[test]
fn queue_backpressure_retries_and_missing_ack_does_not_create_duplicate_listener() {
    reset();
    REJECT_OPEN.store(true, Ordering::Relaxed);
    let mut task = Box::pin(production::logtotcp::logtotcp_task());
    tick(task.as_mut(), 0);
    assert!(commands().is_empty());
    assert_eq!(listening_records(), 0);
    REJECT_OPEN.store(false, Ordering::Relaxed);
    tick(task.as_mut(), 999);
    assert!(commands().is_empty());
    tick(task.as_mut(), 1009);
    only_open(commands());
    error("link down"); // A late send error cannot reject this pending bind.
    tick(task.as_mut(), 1020);
    error("bad handle");
    tick(task.as_mut(), 1030);
    tick(task.as_mut(), 7010);
    assert!(commands().is_empty(), "an unacknowledged open may already own a socket");
    assert_eq!(listening_records(), 0);
    connected(3); // Recover the accepted socket even if Opened was lost.
    tick(task.as_mut(), 7020);
    assert!(commands().is_empty());
    assert_eq!(SNAPSHOTS.load(Ordering::Relaxed), 1);
    tick(task.as_mut(), 20000);
    assert!(commands().is_empty(), "accepted socket consumes the sole listener");
}

#[test]
fn stale_events_and_active_errors_do_not_rebind_or_replace_the_connection() {
    reset();
    let mut task = Box::pin(production::logtotcp::logtotcp_task());
    tick(task.as_mut(), 0);
    only_open(commands());
    opened(7);
    tick(task.as_mut(), 10);
    connected(7);
    tick(task.as_mut(), 20);
    assert!(commands().is_empty());
    closed(99);
    error("bad tcp owner");
    tick(task.as_mut(), 30);
    assert!(commands().is_empty());
    connected(99);
    tick(task.as_mut(), 40);
    assert_eq!(SNAPSHOTS.load(Ordering::Relaxed), 1, "unrelated connection is not adopted");
    LOG_BYTES.lock().unwrap().extend(b"active connection evidence\n");
    tick(task.as_mut(), 50);
    assert!(matches!(commands().as_slice(), [NetCommand::SendTcp { handle: NetHandle(7), .. }]));
    closed(7);
    tick(task.as_mut(), 60);
    assert!(commands().is_empty());
    tick(task.as_mut(), 1059);
    assert!(commands().is_empty());
    tick(task.as_mut(), 1069);
    only_open(commands());
    closed(7);
    tick(task.as_mut(), 1079);
    assert!(commands().is_empty(), "stale close cannot create a second pending open");
    opened(8);
    tick(task.as_mut(), 1089);
    assert_eq!(listening_records(), 2);
}

#[test]
fn unsent_chunk_survives_backpressure_disconnect_and_reconnect() {
    reset();
    let mut task = Box::pin(production::logtotcp::logtotcp_task());
    tick(task.as_mut(), 0);
    only_open(commands());
    let evidence = b"retained original evidence\n";
    LOG_BYTES.lock().unwrap().extend(evidence);
    REJECT_SEND.store(true, Ordering::Relaxed);
    opened(5);
    connected(5);
    tick(task.as_mut(), 10);
    assert!(commands().is_empty());
    assert_eq!(DRAIN_CALLS.load(Ordering::Relaxed), 1);
    assert!(LOG_BYTES.lock().unwrap().is_empty());
    tick(task.as_mut(), 20);
    assert_eq!(DRAIN_CALLS.load(Ordering::Relaxed), 1, "pending bytes must not be drained again");
    closed(5);
    tick(task.as_mut(), 30);
    tick(task.as_mut(), 1030);
    only_open(commands());
    REJECT_SEND.store(false, Ordering::Relaxed);
    opened(6);
    connected(6);
    tick(task.as_mut(), 1040);
    let sent = commands();
    assert!(matches!(sent.as_slice(), [NetCommand::SendTcp { handle: NetHandle(6), data }] if data == evidence));
    assert_eq!(DRAIN_CALLS.load(Ordering::Relaxed), 1);
    tick(task.as_mut(), 1050);
    assert!(commands().is_empty(), "retained chunk is sent once");
    assert!(RECORDS.lock().unwrap().iter().any(|line| line.contains(&format!("retained_unsent_bytes={}", evidence.len()))));
}
'''


def run():
    with tempfile.TemporaryDirectory(prefix="trueos-log-listener-") as temporary:
        directory = Path(temporary)
        source = directory / "tests.rs"
        executable = directory / "tests"
        source.write_text(HARNESS.replace("@TASK@", task_item()))
        channel = re.search(
            r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text()
        ).group(1)
        environment = dict(os.environ, RUSTUP_TOOLCHAIN=channel)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
            cwd=directory, env=environment, check=True, timeout=30,
        )
        subprocess.run([str(executable), "--test-threads=1"], check=True, timeout=15)


if __name__ == "__main__":
    run()
