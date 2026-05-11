//! Rust ABI hooks shared by TRUEOS-aware vendored crates.
//!
//! This module is the common Platform/Core contract for vendored Rust crates.
//! It intentionally names OS-shaped services in Rust terms instead of exposing
//! POSIX symbols. `core` supplies atomics and memory rules; TRUEOS supplies the
//! execution environment that `std` would normally assume: time, topology,
//! sleep/yield, and eventually wait-aware synchronization.

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
