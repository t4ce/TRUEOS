#!/usr/bin/env python3
"""Poll the production RAM parking/control futures with owned I/O held open.

This exercises the execution boundary without privileged VMX instructions.
Native scheduler and BSP filesystem ownership have separate production tests.
"""
from pathlib import Path
import subprocess
import tempfile
import re


ROOT = Path(__file__).resolve().parents[2]


def production_item(name):
    source = (ROOT / "src/hv/mod.rs").read_text()
    match = re.search(rf"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn {name}\b", source, re.M)
    if not match:
        raise ValueError(f"missing production function {name}")
    end = re.search(r"^}\s*\n", source[match.end():], re.M)
    return source[match.start():match.end() + end.end()]


def main():
    source = r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::future::Future;
use std::task::{Poll, Context};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
static NOW: AtomicU64 = AtomicU64::new(0);
static JOBS_READY: AtomicBool = AtomicBool::new(true);
static SYNC_PENDING: AtomicUsize = AtomicUsize::new(0);
static WRITES: AtomicUsize = AtomicUsize::new(0);
static ASYNC_PENDING: AtomicUsize = AtomicUsize::new(0);
static ADMISSION: AtomicBool = AtomicBool::new(true);
static RELEASE_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static DISCARD_REQUESTS: AtomicUsize = AtomicUsize::new(0);
struct EmbassyDuration(u64);
impl EmbassyDuration { fn from_millis(ms: u64) -> Self { Self(ms) } }
struct Timer(u64);
impl Timer { fn after(ms: EmbassyDuration) -> Self { Self(NOW.load(Ordering::SeqCst) + ms.0) } }
impl Future for Timer {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if NOW.load(Ordering::SeqCst) >= self.0 { Poll::Ready(()) } else { Poll::Pending }
    }
}
mod wait {
    use super::*;
    pub struct WaitQueue(AtomicUsize);
    impl WaitQueue {
        pub const fn new() -> Self { Self(AtomicUsize::new(0)) }
        pub fn observe(&self) -> usize { self.0.load(Ordering::SeqCst) }
        pub fn notify_all(&self) { self.0.fetch_add(1, Ordering::SeqCst); }
        pub async fn wait_after(&self, seq: usize) {
            std::future::poll_fn(|_| if self.observe() != seq { Poll::Ready(()) } else { Poll::Pending }).await
        }
    }
}
mod shell3 { #[derive(PartialEq, Eq)] pub struct MatrixSlotLease(pub u64); }
mod log_os { pub fn blueprint_important_line(_: std::fmt::Arguments<'_>) {} }
mod percpu { pub fn current_slot() -> usize { 2 } }
mod r {
    pub mod blocking {
        use super::super::*;
        pub fn pause_guest_jobs(_: u8) -> usize { ADMISSION.store(false, Ordering::SeqCst); 1 }
        pub fn resume_guest_jobs(_: u8, _: u64) -> bool { ADMISSION.store(true, Ordering::SeqCst); true }
        pub fn close_guest_jobs(_: u8) -> usize { ADMISSION.store(false, Ordering::SeqCst); 1 }
        pub struct GuestDiscardSummary { pub queued: usize, pub retained: usize }
        pub fn discard_guest_jobs(_: u8, _: u64) -> GuestDiscardSummary {
            DISCARD_REQUESTS.fetch_add(1, Ordering::SeqCst);
            GuestDiscardSummary { queued: 0, retained: 0 }
        }
        pub fn guest_jobs_quiescent(_: u8) -> bool { JOBS_READY.load(Ordering::SeqCst) }
        pub fn guest_jobs_in_flight(_: u8) -> usize { 1 }
    }
    pub mod threads { pub fn wake_guest_control(_: u8) {} }
    pub mod fs { pub mod request_broker {
        use crate::*;
        pub fn pending_for_vm(_: u8) -> usize { SYNC_PENDING.load(Ordering::SeqCst) }
        pub fn open_writes_for_vm(_: u8) -> usize { WRITES.load(Ordering::SeqCst) }
        pub fn request_release_vm(_: u8) -> usize {
            RELEASE_REQUESTS.fetch_add(1, Ordering::SeqCst);
            WRITES.load(Ordering::SeqCst)
        }
    } }
    pub mod io { pub mod async_fs_cabi {
        use crate::*;
        pub fn owner_for_vm(id: u8) -> u32 { id as u32 }
        pub fn pending_for_owner(_: u32) -> usize { ASYNC_PENDING.load(Ordering::SeqCst) }
    } }
}
mod hv {
    use super::*;
    #[path = "@LIFECYCLE@"] mod lifecycle;
    #[path = "@COOPERATIVE@"] mod cooperative_stop;
    pub mod control_kick { pub enum LifecycleKickAction { Nudge, Stop } }
    const STOP_CLEANUP_TIMEOUT_MS: u64 = 2_000;
    const PAUSE_CHECKPOINT_TIMEOUT_MS: u64 = 15_000;
    struct TrueosVmId {
        running: AtomicBool, starting: AtomicBool, stop_req: AtomicBool,
        pause_latched: AtomicBool, execution_pause: lifecycle::PauseControl,
        lifecycle_control: Mutex<()>, run_generation: AtomicU64,
        lifecycle_generation: AtomicU64, cooperative_stop: cooperative_stop::CooperativeStop,
        preserve_req: AtomicBool, preserve_exit: AtomicBool,
        execution_pause_deadline_ms: AtomicU64, execution_pause_io_wait_logged: AtomicBool,
        matrix_owner: Mutex<Option<crate::shell3::MatrixSlotLease>>,
    }
    static VM: TrueosVmId = TrueosVmId {
        running: AtomicBool::new(true), starting: AtomicBool::new(false), stop_req: AtomicBool::new(false),
        pause_latched: AtomicBool::new(false), execution_pause: lifecycle::PauseControl::new(),
        lifecycle_control: Mutex::new(()), run_generation: AtomicU64::new(9),
        lifecycle_generation: AtomicU64::new(9), cooperative_stop: cooperative_stop::CooperativeStop::new(),
        preserve_req: AtomicBool::new(false), preserve_exit: AtomicBool::new(false),
        execution_pause_deadline_ms: AtomicU64::new(0), execution_pause_io_wait_logged: AtomicBool::new(false),
        matrix_owner: Mutex::new(None),
    };
    static VM_CONTROL_WAITS: [crate::wait::WaitQueue; 1] = [crate::wait::WaitQueue::new()];
    #[derive(Debug)] pub enum StopError { UnsupportedVmId }
    fn vm_slot(id: u8) -> Option<&'static TrueosVmId> { (id == 0).then_some(&VM) }
    fn lifecycle_now_ms() -> u64 { NOW.load(Ordering::SeqCst) }
    fn nudge_vm_control(_: u8, _: control_kick::LifecycleKickAction, _: &str) {}
    fn hvlogf(_: std::fmt::Arguments<'_>) {}
    fn hvwarnf(_: std::fmt::Arguments<'_>) {}
    static IMPORTANT: Mutex<Vec<String>> = Mutex::new(Vec::new());
    fn hvimportantf(args: std::fmt::Arguments<'_>) { IMPORTANT.lock().push(args.to_string()); }
    fn reset_prepare_pause(_: &TrueosVmId) {}
    fn clear_blueprint_lifecycle_capability(_: u8) {}
    @PRODUCTION@
    fn reset() {
        VM.execution_pause.resume();
        VM.cooperative_stop.reset();
        VM.running.store(true, Ordering::SeqCst);
        VM.starting.store(false, Ordering::SeqCst);
        VM.stop_req.store(false, Ordering::SeqCst);
        VM.lifecycle_generation.store(9, Ordering::SeqCst);
        VM.run_generation.store(9, Ordering::SeqCst);
        *VM.matrix_owner.lock() = None;
        IMPORTANT.lock().clear();
        NOW.store(0, Ordering::SeqCst);
        JOBS_READY.store(true, Ordering::SeqCst);
        SYNC_PENDING.store(0, Ordering::SeqCst);
        WRITES.store(0, Ordering::SeqCst);
        ASYNC_PENDING.store(0, Ordering::SeqCst);
        ADMISSION.store(true, Ordering::SeqCst);
        RELEASE_REQUESTS.store(0, Ordering::SeqCst);
        DISCARD_REQUESTS.store(0, Ordering::SeqCst);
    }
    fn poll<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(std::task::Waker::noop()))
    }
    #[test]
    fn pause_waits_for_native_jobs_and_both_filesystem_paths() {
        reset();
        JOBS_READY.store(false, Ordering::SeqCst);
        WRITES.store(1, Ordering::SeqCst);
        SYNC_PENDING.store(1, Ordering::SeqCst);
        ASYNC_PENDING.store(1, Ordering::SeqCst);
        assert!(pause(0).unwrap());
        assert!(!ADMISSION.load(Ordering::SeqCst));
        let mut first_boundary = std::pin::pin!(park_vm_execution(0));
        assert_eq!(poll(first_boundary.as_mut()), Poll::Ready(true), "Hull must continue to finish its write");
        assert!(!VM.execution_pause.paused());
        assert!(IMPORTANT.lock().iter().any(|line| line.contains("reason=filesystem") && line.contains("fs_streams=1")));
        JOBS_READY.store(true, Ordering::SeqCst);
        SYNC_PENDING.store(0, Ordering::SeqCst);
        ASYNC_PENDING.store(0, Ordering::SeqCst);
        NOW.store(1, Ordering::SeqCst);
        let mut second_boundary = std::pin::pin!(park_vm_execution(0));
        assert_eq!(poll(second_boundary.as_mut()), Poll::Ready(true));
        assert!(!VM.execution_pause.paused(), "streamed begin still owns a write lease");
        WRITES.store(0, Ordering::SeqCst);
        NOW.store(2, Ordering::SeqCst);
        let mut parked = std::pin::pin!(park_vm_execution(0));
        assert!(poll(parked.as_mut()).is_pending());
        assert!(IMPORTANT.lock().iter().any(|line| line.contains("filesystem checkpoint cleared")));
        assert!(VM.execution_pause.paused());
        assert!(VM.running.load(Ordering::SeqCst), "pause retains the occupied slot");
        assert!(resume(0).unwrap());
        assert_eq!(poll(parked.as_mut()), Poll::Ready(true));
        assert_eq!(VM.run_generation.load(Ordering::SeqCst), 9);
    }
    #[test]
    fn repeat_cycles_keep_the_original_host_wait_future_and_its_result() {
        reset();
        let polls = AtomicUsize::new(0);
        let dropped = AtomicBool::new(false);
        struct Wait<'a>(&'a AtomicUsize, &'a AtomicBool);
        impl Future for Wait<'_> {
            type Output = u64;
            fn poll(self: std::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<u64> {
                if self.0.fetch_add(1, Ordering::SeqCst) >= 3 { Poll::Ready(42) } else { Poll::Pending }
            }
        }
        impl Drop for Wait<'_> { fn drop(&mut self) { self.1.store(true, Ordering::SeqCst); } }
        let mut future = std::pin::pin!(await_vm_control_boundary(0, Wait(&polls, &dropped)));
        for _ in 0..3 {
            assert!(poll(future.as_mut()).is_pending());
            assert!(pause(0).unwrap());
            assert!(poll(future.as_mut()).is_pending());
            let before = polls.load(Ordering::SeqCst);
            assert!(VM.execution_pause.paused());
            assert!(poll(future.as_mut()).is_pending());
            assert_eq!(polls.load(Ordering::SeqCst), before);
            assert!(!dropped.load(Ordering::SeqCst));
            assert!(resume(0).unwrap());
        }
        assert_eq!(poll(future.as_mut()), Poll::Ready(Some(42)));
        assert!(dropped.load(Ordering::SeqCst));
    }
    #[test]
    fn kill_wakes_a_paused_host_wait_without_resuming_the_guest() {
        reset();
        assert!(pause(0).unwrap());
        let mut future = std::pin::pin!(await_vm_control_boundary(0, std::future::pending::<()>()));
        assert!(poll(future.as_mut()).is_pending());
        assert!(VM.execution_pause.paused());
        force_stop(0, &VM);
        assert_eq!(poll(future.as_mut()), Poll::Ready(None));
        assert!(VM.cooperative_stop.forced());
        assert_eq!(RELEASE_REQUESTS.load(Ordering::SeqCst), 1);
        assert_eq!(DISCARD_REQUESTS.load(Ordering::SeqCst), 1);
        assert!(!resume(0).unwrap());
    }
    #[test]
    fn native_terminal_failure_forces_only_its_live_incarnation() {
        reset();
        native_guest_terminated(0, 8);
        assert!(!VM.cooperative_stop.forced());
        assert_eq!(DISCARD_REQUESTS.load(Ordering::SeqCst), 0);
        native_guest_terminated(0, 9);
        assert!(VM.cooperative_stop.forced());
        assert!(!ADMISSION.load(Ordering::SeqCst));
        assert_eq!(DISCARD_REQUESTS.load(Ordering::SeqCst), 1);
        reset();
        VM.running.store(false, Ordering::SeqCst);
        native_guest_terminated(0, 9);
        assert!(!VM.cooperative_stop.forced());
    }
    #[test]
    fn pause_timeout_resumes_execution_and_never_claims_quiescence() {
        reset();
        WRITES.store(1, Ordering::SeqCst);
        assert!(pause(0).unwrap());
        let mut future = std::pin::pin!(await_vm_control_boundary(0, std::future::pending::<()>()));
        assert!(poll(future.as_mut()).is_pending());
        NOW.store(PAUSE_CHECKPOINT_TIMEOUT_MS, Ordering::SeqCst);
        assert!(poll(future.as_mut()).is_pending());
        assert!(!VM.execution_pause.paused());
        assert!(!VM.execution_pause.requested());
        assert!(ADMISSION.load(Ordering::SeqCst));
        assert!(IMPORTANT.lock().iter().any(|line| line.contains("reason=checkpoint-timeout") && line.contains("fs_streams=1")));
    }
    #[test]
    fn cooperative_stop_interrupts_wait_so_guest_can_acknowledge_cleanup() {
        reset();
        assert!(VM.cooperative_stop.register());
        let mut future = std::pin::pin!(await_vm_control_boundary(0, std::future::pending::<()>()));
        assert!(poll(future.as_mut()).is_pending());
        assert!(VM.cooperative_stop.request());
        VM.stop_req.store(true, Ordering::SeqCst);
        VM_CONTROL_WAITS[0].notify_all();
        assert_eq!(poll(future.as_mut()), Poll::Ready(None));
        assert!(!VM.cooperative_stop.forced());
    }
    #[test]
    fn discard_logs_why_an_abandoned_stream_keeps_its_slot_owned() {
        reset();
        WRITES.store(1, Ordering::SeqCst);
        force_stop(0, &VM);
        assert!(IMPORTANT.lock().iter().any(|line| line.contains("rollback requested")
            && line.contains("fs_streams=1") && line.contains("slot=retained-until-abort")));
    }
    #[test]
    fn stale_slot_lease_cannot_pause_or_resume_a_new_occupant() {
        reset();
        *VM.matrix_owner.lock() = Some(crate::shell3::MatrixSlotLease(2));
        let stale = crate::shell3::MatrixSlotLease(1);
        let current = crate::shell3::MatrixSlotLease(2);
        assert!(!pause_matching_owner(0, Some(&stale)).unwrap());
        assert!(pause_matching_owner(0, Some(&current)).unwrap());
        assert!(!resume_matching_owner(0, Some(&stale)).unwrap());
        assert!(VM.execution_pause.requested());
        assert!(resume_matching_owner(0, Some(&current)).unwrap());
    }
    #[test]
    fn cleanup_deadline_overrides_unresponsive_stop_but_not_a_new_incarnation() {
        reset();
        assert!(VM.cooperative_stop.register());
        assert!(VM.cooperative_stop.request());
        VM.stop_req.store(true, Ordering::SeqCst);
        let mut deadline = std::pin::pin!(stop_cleanup_deadline(0, 9));
        assert!(poll(deadline.as_mut()).is_pending());
        NOW.store(STOP_CLEANUP_TIMEOUT_MS, Ordering::SeqCst);
        assert!(poll(deadline.as_mut()).is_ready());
        assert!(VM.cooperative_stop.forced());
        reset();
        VM.stop_req.store(true, Ordering::SeqCst);
        let mut stale = std::pin::pin!(stop_cleanup_deadline(0, 9));
        assert!(poll(stale.as_mut()).is_pending());
        VM.lifecycle_generation.store(10, Ordering::SeqCst);
        NOW.store(STOP_CLEANUP_TIMEOUT_MS, Ordering::SeqCst);
        assert!(poll(stale.as_mut()).is_ready());
        assert!(!VM.cooperative_stop.forced());
    }
}
'''
    production = "\n".join(production_item(name) for name in (
        "pause", "resume", "guest_pause_requested", "guest_native_pause_requested",
        "park_vm_execution", "await_vm_control_boundary", "force_stop", "native_guest_terminated",
        "immediate_stop_requested", "stop_cleanup_deadline",
        "pause_matching_owner", "resume_matching_owner",
    ))
    source = source.replace("@LIFECYCLE@", str(ROOT / "src/hv/lifecycle.rs"))
    source = source.replace("@COOPERATIVE@", str(ROOT / "src/hv/cooperative_stop.rs"))
    source = source.replace("@PRODUCTION@", production)
    with tempfile.TemporaryDirectory(prefix="trueos-vmx-lifecycle-") as directory:
        directory = Path(directory)
        rust = directory / "lifecycle.rs"
        binary = directory / "lifecycle"
        rust.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust), "-o", str(binary)], check=True, cwd=ROOT)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
