// Reference source for `library/std/src/sys/thread/trueos.rs`.
//
// TRUEOS std threads are stackful logical threads scheduled on platform
// carriers. Each owns its stack, execution identity and keyed TLS namespace;
// blocking a logical thread must leave its carrier available to other threads.

use crate::ffi::{CStr, c_char, c_int, c_void};
use crate::io;
use crate::mem::ManuallyDrop;
use crate::num::NonZero;
use crate::ptr;
use crate::thread::ThreadInit;
use crate::time::Duration;

unsafe extern "C" {
    // A successful spawn owns `arg`, starts `entry` exactly once, and writes a
    // nonzero handle. On failure `entry` is never called and `arg` stays owned
    // by the caller. Errors are positive platform errno values.
    fn trueos_cabi_thread_spawn(
        stack: usize,
        entry: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
        out: *mut usize,
    ) -> c_int;
    // Join includes entry return, keyed TLS destructors and execution cleanup.
    fn trueos_cabi_thread_join(handle: usize) -> c_int;
    fn trueos_cabi_thread_detach(handle: usize) -> c_int;
    fn trueos_cabi_thread_available_parallelism() -> usize;
    fn trueos_cabi_thread_current_id() -> usize;
    fn trueos_cabi_thread_set_name(name: *const c_char) -> c_int;
    fn trueos_cabi_poll_once();
    fn trueos_cabi_sleep_ms(ms: u64);
}

/// An opaque, process-owned TRUEOS logical thread handle.
pub struct Thread {
    handle: usize,
}

pub const DEFAULT_MIN_STACK_SIZE: usize = 2 * 1024 * 1024;

impl Thread {
    // SAFETY: see `std::thread::Builder::spawn_unchecked` for requirements on
    // the lifetime of references captured by the entry closure.
    pub unsafe fn new(stack: usize, init: Box<ThreadInit>) -> io::Result<Thread> {
        let data = Box::into_raw(init);
        let mut handle = 0;
        let ret =
            unsafe { trueos_cabi_thread_spawn(stack, thread_start, data.cast(), &mut handle) };
        if ret != 0 {
            // The platform did not accept the entry point and cannot access
            // `data` after an unsuccessful spawn.
            unsafe { drop(Box::from_raw(data)) };
            return Err(io::Error::from_raw_os_error(ret));
        }
        assert_ne!(handle, 0, "TRUEOS returned an invalid thread handle");
        Ok(Thread { handle })
    }

    pub fn join(self) {
        // Joining consumes the platform handle; do not detach it again when
        // the Rust wrapper leaves scope.
        let handle = ManuallyDrop::new(self).handle;
        let ret = unsafe { trueos_cabi_thread_join(handle) };
        assert_eq!(ret, 0, "failed to join TRUEOS thread: {}", io::Error::from_raw_os_error(ret));
    }
}

unsafe extern "C" fn thread_start(data: *mut c_void) -> *mut c_void {
    // `ThreadInit::init` must run before allocations or user code on the new
    // logical thread. The platform has already installed its execution/TLS
    // identity. Its exit path runs TLS destructors after this entry returns.
    let init = unsafe { Box::from_raw(data.cast::<ThreadInit>()) };
    let rust_start = init.init();
    rust_start();
    ptr::null_mut()
}

impl Drop for Thread {
    fn drop(&mut self) {
        let ret = unsafe { trueos_cabi_thread_detach(self.handle) };
        debug_assert_eq!(ret, 0, "failed to detach TRUEOS thread");
    }
}

/// Number of carriers on which this process can run its logical threads.
pub fn available_parallelism() -> io::Result<NonZero<usize>> {
    NonZero::new(unsafe { trueos_cabi_thread_available_parallelism() })
        .ok_or(io::Error::UNKNOWN_THREAD_COUNT)
}

/// TRUEOS exposes a stable execution identity for every logical thread.
pub fn current_os_id() -> Option<u64> {
    NonZero::new(unsafe { trueos_cabi_thread_current_id() }).map(|id| id.get() as u64)
}

pub fn set_name(name: &CStr) {
    unsafe { trueos_cabi_thread_set_name(name.as_ptr()) };
}

/// Yield the current logical thread and make platform progress.
pub fn yield_now() {
    unsafe { trueos_cabi_poll_once() }
}

/// Round up to milliseconds so the call never returns earlier solely because
/// the TRUEOS CABI has millisecond granularity.
pub fn sleep(dur: Duration) {
    sleep_with(dur, |chunk| unsafe { trueos_cabi_sleep_ms(chunk) });
}

fn sleep_with(dur: Duration, mut sleep_ms: impl FnMut(u64)) {
    let mut millis = dur.as_millis();
    if dur.subsec_nanos() % 1_000_000 != 0 {
        millis += 1;
    }

    while millis != 0 {
        let chunk = crate::cmp::min(millis, u64::MAX as u128) as u64;
        sleep_ms(chunk);
        millis -= chunk as u128;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_rounds_up_without_sleeping_for_zero() {
        for (duration, expected) in [
            (Duration::ZERO, 0),
            (Duration::from_nanos(1), 1),
            (Duration::from_millis(1), 1),
            (Duration::from_nanos(1_000_001), 2),
            (Duration::from_secs(11), 11_000),
        ] {
            let mut total = 0u64;
            let mut calls = 0;
            sleep_with(duration, |ms| {
                total += ms;
                calls += 1;
            });
            assert_eq!(total, expected);
            assert_eq!(calls, if expected == 0 { 0 } else { 1 });
        }
    }

    #[test]
    fn large_duration_preserves_every_millisecond() {
        let duration = Duration::MAX;
        let mut total = 0u128;
        sleep_with(duration, |ms| {
            assert_ne!(ms, 0);
            total += ms as u128;
        });
        assert_eq!(total, duration.as_millis() + 1);
    }
}
