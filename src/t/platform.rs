//! Rust ABI hooks shared by TRUEOS-aware vendored crates.

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_monotonic_nanos() -> u64 {
    crate::chronos::monotonic_nanos()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_unix_seconds() -> u64 {
    crate::chronos::best_effort_unix_time_seconds().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_cpu_count() -> usize {
    let smp_count = crate::smp::cpu_count();
    if smp_count != 0 {
        return smp_count;
    }

    crate::percpu::total_slots().max(1)
}
