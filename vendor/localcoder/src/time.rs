#[cfg(all(any(target_os = "trueos", target_os = "zkvm"), feature = "trueos-net"))]
#[inline]
pub(crate) fn unix_time_seconds() -> u64 {
    v::vclock::ntp_current_unix_seconds()
}

#[cfg(not(all(any(target_os = "trueos", target_os = "zkvm"), feature = "trueos-net")))]
#[inline]
pub(crate) fn unix_time_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
