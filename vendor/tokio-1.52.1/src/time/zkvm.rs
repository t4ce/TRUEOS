use std::time::Duration;

unsafe extern "C" {
    fn trueos_tokio_time_now_nanos() -> u64;
}

pub(crate) fn std_instant_now() -> std::time::Instant {
    let duration = Duration::from_nanos(unsafe { trueos_tokio_time_now_nanos() });

    // Rust's unsupported std time backend stores Instant as a single Duration.
    // TRUEOS supplies the missing clock value through the std ABI shim.
    unsafe { core::mem::transmute::<Duration, std::time::Instant>(duration) }
}
