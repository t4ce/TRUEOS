use core::time::Duration;

pub(crate) fn platform_instant_now() -> std::time::Instant {
    let duration = Duration::from_nanos(crate::platform::monotonic_nanos());

    // Rust's unsupported std time backend stores Instant as a single Duration.
    // TRUEOS supplies the missing clock value through Tokio platform hooks.
    unsafe { core::mem::transmute::<Duration, std::time::Instant>(duration) }
}
