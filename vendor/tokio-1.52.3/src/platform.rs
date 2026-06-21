//! TRUEOS kernel platform hooks used by Tokio internals.
//!
//! This is the Tokio-facing view of services that `std` normally gets from an
//! OS: time, CPU topology, parking, sleep/yield, and eventually proper thread
//! synchronization. The goal is to keep Tokio's concepts intact while routing
//! them through TRUEOS Platform/Core services instead of Unix/POSIX shims.

pub(crate) const SEMANTIC_GAP_MUTEX_SPIN: u32 = 1;
pub(crate) const SEMANTIC_GAP_RUNTIME_PARK_POLL: u32 = 2;
pub(crate) const SEMANTIC_GAP_BLOCKING_POOL_POLL: u32 = 3;
pub(crate) const SEMANTIC_GAP_MULTI_THREAD_PARK_POLL: u32 = 4;
pub(crate) const SEMANTIC_GAP_BARRIER_POLL: u32 = 5;
pub(crate) const TRUEOS_DEBUG_BUILD_DRIVER_NEW: u32 = 6;
pub(crate) const TRUEOS_DEBUG_BUILD_BLOCKING_POOL: u32 = 7;
pub(crate) const TRUEOS_DEBUG_BUILD_CURRENT_THREAD: u32 = 8;
pub(crate) const TRUEOS_DEBUG_BUILD_CURRENT_THREAD_READY: u32 = 9;

unsafe extern "Rust" {
    fn trueos_platform_cpu_count() -> usize;
    fn trueos_tokio_worker_carriers_enabled() -> bool;
    fn trueos_tokio_platform_monotonic_nanos() -> u64;
    fn trueos_tokio_platform_poll_once();
    fn trueos_tokio_platform_sleep_ms(ms: u64);
    fn trueos_tokio_platform_wait_observe(key: u64) -> u32;
    fn trueos_tokio_platform_wait_after(key: u64, observed: u32, timeout_ms: u64) -> bool;
    fn trueos_tokio_platform_wait(key: u64, timeout_ms: u64) -> bool;
    fn trueos_tokio_platform_wake_one(key: u64) -> bool;
    fn trueos_tokio_platform_wake_all(key: u64) -> usize;
    fn trueos_tokio_platform_log_semantic_gap(code: u32);
    fn trueos_tokio_platform_log(level: u32, bytes: *const u8, len: usize);
}

#[inline]
pub(crate) fn cpu_count() -> usize {
    unsafe { trueos_platform_cpu_count().max(1) }
}

#[inline]
pub(crate) fn worker_carriers_enabled() -> bool {
    unsafe { trueos_tokio_worker_carriers_enabled() }
}

#[inline]
pub(crate) fn monotonic_nanos() -> u64 {
    unsafe { trueos_tokio_platform_monotonic_nanos() }
}

#[inline]
pub(crate) fn poll_once() {
    unsafe { trueos_tokio_platform_poll_once() }
}

#[inline]
pub(crate) fn sleep_ms(ms: u64) {
    unsafe { trueos_tokio_platform_sleep_ms(ms) }
}

#[inline]
pub(crate) fn wait_observe(key: u64) -> u32 {
    unsafe { trueos_tokio_platform_wait_observe(key) }
}

#[inline]
pub(crate) fn wait_after(key: u64, observed: u32, timeout_ms: u64) -> bool {
    unsafe { trueos_tokio_platform_wait_after(key, observed, timeout_ms) }
}

#[inline]
pub(crate) fn wait(key: u64, timeout_ms: u64) -> bool {
    unsafe { trueos_tokio_platform_wait(key, timeout_ms) }
}

#[inline]
pub(crate) fn wake_one(key: u64) -> bool {
    unsafe { trueos_tokio_platform_wake_one(key) }
}

#[inline]
pub(crate) fn wake_all(key: u64) -> usize {
    unsafe { trueos_tokio_platform_wake_all(key) }
}

#[inline]
pub(crate) fn note_semantic_gap(code: u32) {
    unsafe { trueos_tokio_platform_log_semantic_gap(code) }
}

#[inline]
pub(crate) fn log(level: u32, bytes: &[u8]) {
    unsafe { trueos_tokio_platform_log(level, bytes.as_ptr(), bytes.len()) }
}
