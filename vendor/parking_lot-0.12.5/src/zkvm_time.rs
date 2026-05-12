use std::time::Instant;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
unsafe extern "Rust" {
    fn trueos_platform_monotonic_nanos() -> u64;
}

#[inline]
pub(crate) fn instant_now() -> Instant {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        Instant::from_nanos(unsafe { trueos_platform_monotonic_nanos() })
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        Instant::now()
    }
}
