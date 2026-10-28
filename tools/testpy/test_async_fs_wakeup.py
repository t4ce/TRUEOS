#!/usr/bin/env python3
"""Exercise production async-FS admission and WaitQueue wakeups with host rustc."""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FS_SOURCE = (ROOT / "src/r/io/async_fs_cabi.rs").read_text()
WAIT_SOURCE = (ROOT / "src/wait.rs").read_text()


def item(source: str, name: str) -> str:
    match = re.search(
        rf"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+{name}\b",
        source,
        re.MULTILINE,
    )
    if match is None:
        raise ValueError(f"missing function {name}")
    end = re.search(r"^}\s*\n", source[match.end():], re.MULTILINE)
    if end is None:
        raise ValueError(f"missing closing brace for {name}")
    return source[match.start():match.end() + end.end()]


def harness_source() -> str:
    consumer = item(FS_SOURCE, "next_request")
    assert consumer.index("ASYNC_FS_WAIT.observe()") < consumer.index("take_request()")
    blocking = item((ROOT / "src/r/blocking.rs").read_text(), "service_lane_executor_task")
    assert blocking.index("service_lane_wait(slot).observe()") < blocking.index("service_lane_queue_depth(slot)")
    assert ".wait_after_timeout(observed, SERVICE_LANE_IDLE_POLL_MS)" in blocking

    # Compile the actual generation/registration implementation. Only the
    # unrelated synchronous polling half of WaitQueue is omitted.
    wait_start = WAIT_SOURCE.index("pub struct WaitQueue {")
    wait_end = WAIT_SOURCE.index("    pub fn wait_for_event_blocking(")
    wait = WAIT_SOURCE[wait_start:wait_end].removesuffix("    #[inline]\n") + "}\n"
    functions = "\n".join(item(FS_SOURCE, name) for name in (
        "lock_operations", "next_operation_id", "start_inner", "take_request",
        "next_request", "notify_request_arrival", "write_commit",
    ))
    return r'''#![allow(dead_code)]
extern crate alloc;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use core::task::{Context, Poll, Waker};
use std::pin::Pin;
use std::sync::Arc;

mod spin {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    pub type MutexGuard<'a, T> = std::sync::MutexGuard<'a, T>;
    impl<T> Mutex<T> {
        pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
        pub fn lock(&self) -> MutexGuard<'_, T> { self.0.lock().unwrap() }
    }
}
use spin::Mutex;
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() }
}
mod remote_work_wake {
    pub static WAKES: super::AtomicUsize = super::AtomicUsize::new(0);
    pub fn wake_cpu_for_remote_work(slot: u32) {
        assert_eq!(slot, 0);
        WAKES.fetch_add(1, super::Ordering::Relaxed);
    }
}
mod trueos_time {
    pub struct Timer;
    impl Timer {
        pub async fn after_millis(_: u64) { core::future::pending::<()>().await }
    }
}
mod wait {
    use super::*;
''' + wait + r'''
}
const ASYNC_FS_MAX_OPERATIONS: usize = 64;
const FS_ERR_NO_SPACE: i32 = -1;
const FS_ERR_NOT_FOUND: i32 = -2;
const FS_ERR_BAD_PARAM: i32 = -3;
static REGISTRY_WAITERS: AtomicU32 = AtomicU32::new(0);
static ASYNC_FS_SEQUENCE: AtomicU32 = AtomicU32::new(1);
static ASYNC_FS_WAIT: wait::WaitQueue = wait::WaitQueue::new();
static ASYNC_FS_REQUESTS: Mutex<VecDeque<Request>> = Mutex::new(VecDeque::new());
static ASYNC_FS_ACTIVE_REQUEST: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static ASYNC_FS_OPERATIONS: Mutex<BTreeMap<u32, Operation>> = Mutex::new(BTreeMap::new());
struct Tracker;
static PROGRESS: Tracker = Tracker;
impl Tracker {
    fn admit(&self, _: u32, _: u32, _: u32, _: u64) {}
    fn release(&self, _: u32, _: u32) {}
}
fn progress_now_ms() -> u64 { 1 }
enum RequestKind {
    Stat { path: String },
    Write { path: String, bytes: Vec<u8>, content_type: u32, legacy_blob: bool, checked: Option<()> },
}
impl RequestKind {
    fn progress_class(&self) -> u32 { match self { Self::Stat { .. } => 4, Self::Write { .. } => 2 } }
}
struct Request { owner: u32, id: u32, kind: RequestKind }
struct Operation { owner: u32, state: OperationState }
enum OperationState {
    Pending,
    Upload { path: String, bytes: Vec<u8>, total_len: usize, content_type: u32, legacy_blob: bool, checked: Option<()> },
    Failed(i32),
}
''' + functions + r'''
struct WakeCount(AtomicUsize);
impl std::task::Wake for WakeCount {
    fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::Relaxed); }
}
fn poll<F: Future>(future: Pin<&mut F>, wakes: &Arc<WakeCount>) -> Poll<F::Output> {
    let waker = Waker::from(wakes.clone());
    future.poll(&mut Context::from_waker(&waker))
}
fn reset() {
    ASYNC_FS_REQUESTS.lock().clear();
    ASYNC_FS_OPERATIONS.lock().clear();
    *ASYNC_FS_ACTIVE_REQUEST.lock() = None;
    remote_work_wake::WAKES.store(0, Ordering::Relaxed);
}
fn stat() -> i32 { start_inner(7, RequestKind::Stat { path: "config".into() }) }
fn finish() { *ASYNC_FS_ACTIVE_REQUEST.lock() = None; }

#[test]
fn existing_requests_are_consumed_without_sleep_and_keep_fifo_order() {
    reset();
    let first = stat();
    let second = stat();
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    for id in [first, second] {
        let mut next = std::pin::pin!(next_request());
        let Poll::Ready(request) = poll(next.as_mut(), &wakes) else { panic!("queued request slept") };
        assert_eq!(request.id as i32, id);
        assert_eq!(request.owner, 7);
        finish();
    }
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
}

#[test]
fn sleeping_service_is_woken_by_read_admission_and_rearms_after_drain() {
    reset();
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    for expected_wakes in 1..=2 {
        let mut next = std::pin::pin!(next_request());
        assert!(poll(next.as_mut(), &wakes).is_pending());
        assert_eq!(wakes.0.load(Ordering::Relaxed), expected_wakes - 1);
        let id = stat();
        assert_eq!(wakes.0.load(Ordering::Relaxed), expected_wakes);
        let Poll::Ready(request) = poll(next.as_mut(), &wakes) else { panic!("admission did not wake service") };
        assert_eq!(request.id as i32, id);
        finish();
    }
    assert_eq!(remote_work_wake::WAKES.load(Ordering::Relaxed), 2);
}

#[test]
fn committed_upload_wakes_the_service_and_rejected_commit_does_not() {
    reset();
    ASYNC_FS_OPERATIONS.lock().insert(123, Operation {
        owner: 7,
        state: OperationState::Upload {
            path: "config".into(), bytes: vec![1, 2], total_len: 2,
            content_type: 1, legacy_blob: false, checked: None,
        },
    });
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut next = std::pin::pin!(next_request());
    assert!(poll(next.as_mut(), &wakes).is_pending());
    assert_eq!(write_commit(8, 123), FS_ERR_NOT_FOUND);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    assert_eq!(write_commit(7, 123), 0);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(remote_work_wake::WAKES.load(Ordering::Relaxed), 1);
    let Poll::Ready(request) = poll(next.as_mut(), &wakes) else { panic!("commit did not wake service") };
    assert_eq!(request.id, 123);
    assert!(matches!(request.kind, RequestKind::Write { bytes, .. } if bytes == [1, 2]));
    finish();
}

#[test]
fn arrival_between_empty_check_and_registration_is_retained() {
    reset();
    let observed = ASYNC_FS_WAIT.observe();
    assert!(take_request().is_none());
    let id = stat();
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut wait = std::pin::pin!(ASYNC_FS_WAIT.wait_after(observed));
    assert!(poll(wait.as_mut(), &wakes).is_ready(), "early notification was lost");
    assert_eq!(take_request().unwrap().id as i32, id);
    finish();
}

#[test]
fn service_lane_arrival_before_registration_bypasses_timeout() {
    let queue = wait::WaitQueue::new();
    let observed = queue.observe();
    // The publisher wins after the consumer checked for queued work, before
    // it could register a waker. The timer stub never expires in this test.
    assert!(!queue.notify_one());
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut waiting = std::pin::pin!(queue.wait_after_timeout(observed, 10));
    assert!(matches!(poll(waiting.as_mut(), &wakes), Poll::Ready(true)));
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
}

#[test]
fn cancelled_idle_wait_does_not_consume_the_next_services_wake() {
    reset();
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    {
        let mut cancelled = std::pin::pin!(next_request());
        assert!(poll(cancelled.as_mut(), &wakes).is_pending());
    }
    let mut live = std::pin::pin!(next_request());
    assert!(poll(live.as_mut(), &wakes).is_pending());
    stat();
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert!(poll(live.as_mut(), &wakes).is_ready());
    finish();
}
'''


def run_contracts() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-async-fs-wakeup-") as temporary:
        directory = Path(temporary)
        source = directory / "tests.rs"
        executable = directory / "tests"
        source.write_text(harness_source())
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
            check=True,
        )
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    run_contracts()
