#!/usr/bin/env python3
"""Exercise the production BSP async-FS pump, ordering and lifecycle with rustc."""

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
    consumer = item(FS_SOURCE, "service_task")
    assert consumer.index("ASYNC_FS_WAIT.observe()") < consumer.index("poll_request_batch(")
    assert consumer.index("arrival.as_mut().poll(cx)") < consumer.index("poll_request_batch(")
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
        "notify_request_arrival", "write_commit", "pending_in", "pending_for_owner",
        "release_owner", "begin_request", "finish_request", "discard",
        "process_owned_request", "poll_request_batch", "service_task",
    ))
    active_limit = re.search(r"^const ASYNC_FS_MAX_ACTIVE:.*;$", FS_SOURCE, re.MULTILINE)[0]
    active_type = re.search(r"^type ActiveRequest =.*;$", FS_SOURCE, re.MULTILINE)[0]
    assert "Send" not in active_type
    return r'''#![allow(dead_code)]
extern crate alloc;
use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::future::{Future, poll_fn};
use core::pin::{Pin, pin};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use core::task::{Context, Poll, Waker};
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
''' + active_limit + "\n" + active_type + r'''
const FS_ERR_NO_SPACE: i32 = -1;
const FS_ERR_NOT_FOUND: i32 = -2;
const FS_ERR_BAD_PARAM: i32 = -3;
static REGISTRY_WAITERS: AtomicU32 = AtomicU32::new(0);
static ASYNC_FS_SEQUENCE: AtomicU32 = AtomicU32::new(1);
static ASYNC_FS_WAIT: wait::WaitQueue = wait::WaitQueue::new();
static ASYNC_FS_REQUESTS: Mutex<VecDeque<Request>> = Mutex::new(VecDeque::new());
static ASYNC_FS_ACTIVE_REQUESTS: Mutex<[Option<(u32, u32)>; ASYNC_FS_MAX_ACTIVE]> =
    Mutex::new([None; ASYNC_FS_MAX_ACTIVE]);
static ASYNC_FS_OPERATIONS: Mutex<BTreeMap<u32, Operation>> = Mutex::new(BTreeMap::new());
struct Tracker;
static PROGRESS: Tracker = Tracker;
impl Tracker {
    fn admit(&self, _: u32, _: u32, _: u32, _: u64) {}
    fn release(&self, _: u32, _: u32) {}
    fn begin(&self, _: u32, _: u32, _: u64) {}
    fn done(&self, _: u32, _: u32, _: u64) -> Option<()> { None }
}
fn trace_slow_progress(_: ()) {}
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
    Unit,
    Failed(i32),
}
static SLOW_OWNER: AtomicU32 = AtomicU32::new(0);
static RELEASE_SLOW: AtomicBool = AtomicBool::new(false);
static SLOW_WAKER: Mutex<Option<Waker>> = Mutex::new(None);
static STARTED: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());
async fn process(request: &Request) -> OperationState {
    // Deliberately non-Send: the real block futures must also stay on the BSP.
    let same_lane = std::rc::Rc::new(request.owner);
    STARTED.lock().push((request.owner, request.id));
    if request.owner == SLOW_OWNER.load(Ordering::Relaxed) {
        poll_fn(|cx| {
            if RELEASE_SLOW.load(Ordering::Relaxed) { Poll::Ready(()) }
            else { *SLOW_WAKER.lock() = Some(cx.waker().clone()); Poll::Pending }
        }).await;
    }
    assert_eq!(*same_lane, request.owner);
    OperationState::Unit
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
    *ASYNC_FS_ACTIVE_REQUESTS.lock() = [None; ASYNC_FS_MAX_ACTIVE];
    remote_work_wake::WAKES.store(0, Ordering::Relaxed);
    SLOW_OWNER.store(0, Ordering::Relaxed);
    RELEASE_SLOW.store(false, Ordering::Relaxed);
    *SLOW_WAKER.lock() = None;
    STARTED.lock().clear();
}
fn stat_for(owner: u32) -> u32 {
    let id = start_inner(owner, RequestKind::Stat { path: "config".into() });
    assert!(id > 0);
    id as u32
}
fn done(id: u32) -> bool {
    ASYNC_FS_OPERATIONS.lock().get(&id).is_some_and(|op| matches!(op.state, OperationState::Unit))
}
fn release_slow() {
    RELEASE_SLOW.store(true, Ordering::Relaxed);
    if let Some(waker) = SLOW_WAKER.lock().take() { waker.wake(); }
}

#[test]
fn sleeping_service_is_woken_by_admission_and_rearms_after_drain() {
    reset();
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut service = pin!(service_task());
    assert!(poll(service.as_mut(), &wakes).is_pending());
    for expected_wakes in 1..=2 {
        let id = stat_for(7);
        assert_eq!(wakes.0.load(Ordering::Relaxed), expected_wakes);
        assert!(poll(service.as_mut(), &wakes).is_pending());
        assert!(done(id));
    }
    assert_eq!(remote_work_wake::WAKES.load(Ordering::Relaxed), 2);
}

#[test]
fn committed_upload_wakes_service_and_rejected_commit_does_not() {
    reset();
    ASYNC_FS_OPERATIONS.lock().insert(123, Operation {
        owner: 7,
        state: OperationState::Upload {
            path: "config".into(), bytes: vec![1, 2], total_len: 2,
            content_type: 1, legacy_blob: false, checked: None,
        },
    });
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut service = pin!(service_task());
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert_eq!(write_commit(8, 123), FS_ERR_NOT_FOUND);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
    assert_eq!(write_commit(7, 123), 0);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(remote_work_wake::WAKES.load(Ordering::Relaxed), 1);
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert!(done(123));
}

#[test]
fn arrival_between_empty_check_and_registration_is_retained() {
    reset();
    let observed = ASYNC_FS_WAIT.observe();
    assert!(take_request().is_none());
    let id = stat_for(7);
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut waiting = pin!(ASYNC_FS_WAIT.wait_after(observed));
    assert!(poll(waiting.as_mut(), &wakes).is_ready(), "early notification was lost");
    assert_eq!(take_request().unwrap().id, id);
}

#[test]
fn service_lane_arrival_before_registration_bypasses_timeout() {
    let queue = wait::WaitQueue::new();
    let observed = queue.observe();
    assert!(!queue.notify_one());
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut waiting = pin!(queue.wait_after_timeout(observed, 10));
    assert!(matches!(poll(waiting.as_mut(), &wakes), Poll::Ready(true)));
    assert_eq!(wakes.0.load(Ordering::Relaxed), 0);
}

#[test]
fn cancelled_idle_wait_does_not_consume_next_services_wake() {
    reset();
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    {
        let mut cancelled = pin!(service_task());
        assert!(poll(cancelled.as_mut(), &wakes).is_pending());
    }
    let mut live = pin!(service_task());
    assert!(poll(live.as_mut(), &wakes).is_pending());
    let id = stat_for(7);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert!(poll(live.as_mut(), &wakes).is_pending());
    assert!(done(id));
}

#[test]
fn slow_owner_does_not_block_other_owner_or_busy_poll() {
    reset();
    SLOW_OWNER.store(7, Ordering::Relaxed);
    let first = stat_for(7);
    let second = stat_for(7);
    let other = stat_for(8);
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut service = pin!(service_task());
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert!(!done(first));
    assert!(!done(second));
    assert!(done(other), "slow owner blocked independent work");
    assert_eq!(*STARTED.lock(), [(7, first), (8, other)]);
    assert_eq!(pending_for_owner(7), 2);
    let previous = wakes.0.load(Ordering::Relaxed);
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert_eq!(wakes.0.load(Ordering::Relaxed), previous, "blocked owner busy-polled");
    release_slow();
    assert!(wakes.0.load(Ordering::Relaxed) > previous);
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert!(done(first));
    assert!(!done(second), "next same-owner request bypassed pump fairness turn");
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert!(done(second));
    assert_eq!(*STARTED.lock(), [(7, first), (8, other), (7, second)]);
    assert_eq!(pending_for_owner(7), 0);
}

#[test]
fn concurrent_owner_slots_are_bounded_and_skip_queued_same_owner() {
    reset();
    let ids: Vec<_> = (1..=ASYNC_FS_MAX_ACTIVE as u32 + 1).map(stat_for).collect();
    for owner in 1..=ASYNC_FS_MAX_ACTIVE as u32 {
        let request = take_request().expect("unused active slot");
        assert_eq!(request.owner, owner);
    }
    assert!(take_request().is_none(), "concurrency limit exceeded");
    let first = Request { owner: 1, id: ids[0], kind: RequestKind::Stat { path: "".into() } };
    finish_request(&first, Some(OperationState::Unit));
    assert_eq!(take_request().unwrap().owner, ASYNC_FS_MAX_ACTIVE as u32 + 1);
}

#[test]
fn discarded_active_write_stays_pending_until_commit_boundary() {
    reset();
    SLOW_OWNER.store(7, Ordering::Relaxed);
    let id = start_inner(7, RequestKind::Write {
        path: "database".into(), bytes: vec![1, 2], content_type: 1,
        legacy_blob: false, checked: None,
    }) as u32;
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut service = pin!(service_task());
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert_eq!(discard(7, id), 0);
    assert_eq!(pending_for_owner(7), 1);
    assert_eq!(release_owner(7), Err(1), "discarded write allowed early owner reuse");
    let other = stat_for(8);
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert!(done(other));
    assert_eq!(release_owner(7), Err(1));
    release_slow();
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert_eq!(pending_for_owner(7), 0);
    assert_eq!(release_owner(7), Ok(0));
}

#[test]
fn discarded_queued_request_is_skipped_before_its_owner_successor() {
    reset();
    let abandoned = stat_for(7);
    let successor = stat_for(7);
    assert_eq!(discard(7, abandoned), 0);
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let mut service = pin!(service_task());
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert!(!done(successor));
    assert!(poll(service.as_mut(), &wakes).is_pending());
    assert!(done(successor));
    assert_eq!(*STARTED.lock(), [(7, successor)]);
    assert_eq!(release_owner(7), Ok(1));
}

#[test]
fn pending_count_includes_submitting_and_running_owners_without_duplicates() {
    reset();
    let queued = stat_for(7);
    ASYNC_FS_OPERATIONS.lock().insert(10_000, Operation { owner: 7, state: OperationState::Pending });
    ASYNC_FS_OPERATIONS.lock().insert(10_001, Operation { owner: 7, state: OperationState::Unit });
    assert_eq!(pending_for_owner(7), 2);
    let request = take_request().unwrap();
    assert_eq!(request.id, queued);
    assert_eq!(pending_for_owner(7), 2);
    assert_eq!(release_owner(7), Err(2));
    finish_request(&request, Some(OperationState::Unit));
    assert_eq!(pending_for_owner(7), 1);
    ASYNC_FS_OPERATIONS.lock().remove(&10_000);
    assert_eq!(release_owner(7), Ok(2));
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
