//! Native wait transport for Rust's atomic synchronization algorithms.
//!
//! The atomic value owns the synchronization state. TRUEOS wait generations
//! only arrange sleeping and waking, and are scoped to the caller's VM.

use crate::sync::atomic::{AtomicU32, Ordering::Relaxed};
use crate::time::Duration;

pub type Primitive = u32;
pub type Futex = AtomicU32;
pub type SmallPrimitive = u32;
pub type SmallFutex = AtomicU32;

unsafe extern "Rust" {
    fn trueos_tokio_platform_wait_observe(key: u64) -> u32;
    fn trueos_tokio_platform_wait_after(key: u64, observed: u32, timeout_ms: u64) -> bool;
    fn trueos_tokio_platform_wake_one(key: u64) -> bool;
    fn trueos_tokio_platform_wake_all(key: u64) -> usize;
}

/// Wait only while the atomic retains `expected`; false means a timeout.
pub fn futex_wait(futex: &Futex, expected: Primitive, timeout: Option<Duration>) -> bool {
    let key = key(futex);
    // Observe before checking the atomic. A wake after this observation either
    // changes the predicate or advances the generation seen by wait_after.
    let observed = unsafe { trueos_tokio_platform_wait_observe(key) };
    if futex.load(Relaxed) != expected {
        return true;
    }
    unsafe { trueos_tokio_platform_wait_after(key, observed, timeout_millis(timeout)) }
}

pub fn futex_wake(futex: &Futex) -> bool {
    unsafe { trueos_tokio_platform_wake_one(key(futex)) }
}

pub fn futex_wake_all(futex: &Futex) {
    unsafe { trueos_tokio_platform_wake_all(key(futex)) };
}

#[inline]
fn key(futex: &Futex) -> u64 {
    // The platform treats this address as opaque identity, never as a pointer
    // to dereference. Reused addresses may receive harmless spurious wakes.
    futex as *const Futex as usize as u64
}

fn timeout_millis(timeout: Option<Duration>) -> u64 {
    // Zero probes, MAX waits forever. Round finite waits up; durations too
    // large for a finite platform deadline become infinite, like Unix futex.
    timeout.map_or(u64::MAX, |duration| {
        u64::try_from(duration.as_nanos().div_ceil(1_000_000)).unwrap_or(u64::MAX)
    })
}
