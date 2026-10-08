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
    scheduler = re.sub(r"^mod (context|stack|tests);$", lambda match:
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
mod allcaps { pub mod hv { pub const VM_CPU_SLOT_LIMIT: usize = 1; pub const VM_ID_LIMIT: usize = 64; } }
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { None }
    static KILLED: [core::sync::atomic::AtomicBool; 64] = [const { core::sync::atomic::AtomicBool::new(false) }; 64];
    pub fn guest_kill_requested(vm: u8) -> bool { KILLED[vm as usize].load(core::sync::atomic::Ordering::Acquire) }
    pub fn set_guest_kill_for_test(vm: u8, value: bool) { KILLED[vm as usize].store(value, core::sync::atomic::Ordering::Release); }
    fn vm_slot(vm: u8) -> Option<u8> { Some(vm) }
    fn immediate_stop_requested(vm: u8) -> bool { guest_kill_requested(vm) }
    const TRUEOS_VM_ID_LIMIT: usize = 64;
    __CONTROL_BOUNDARY__
    #[test]
    fn stop_or_kill_interrupts_an_indefinite_hull_wait() {
        let vm = 61;
        let mut future = core::pin::pin!(await_vm_control_boundary(vm, core::future::pending::<()>()));
        let mut cx = core::task::Context::from_waker(std::task::Waker::noop());
        assert!(core::future::Future::poll(future.as_mut(), &mut cx).is_pending());
        VM_CONTROL_WAITS[vm as usize].notify_all();
        assert_eq!(core::future::Future::poll(future.as_mut(), &mut cx), core::task::Poll::Ready(None));
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
}
mod percpu { pub fn current_slot() -> usize { 0 } }
mod allocators {
    static STATE: std::sync::Mutex<[u32; 4]> = std::sync::Mutex::new([0; 4]);
    pub fn replace_thread_context(value: [u32; 4]) -> [u32; 4] {
        std::mem::replace(&mut *STATE.lock().unwrap(), value)
    }
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() }
}
mod wls {
    static STATE: std::sync::Mutex<(u64, u32)> = std::sync::Mutex::new((0, 0));
    pub fn replace_thread_context(value: (u64, u32)) -> (u64, u32) {
        std::mem::replace(&mut *STATE.lock().unwrap(), value)
    }
}
mod trueos_time {
    pub struct Timer(std::time::Instant);
    impl Timer {
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
    support += registers + wait_items + platform_items + "\n}\n"
    support += '''mod r {
    pub mod blocking {
        pub type BlockingJobFn = alloc::boxed::Box<dyn FnOnce() + Send + 'static>;
        __DIAGNOSTICS__
        pub struct GuestJobOwner { pub diagnostic: diagnostics::JobDiagnostic }
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
mod time_driver {
    pub const TICK_HZ: u64 = 1000;
    pub fn now() -> u64 {
        static START: std::sync::LazyLock<std::time::Instant> = std::sync::LazyLock::new(std::time::Instant::now);
        START.elapsed().as_millis() as u64
    }
}
'''
    diagnostic = (ROOT / 'src/r/blocking/diagnostics.rs').read_text().replace('embassy_time_driver::', 'crate::time_driver::')
    support = support.replace('__DIAGNOSTICS__', 'pub mod diagnostics {\n' + diagnostic.replace('//!', '//') + '\n}')
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
            "--config", 'build.rustflags=["--cfg","thread_scheduler_harness"]',
        ], cwd=folder, check=True)


if __name__ == "__main__":
    main()
