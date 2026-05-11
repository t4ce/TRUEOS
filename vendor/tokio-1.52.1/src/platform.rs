//! TRUEOS kernel platform hooks used by Tokio internals.

unsafe extern "Rust" {
    fn trueos_platform_cpu_count() -> usize;
    fn trueos_tokio_platform_monotonic_nanos() -> u64;
    fn trueos_tokio_platform_poll_once();
    fn trueos_tokio_platform_sleep_ms(ms: u64);
}

#[inline]
pub(crate) fn cpu_count() -> usize {
    unsafe { trueos_platform_cpu_count().max(1) }
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
