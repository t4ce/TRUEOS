//! Rust ABI hooks consumed by the vendored Tokio TRUEOS platform layer.

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_monotonic_nanos() -> u64 {
    crate::chronos::monotonic_nanos()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_poll_once() {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        crate::hv::vmcall::guest_yield();
        return;
    }
    crate::wait::spin_step();
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_sleep_ms(ms: u64) {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        crate::hv::vmcall::guest_sleep_ms(ms);
        return;
    }
    if ms == 0 {
        crate::wait::spin_step();
        return;
    }
    let _ = crate::wait::spin_until_timeout(ms, || false);
}
