//! TRUEOS kernel platform hooks used by Mio internals.

unsafe extern "Rust" {
    fn trueos_platform_monotonic_nanos() -> u64;
    fn trueos_tokio_platform_wait_observe(key: u64) -> u32;
    fn trueos_tokio_platform_wait_after(key: u64, observed: u32, timeout_ms: u64) -> bool;
    fn trueos_tokio_platform_wake_all(key: u64) -> usize;
}

// Shared network/console readiness generation, scoped to the calling VM (or
// host). This is the kernel's wait::BLUEPRINT_IO_WAIT_KEY, not a selector ID.
const IO_WAIT_KEY: u64 = 0x4250_494f_0000_0001;

#[inline]
pub(crate) fn monotonic_nanos() -> u64 {
    unsafe { trueos_platform_monotonic_nanos() }
}

pub(crate) fn observe_io() -> u32 {
    unsafe { trueos_tokio_platform_wait_observe(IO_WAIT_KEY) }
}

pub(crate) fn wait_io_after(observed: u32, timeout_ms: u64) -> bool {
    unsafe { trueos_tokio_platform_wait_after(IO_WAIT_KEY, observed, timeout_ms) }
}

pub(crate) fn wake_io() {
    unsafe { trueos_tokio_platform_wake_all(IO_WAIT_KEY) };
}
