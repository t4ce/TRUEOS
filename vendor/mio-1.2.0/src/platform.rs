//! TRUEOS kernel platform hooks used by Mio internals.

unsafe extern "Rust" {
    fn trueos_platform_monotonic_nanos() -> u64;
}

#[inline]
pub(crate) fn monotonic_nanos() -> u64 {
    unsafe { trueos_platform_monotonic_nanos() }
}
