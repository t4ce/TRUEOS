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

    support = '''#![allow(dead_code, unused_imports)]
extern crate alloc;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod allcaps { pub mod hv { pub const VM_CPU_SLOT_LIMIT: usize = 1; } }
mod hv { pub fn current_hull_guest_context_vm_id() -> Option<u8> { None } }
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
    support += registers + wait_items + platform_items + "\n}\n"
    support += '''mod r {
    pub mod blocking {
        pub type BlockingJobFn = alloc::boxed::Box<dyn FnOnce() + Send + 'static>;
        pub struct GuestJobOwner;
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
    support += scheduler + "\n}\n}\n"
    with tempfile.TemporaryDirectory(prefix="trueos-thread-scheduler-") as directory:
        folder = Path(directory)
        (folder / "src").mkdir()
        (folder / "Cargo.toml").write_text(
            '[package]\nname="thread-scheduler-tests"\nversion="0.1.0"\n'
            'edition="2024"\n[workspace]\n'
        )
        (folder / "src/lib.rs").write_text(support)
        subprocess.run([
            "cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--target-dir", str(ROOT / "bld/thread-scheduler-host-tests"),
        ], cwd=folder, check=True)


if __name__ == "__main__":
    main()
