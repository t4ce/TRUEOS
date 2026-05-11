//! TRUEOS kernel platform hooks used by Hyper internals.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

unsafe extern "Rust" {
    fn trueos_platform_monotonic_nanos() -> u64;
    fn trueos_platform_unix_seconds() -> u64;
}

#[inline]
pub(crate) fn instant_now() -> Instant {
    let duration = Duration::from_nanos(unsafe { trueos_platform_monotonic_nanos() });

    // Rust's unsupported std time backend stores Instant as a single Duration.
    // TRUEOS supplies the missing clock value through Rust platform hooks.
    unsafe { core::mem::transmute::<Duration, Instant>(duration) }
}

#[inline]
pub(crate) fn system_time_now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(unsafe { trueos_platform_unix_seconds() })
}
