//! TRUEOS kernel platform hooks used by Hyper internals.

use core::time::Duration;
use crate::time::{Instant, SystemTime, UNIX_EPOCH};

unsafe extern "Rust" {
    fn trueos_platform_monotonic_nanos() -> u64;
    fn trueos_platform_unix_seconds() -> u64;
}

#[inline]
pub(crate) fn instant_now() -> Instant {
    let duration = Duration::from_nanos(unsafe { trueos_platform_monotonic_nanos() });
    Instant::from_std(duration)
}

#[inline]
pub(crate) fn system_time_now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(unsafe { trueos_platform_unix_seconds() })
}
