use core::time::Duration;

pub(crate) fn platform_instant_now() -> Duration {
    Duration::from_nanos(crate::platform::monotonic_nanos())
}
