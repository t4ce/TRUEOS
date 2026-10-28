#![allow(dead_code, unused_imports)]
extern crate alloc;
extern crate self as embassy_time_driver;
extern crate self as trueos_time;

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const TICK_HZ: u64 = 1_000;
pub fn now() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub struct Timer { deadline: Instant }
impl Timer {
    pub fn after_millis(ms: u64) -> Self { Self { deadline: Instant::now() + Duration::from_millis(ms) } }
    pub fn after(duration: Duration) -> Self { Self { deadline: Instant::now() + duration } }
}
impl core::future::Future for Timer {
    type Output = ();
    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> core::task::Poll<()> {
        if Instant::now() >= self.deadline { core::task::Poll::Ready(()) }
        else {
            // These tests drive polls manually. Supply the same scheduling
            // obligation as the platform timer, without an extra timer thread.
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    }
}

static SLICE_LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
type Realm = ((u32, u8), [u32; 4], wls::ThreadContext);
static NATIVE_TERMINATIONS: Mutex<Vec<(u8, u64, (u32, u8), [u32; 4], wls::ThreadContext)>> = Mutex::new(Vec::new());

fn log_line(args: core::fmt::Arguments<'_>) {
    let line = args.to_string();
    if line.starts_with("codex-slice-exit") { SLICE_LOGS.lock().unwrap().push(line); }
}
#[macro_export]
macro_rules! log_info { (target: $target:literal; $($args:tt)*) => { $crate::log_line(format_args!($($args)*)) }; ($($args:tt)*) => { $crate::log_line(format_args!($($args)*)) }; }
#[macro_export]
macro_rules! log_warn { (target: $target:literal; $($args:tt)*) => { $crate::log_line(format_args!($($args)*)) }; ($($args:tt)*) => { $crate::log_line(format_args!($($args)*)) }; }
#[macro_export]
macro_rules! log { ($($args:tt)*) => { $crate::log_line(format_args!($($args)*)) }; }
mod log_os { pub fn blueprint_important_line(args: core::fmt::Arguments<'_>) { crate::log_line(args); } }

mod allcaps {
    pub mod hv {
        pub const VM_ID_LIMIT: usize = @VM_ID_LIMIT@;
        pub const VM_CPU_SLOT_LIMIT: usize = @VM_CPU_SLOT_LIMIT@;
        pub const CODEX_PROGRESS_PROBES_ENABLED: bool = true;
        pub const INPUT_TIMING_ENABLED: bool = true;
    }
    pub mod wls {
        pub const WORKER_SLOT_COUNT: usize = @WORKER_SLOT_COUNT@;
        pub const CPU_TRACK_COUNT: usize = @CPU_TRACK_COUNT@;
        pub const TLS_SLOT_COUNT: usize = @TLS_SLOT_COUNT@;
    }
}
mod percpu {
    use std::cell::Cell;
    std::thread_local! { static SLOT: Cell<usize> = const { Cell::new(0) }; }
    pub fn current_slot() -> usize { SLOT.get() }
    pub fn current_slot_via_cpuid() -> usize { current_slot() }
    pub fn set_slot(slot: usize) { SLOT.set(slot); }
}
mod allocators {
    use std::cell::Cell;
    std::thread_local! { static CONTEXT: Cell<[u32; 4]> = const { Cell::new([0; 4]) }; }
    pub fn replace_thread_context(next: [u32; 4]) -> [u32; 4] { CONTEXT.replace(next) }
    pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T { f() }
}
mod hv {
    use core::sync::atomic::{AtomicBool, Ordering};
    const VM_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;
    static KILL: [AtomicBool; VM_LIMIT] = [const { AtomicBool::new(false) }; VM_LIMIT];
    static PAUSE: [AtomicBool; VM_LIMIT] = [const { AtomicBool::new(false) }; VM_LIMIT];
    static IO: [AtomicBool; VM_LIMIT] = [const { AtomicBool::new(false) }; VM_LIMIT];
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { None }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> { crate::r::threads::current_vm_id() }
    pub fn vm_run_generation(_: u8) -> Option<u64> { Some(19) }
    pub fn guest_kill_requested(vm: u8) -> bool { KILL[vm as usize].load(Ordering::Acquire) }
    pub fn guest_native_pause_requested(vm: u8) -> bool { PAUSE[vm as usize].load(Ordering::Acquire) && !IO[vm as usize].load(Ordering::Acquire) }
    pub fn set_guest_kill_for_test(vm: u8, value: bool) { KILL[vm as usize].store(value, Ordering::Release); }
    pub fn set_guest_pause_for_test(vm: u8, value: bool) { PAUSE[vm as usize].store(value, Ordering::Release); }
    pub fn set_guest_io_for_test(vm: u8, value: bool) { IO[vm as usize].store(value, Ordering::Release); }
    pub fn native_guest_terminated(vm: u8, generation: u64) {
        let domain = crate::r::kernel_task_domain::replace_context((0, u8::MAX));
        crate::r::kernel_task_domain::replace_context(domain);
        let allocation = crate::allocators::replace_thread_context([0; 4]);
        crate::allocators::replace_thread_context(allocation);
        let wls = crate::wls::replace_thread_context((0, 0, None));
        crate::wls::replace_thread_context(wls);
        crate::NATIVE_TERMINATIONS.lock().unwrap().push((vm, generation, domain, allocation, wls));
        set_guest_kill_for_test(vm, true);
        crate::r::blocking::close_guest_jobs(vm);
    }
    pub mod vmcall { pub fn guest_yield() { unreachable!("host continuation used the Hull ABI"); } }
}
mod time { pub fn poll() {} }
mod runtime { pub fn poll_local_executor() {} }

#[path = "@TRUEOS_ROOT@/src/wls.rs"] mod wls;
mod workers {
    #[path = "@TRUEOS_ROOT@/src/workers/application_policy.rs"] pub mod application_policy;
}
mod wait { @WAIT@ }
mod r {
    #[path = "@TRUEOS_ROOT@/src/r/kernel_task_domain.rs"] pub mod kernel_task_domain;
    #[path = "@TRUEOS_ROOT@/src/r/input_timing.rs"] pub mod input_timing;
    #[path = "@TRUEOS_ROOT@/src/r/progress_probe.rs"] pub mod progress_probe;
    pub mod blocking {
        pub type BlockingJobFn = Box<dyn FnOnce() + Send + 'static>;
        #[path = "@TRUEOS_ROOT@/src/r/blocking/diagnostics.rs"] pub(crate) mod diagnostics;
        #[path = "@TRUEOS_ROOT@/src/r/blocking/lifetime.rs"] mod lifetime;
        pub(crate) use lifetime::*;
        pub(crate) use lifetime::reserve as reserve_guest_job;
        fn now_ms() -> u64 { crate::now() }
        pub(crate) fn guest_job_cancellation_requested(vm: u8) -> bool { guest_jobs_closed(vm) || crate::hv::guest_kill_requested(vm) }
    }
    pub mod threads { @THREADS@ }
}

#[cfg(test)]
#[path = "@TRUEOS_ROOT@/tools/testpy/thread_scheduler/tokio_experiments.rs"]
mod tokio_experiments;
