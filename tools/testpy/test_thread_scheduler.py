#!/usr/bin/env python3
"""Host acceptance for production stack switching, ThreadTask and wait queues.

Only carrier realm/topology/time services are mocked. Scheduler transitions,
stack assembly, async wait generations, CompletionCell and Rust tests come
directly from the kernel sources.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    thread_path = ROOT / "src/r/threads.rs"
    thread_source = thread_path.read_text()
    scheduler = thread_source[:thread_source.index("#[trueos_executor::task(")]
    scheduler = re.sub(r"^mod (context|stack|tests|timing|exit_trace);$", lambda match:
        f'#[path="{ROOT / "src/r/threads" / (match[1] + ".rs")}"] mod {match[1]};',
        scheduler, flags=re.MULTILINE)
    if not re.search(r"mod tests;", scheduler):
        scheduler += f'\n#[path="{ROOT / "src/r/threads/tests.rs"}"] mod tests;\n'
    # Inner module documentation must precede the host-only import below.
    scheduler = re.sub(r"^//!.*\n", "", scheduler, flags=re.MULTILINE)

    wait_source = (ROOT / "src/wait.rs").read_text()
    registers = wait_source[wait_source.index("pub fn register_waker_list("):
                            wait_source.index("/// Single spin step")]
    wait_items = wait_source[wait_source.index("pub struct WaitQueue {"):
                            wait_source.index("type JobFuture =")]
    platform_items = wait_source[wait_source.index("const PLATFORM_WAIT_HOST_SCOPE:"):
                                wait_source.index("struct LocalJobQueue {")]

    support = '''#![allow(dead_code, unused_imports, unexpected_cfgs)]
extern crate alloc;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod allcaps {
    pub mod hv {
        pub const VM_CPU_SLOT_LIMIT: usize = 1; pub const VM_ID_LIMIT: usize = 64;
        pub const INPUT_TIMING_ENABLED: bool = true; pub const CODEX_PROGRESS_PROBES_ENABLED: bool = true;
    }
    pub mod wls { pub const CPU_TRACK_COUNT: usize = 1; pub const WORKER_SLOT_COUNT: usize = 16; pub const TLS_SLOT_COUNT: usize = 4096; }
}
mod hv {
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use crate::trueos_time::{Timer, Duration as EmbassyDuration};
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { None }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { crate::r::threads::current_vm_id() }
    static KILLED: [core::sync::atomic::AtomicBool; 64] = [const { core::sync::atomic::AtomicBool::new(false) }; 64];
    pub fn guest_kill_requested(vm: u8) -> bool { KILLED[vm as usize].load(core::sync::atomic::Ordering::Acquire) }
    pub fn set_guest_kill_for_test(vm: u8, value: bool) { KILLED[vm as usize].store(value, core::sync::atomic::Ordering::Release); }
    struct Pause(AtomicBool);
    impl Pause { fn requested(&self) -> bool { self.0.load(Ordering::Acquire) } }
    struct Vm {
        stop_req: AtomicBool,
        execution_pause: Pause,
        execution_pause_deadline_ms: AtomicU64,
    }
    static VMS: [Vm; 64] = [const { Vm {
        stop_req: AtomicBool::new(false), execution_pause: Pause(AtomicBool::new(false)),
        execution_pause_deadline_ms: AtomicU64::new(u64::MAX),
    }}; 64];
    static IO_ACTIVE: [core::sync::atomic::AtomicBool; 64] = [const { core::sync::atomic::AtomicBool::new(false) }; 64];
    pub fn guest_pause_requested(vm: u8) -> bool { VMS[vm as usize].execution_pause.requested() }
    pub fn guest_native_pause_requested(vm: u8) -> bool { guest_pause_requested(vm) && !IO_ACTIVE[vm as usize].load(core::sync::atomic::Ordering::Acquire) }
    pub fn set_guest_pause_for_test(vm: u8, value: bool) { VMS[vm as usize].execution_pause.0.store(value, Ordering::Release); }
    pub fn set_guest_io_for_test(vm: u8, value: bool) { IO_ACTIVE[vm as usize].store(value, core::sync::atomic::Ordering::Release); }
    pub fn vm_run_generation(_: u8) -> Option<u64> { Some(19) }
    fn vm_slot(vm: u8) -> Option<&'static Vm> { VMS.get(vm as usize) }
    fn lifecycle_now_ms() -> u64 { crate::time_driver::now() }
    const TRUEOS_VM_ID_LIMIT: usize = 64;
    __CONTROL_BOUNDARY__
    async fn park_vm_execution(vm: u8) -> bool {
        loop {
            let observed = VM_CONTROL_WAITS[vm as usize].observe();
            if guest_kill_requested(vm) { return false; }
            if !guest_pause_requested(vm) { return true; }
            VM_CONTROL_WAITS[vm as usize].wait_after(observed).await;
        }
    }
    #[test]
    fn stop_or_kill_interrupts_an_indefinite_hull_wait() {
        let vm = 61;
        let mut future = core::pin::pin!(await_vm_control_boundary(vm, core::future::pending::<()>()));
        let mut cx = core::task::Context::from_waker(std::task::Waker::noop());
        assert!(core::future::Future::poll(future.as_mut(), &mut cx).is_pending());
        set_guest_kill_for_test(vm, true);
        VM_CONTROL_WAITS[vm as usize].notify_all();
        assert_eq!(core::future::Future::poll(future.as_mut(), &mut cx), core::task::Poll::Ready(None));
        set_guest_kill_for_test(vm, false);
    }
    #[test]
    fn latched_kill_does_not_wait_for_another_notification() {
        let vm = 62;
        set_guest_kill_for_test(vm, true);
        let mut future = core::pin::pin!(await_vm_control_boundary(vm, core::future::pending::<()>()));
        let mut cx = core::task::Context::from_waker(std::task::Waker::noop());
        assert_eq!(core::future::Future::poll(future.as_mut(), &mut cx), core::task::Poll::Ready(None));
        set_guest_kill_for_test(vm, false);
    }
    #[test]
    fn pause_retains_the_original_pending_hull_wait_until_resume() {
        use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let vm = 60;
        let ready = AtomicBool::new(false);
        let polls = AtomicUsize::new(0);
        let inner = core::future::poll_fn(|_| {
            polls.fetch_add(1, Ordering::AcqRel);
            if ready.load(Ordering::Acquire) { core::task::Poll::Ready(42) }
            else { core::task::Poll::Pending }
        });
        let mut future = core::pin::pin!(await_vm_control_boundary(vm, inner));
        let mut cx = core::task::Context::from_waker(std::task::Waker::noop());
        assert!(core::future::Future::poll(future.as_mut(), &mut cx).is_pending());
        assert_eq!(polls.load(Ordering::Acquire), 1);
        set_guest_pause_for_test(vm, true);
        VM_CONTROL_WAITS[vm as usize].notify_all();
        assert!(core::future::Future::poll(future.as_mut(), &mut cx).is_pending());
        ready.store(true, Ordering::Release);
        assert!(core::future::Future::poll(future.as_mut(), &mut cx).is_pending());
        assert_eq!(polls.load(Ordering::Acquire), 1, "paused wait ran guest-associated work");
        set_guest_pause_for_test(vm, false);
        VM_CONTROL_WAITS[vm as usize].notify_all();
        assert_eq!(core::future::Future::poll(future.as_mut(), &mut cx), core::task::Poll::Ready(Some(42)));
        assert_eq!(polls.load(Ordering::Acquire), 2);
    }
    #[test]
    fn cooperative_stop_returns_a_sleeping_hull_to_its_cleanup_code() {
        let vm = 63;
        let mut future = core::pin::pin!(await_vm_control_boundary(vm, core::future::pending::<()>()));
        let mut cx = core::task::Context::from_waker(std::task::Waker::noop());
        assert!(core::future::Future::poll(future.as_mut(), &mut cx).is_pending());
        VMS[vm as usize].stop_req.store(true, Ordering::Release);
        VM_CONTROL_WAITS[vm as usize].notify_all();
        assert!(!guest_kill_requested(vm));
        assert_eq!(core::future::Future::poll(future.as_mut(), &mut cx), core::task::Poll::Ready(None));
        VMS[vm as usize].stop_req.store(false, Ordering::Release);
    }
}
mod percpu { pub fn current_slot() -> usize { 0 } }
mod allocators {
    static STATE: std::sync::Mutex<[u32; 4]> = std::sync::Mutex::new([0; 4]);
    pub fn replace_thread_context(value: [u32; 4]) -> [u32; 4] {
        std::mem::replace(&mut *STATE.lock().unwrap(), value)
    }
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() }
}
__WLS__
mod trueos_time {
    pub use std::time::Duration;
    pub struct Timer(std::time::Instant);
    impl Timer {
        pub fn after(duration: Duration) -> Self { Self(std::time::Instant::now() + duration) }
        pub fn after_millis(ms: u64) -> Self {
            Self(std::time::Instant::now() + std::time::Duration::from_millis(ms))
        }
    }
    impl core::future::Future for Timer {
        type Output = ();
        fn poll(self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<()> {
            if std::time::Instant::now() >= self.0 { core::task::Poll::Ready(()) }
            else { cx.waker().wake_by_ref(); core::task::Poll::Pending }
        }
    }
}
mod wait {
    use super::{Mutex, trueos_time};
    use alloc::{vec::Vec, boxed::Box, collections::BTreeMap, sync::Arc};
    use core::future::Future;
    use core::pin::Pin;
    use core::sync::atomic::{AtomicU32, Ordering};
    use core::task::{Context, Poll, Waker};
    const TICK_HZ: u64 = 1000;
    fn now() -> u64 {
        static START: std::sync::LazyLock<std::time::Instant> = std::sync::LazyLock::new(std::time::Instant::now);
        START.elapsed().as_millis() as u64
    }
    fn spin_step() {
        if !crate::r::threads::yield_now() { std::thread::yield_now(); }
    }
    fn spin_step_no_exec() { spin_step(); }
'''
    hv_source = (ROOT / "src/hv/mod.rs").read_text()
    control = hv_source[hv_source.index("static VM_CONTROL_WAITS:"):hv_source.index("\nstruct TrueosVmId")]
    support = support.replace("__CONTROL_BOUNDARY__", control)
    platform_tests = r"""
#[cfg(test)] mod hull_platform_wait_contract_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;
    struct Count(AtomicUsize);
    impl Wake for Count { fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::Relaxed); } }
    fn counter() -> (Arc<Count>, Waker) {
        let count = Arc::new(Count(AtomicUsize::new(0)));
        (count.clone(), Waker::from(count))
    }
    #[test] fn native_notify_reaches_hull_wait_only_in_the_same_vm_scope() {
        let (a, aw) = counter(); let (b, bw) = counter();
        let key = 0x9911;
        let mut first = Box::pin(platform_wait_after_for_vm_async(200, key, platform_wait_observe_for_vm(200, key), u64::MAX));
        let mut other = Box::pin(platform_wait_after_for_vm_async(201, key, platform_wait_observe_for_vm(201, key), u64::MAX));
        assert!(first.as_mut().poll(&mut Context::from_waker(&aw)).is_pending());
        assert!(other.as_mut().poll(&mut Context::from_waker(&bw)).is_pending());
        assert!(platform_wake_one_for_vm(200, key));
        assert_eq!(a.0.load(Ordering::Relaxed), 1);
        assert_eq!(b.0.load(Ordering::Relaxed), 0);
        assert_eq!(first.as_mut().poll(&mut Context::from_waker(&aw)), Poll::Ready(true));
        assert!(other.as_mut().poll(&mut Context::from_waker(&bw)).is_pending());
        drop((first, other)); retire_platform_vm_waits(200); retire_platform_vm_waits(201);
    }
    #[test] fn completed_hull_wait_drop_cannot_unregister_a_new_wait_with_the_same_waker() {
        let (_, waker) = counter(); let key = 0x9922;
        let mut old = Box::pin(platform_wait_after_for_vm_async(202, key, platform_wait_observe_for_vm(202, key), u64::MAX));
        assert!(old.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        assert!(platform_wake_one_for_vm(202, key));
        let mut replacement = Box::pin(platform_wait_after_for_vm_async(202, key, platform_wait_observe_for_vm(202, key), u64::MAX));
        assert!(replacement.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        assert_eq!(old.as_mut().poll(&mut Context::from_waker(&waker)), Poll::Ready(true));
        drop(old);
        assert!(platform_wake_one_for_vm(202, key), "replacement registration was lost");
        assert_eq!(replacement.as_mut().poll(&mut Context::from_waker(&waker)), Poll::Ready(true));
        drop(replacement); retire_platform_vm_waits(202);
    }
    #[test] fn expired_hull_wait_preserves_generation_and_later_same_key_notifications() {
        let (_, waker) = counter(); let key = 0x9933;
        let observed = platform_wait_observe_for_vm(203, key);
        let mut old = Box::pin(platform_wait_after_for_vm_async(203, key, observed, 1));
        assert!(old.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        std::thread::sleep(std::time::Duration::from_millis(3));
        assert_eq!(old.as_mut().poll(&mut Context::from_waker(&waker)), Poll::Ready(false));
        drop(old);
        assert_eq!(platform_wait_observe_for_vm(203, key), observed);
        let mut next = Box::pin(platform_wait_after_for_vm_async(203, key, observed, u64::MAX));
        assert!(next.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        assert!(platform_wake_one_for_vm(203, key));
        assert_eq!(next.as_mut().poll(&mut Context::from_waker(&waker)), Poll::Ready(true));
        drop(next); retire_platform_vm_waits(203);
    }
}
"""
    support += registers + wait_items + platform_items + platform_tests + "\n}\n"
    timing = (ROOT / 'src/r/input_timing.rs').read_text().replace('embassy_time_driver::', 'crate::time_driver::').replace('//!', '//')
    probe = (ROOT / 'src/r/progress_probe.rs').read_text().replace('//!', '//')
    support += 'mod r { pub mod progress_probe {\n' + probe + '\n}\n'
    support += 'pub mod input_timing {\n' + timing + '\n}\n'
    support += '''
    pub mod blocking {
        pub type BlockingJobFn = alloc::boxed::Box<dyn FnOnce() + Send + 'static>;
        __DIAGNOSTICS__
        mod lifetime { __LIFETIME__ }
        pub(crate) use lifetime::{GuestJobOwner, open_guest_jobs, pause_guest_jobs, resume_guest_jobs,
            close_guest_jobs, guest_jobs_in_flight, guest_jobs_quiescent};
        pub(crate) use lifetime::reserve as reserve_guest_job;
        fn now_ms() -> u64 { crate::time_driver::now() }
        __CANCELLATION__
    }
    pub mod kernel_task_domain {
        pub enum KernelTaskDomain { HostService, VmGuestOwnedAlloc }
        static STATE: std::sync::Mutex<(u32, u8)> = std::sync::Mutex::new((0, 0));
        pub fn replace_context(value: (u32, u8)) -> (u32, u8) {
            std::mem::replace(&mut *STATE.lock().unwrap(), value)
        }
    }
    pub mod threads {
        use crate::trueos_time;
'''
    scheduler = scheduler.replace("spin::Mutex", "crate::Mutex")
    support += scheduler + "\n}\n}\n"
    support += '''
mod log_os { pub fn blueprint_important_line(_: core::fmt::Arguments<'_>) {} }
#[macro_export] macro_rules! log_warn { ($($tokens:tt)*) => {} }
#[macro_export] macro_rules! log { ($($tokens:tt)*) => {} }
static SLICE_LOGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
fn capture_log(args: core::fmt::Arguments<'_>) {
    let line = args.to_string();
    if line.starts_with("codex-slice-exit ") {
        assert_eq!(crate::r::threads::current_id(), None, "slice formatted before clearing guest execution context");
        SLICE_LOGS.lock().unwrap().push(line);
    }
}
#[macro_export] macro_rules! log_info { (target: $target:expr; $($tokens:tt)*) => { crate::capture_log(format_args!($($tokens)*)) } }
mod time_driver {
    pub const TICK_HZ: u64 = 1000;
    pub fn now() -> u64 {
        static START: std::sync::LazyLock<std::time::Instant> = std::sync::LazyLock::new(std::time::Instant::now);
        START.elapsed().as_millis() as u64
    }
}
'''
    diagnostic = (ROOT / 'src/r/blocking/diagnostics.rs').read_text().replace('embassy_time_driver::', 'crate::time_driver::')
    diagnostic = diagnostic.replace('#[path = "abi_breadcrumb.rs"] mod abi_breadcrumb;', f'#[path="{ROOT / "src/r/blocking/abi_breadcrumb.rs"}"] mod abi_breadcrumb;')
    support = support.replace('__WLS__', f'#[path="{ROOT / "src/wls.rs"}"] mod wls;')
    support = support.replace('__DIAGNOSTICS__', 'pub mod diagnostics {\n' + diagnostic.replace('//!', '//') + '\n}')
    lifetime = (ROOT / 'src/r/blocking/lifetime.rs').read_text().replace('//!', '//')
    lifetime = lifetime.replace('trueos_time::', 'crate::trueos_time::')
    support = support.replace('__LIFETIME__', lifetime)
    blocking = (ROOT / 'src/r/blocking.rs').read_text()
    start = blocking.index('pub(crate) fn guest_job_cancellation_requested(')
    end = blocking.index('\n}', start) + 2
    support = support.replace('__CANCELLATION__', blocking[start:end])
    with tempfile.TemporaryDirectory(prefix="trueos-thread-scheduler-") as directory:
        folder = Path(directory)
        (folder / "src").mkdir()
        (folder / "Cargo.toml").write_text(
            '[package]\nname="thread-scheduler-tests"\nversion="0.1.0"\n'
            'edition="2024"\n[dependencies]\nspin="0.10"\nheapless="0.9"\n[workspace]\n'
        )
        (folder / "src/lib.rs").write_text(support)
        subprocess.run([
            "cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--target-dir", str(ROOT / "bld/thread-scheduler-host-tests"),
            "--config", 'build.rustflags=["--cfg","thread_scheduler_harness","-C","force-frame-pointers=yes"]',
            # Realm/CURRENT mocks represent one physical carrier. Parallel
            # host test threads must not share that simulated CPU state.
            "--", "--test-threads=1",
        ], cwd=folder, check=True)


if __name__ == "__main__":
    main()
