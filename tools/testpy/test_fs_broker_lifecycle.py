#!/usr/bin/env python3
"""Exercise BSP broker ownership and abandoned stream cleanup on the host."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    path = "src/r/fs/request_broker.rs"
    source = (ROOT / path).read_text()
    start = source.index("impl OwnerState {")
    end = source.index("\n}\n", start) + len("\n}\n")
    production = item(path, "OwnerState") + source[start:end]
    production += "\n".join(item(path, name) for name in (
        "pending_for_vm", "open_writes_for_vm", "open_vm", "request_release_vm",
        "queue_retired_write_aborts",
    ))
    harness = r'''
#![allow(dead_code)]
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
struct CompletionCell;
impl CompletionCell { fn new() -> Self { Self } }
enum Request { WriteFileAbort { id: u64, handle: u32, completion: Arc<CompletionCell> } }
struct OwnedRequest { request: Request, owner: Option<u8> }
const BROKER_QUEUE_CAP: usize = 256;
static BROKER_OWNERS: Mutex<OwnerState> = Mutex::new(OwnerState::new());
static BROKER_REQUESTS: Mutex<VecDeque<OwnedRequest>> = Mutex::new(VecDeque::new());
static BROKER_REQUEST_SEQ: AtomicU64 = AtomicU64::new(100);
struct WaitQueue;
impl WaitQueue { fn notify_one(&self) {} }
static BROKER_WAIT: WaitQueue = WaitQueue;
mod remote_work_wake { pub fn wake_cpu_for_remote_work(_: usize) {} }
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() }
}
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn reset() -> std::sync::MutexGuard<'static, ()> {
    let serial = TEST_SERIAL.lock().unwrap();
    *BROKER_OWNERS.lock() = OwnerState::new();
    BROKER_REQUESTS.lock().clear();
    serial
}
fn began(id: u64, owner: u8, handle: u32) {
    let mut state = BROKER_OWNERS.lock();
    state.submit(id, Some(owner), None);
    state.remember_write(Some(owner), handle);
    state.requests.remove(&id);
}
fn finish_abort() {
    let request = BROKER_REQUESTS.lock().pop_front().unwrap();
    let Request::WriteFileAbort { id, handle, .. } = request.request;
    let mut owners = BROKER_OWNERS.lock();
    owners.forget_write(request.owner, handle);
    owners.requests.remove(&id);
}
#[test]
fn begin_chunk_finish_tracks_the_root_lease_between_requests() {
    let _serial = reset();
    assert!(open_vm(1));
    began(1, 1, 10);
    assert_eq!(pending_for_vm(1), 0);
    assert_eq!(open_writes_for_vm(1), 1, "a completed begin still owns an incomplete write");
    assert!(!open_vm(1));
    BROKER_OWNERS.lock().submit(2, Some(1), Some(10));
    assert_eq!(pending_for_vm(1), 1);
    BROKER_OWNERS.lock().requests.remove(&2);
    assert_eq!(open_writes_for_vm(1), 1, "chunk completion must retain the stream");
    BROKER_OWNERS.lock().submit(3, Some(1), Some(10));
    BROKER_OWNERS.lock().forget_write(Some(1), 10);
    assert_eq!(pending_for_vm(1), 1, "finish stays pending until its future returns");
    BROKER_OWNERS.lock().requests.remove(&3);
    assert_eq!(open_writes_for_vm(1), 0);
    assert!(open_vm(1));
}
#[test]
fn kill_aborts_forgotten_streams_without_waiting_for_another_writer() {
    let _serial = reset();
    began(4, 1, 11);
    // A second writer waits on the first stream's retained root lease.
    BROKER_OWNERS.lock().submit(5, Some(1), None);
    assert_eq!(request_release_vm(1), 1);
    queue_retired_write_aborts();
    assert_eq!(BROKER_REQUESTS.lock().len(), 1, "abort must pass a blocked competing begin");
    assert_eq!(pending_for_vm(1), 2);
    assert_eq!(open_writes_for_vm(1), 1);
    queue_retired_write_aborts();
    assert_eq!(BROKER_REQUESTS.lock().len(), 1, "never enqueue duplicate aborts");
    finish_abort();
    assert_eq!(open_writes_for_vm(1), 0);
    assert_eq!(pending_for_vm(1), 1, "the waiting begin must still finish or reject");
    BROKER_OWNERS.lock().requests.remove(&5);
    assert!(open_vm(1));
}
#[test]
fn abort_waits_for_running_chunk_and_tracks_a_late_begin() {
    let _serial = reset();
    began(6, 1, 12);
    BROKER_OWNERS.lock().submit(7, Some(1), Some(12));
    request_release_vm(1);
    queue_retired_write_aborts();
    assert!(BROKER_REQUESTS.lock().is_empty(), "an active chunk temporarily owns the stream");
    BROKER_OWNERS.lock().requests.remove(&7);
    queue_retired_write_aborts();
    assert_eq!(BROKER_REQUESTS.lock().len(), 1);
    finish_abort();
    // A begin already awaiting its lease can return after kill was requested.
    began(8, 1, 13);
    queue_retired_write_aborts();
    assert_eq!(BROKER_REQUESTS.lock().len(), 1);
    finish_abort();
    assert_eq!(pending_for_vm(1), 0);
    assert_eq!(open_writes_for_vm(1), 0);
}
#[test]
fn teardown_keeps_other_vm_assignments_and_does_not_accept_foreign_completion() {
    let _serial = reset();
    began(9, 1, 14);
    began(10, 2, 15);
    BROKER_OWNERS.lock().forget_write(Some(1), 15);
    assert_eq!(open_writes_for_vm(2), 1);
    request_release_vm(1);
    queue_retired_write_aborts();
    finish_abort();
    assert_eq!(open_writes_for_vm(1), 0);
    assert_eq!(open_writes_for_vm(2), 1);
    assert_eq!(pending_for_vm(2), 0);
}
#[test]
fn same_stream_chunks_serialize_while_independent_streams_can_progress() {
    let _serial = reset();
    let mut owners = BROKER_OWNERS.lock();
    owners.submit(11, Some(1), Some(16));
    owners.submit(12, Some(1), Some(16));
    owners.submit(13, Some(1), Some(17));
    assert!(owners.stream_turn(11, 16));
    assert!(!owners.stream_turn(12, 16), "second chunk must not see a temporarily absent stream");
    assert!(owners.stream_turn(13, 17));
    owners.requests.remove(&11);
    assert!(owners.stream_turn(12, 16));
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-fs-broker-lifecycle-") as directory:
        path = Path(directory)
        rust = path / "lifecycle.rs"
        rust.write_text(harness + production)
        binary = path / "lifecycle"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "--target", "x86_64-unknown-linux-gnu",
             str(rust), "-o", str(binary)], check=True, cwd=ROOT,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
