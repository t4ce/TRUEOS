//! Rust ABI hooks shared by TRUEOS-aware vendored crates.
//!
//! This module is the common Platform/Core contract for vendored Rust crates.
//! It intentionally names OS-shaped services in Rust terms instead of exposing
//! POSIX symbols. `core` supplies atomics and memory rules; TRUEOS supplies the
//! execution environment that `std` would normally assume: time, topology,
//! sleep/yield, and wait-aware synchronization.

use core::sync::atomic::{AtomicU64, Ordering};

static TOKIO_CPU_COUNT_LOGGED: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_monotonic_nanos() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_monotonic_nanos();
    }

    crate::chronos::monotonic_nanos()
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_unix_seconds() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_unix_seconds();
    }

    crate::chronos::best_effort_unix_time_seconds().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_platform_cpu_count() -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some()
        && let Some(count) = crate::hv::vmcall::guest_cpu_count()
    {
        return count;
    }

    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return crate::hv::blueprint_exposed_cpu_count(vm_id);
    }

    let registered = crate::workers::registered_core_spawner_count();
    let topology_slots = crate::workers::topology_core_slot_count();
    let app_lanes = crate::workers::app_visible_parallelism();
    let count = app_lanes.max(1);

    if TOKIO_CPU_COUNT_LOGGED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!(
            "tokio-platform: cpu_count count={} app_lanes={} registered_spawners={} fallback={} smp={} percpu_slots={}\n",
            count,
            app_lanes,
            registered,
            topology_slots.saturating_sub(2).max(1),
            crate::smp::cpu_count(),
            crate::percpu::total_slots()
        );
    }

    count
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wait_observe(key: u64) -> u32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_platform_wait_observe(key);
    }
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return crate::wait::platform_wait_observe_for_vm(vm_id, key);
    }
    crate::wait::platform_wait_observe(key)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wait_after(
    key: u64,
    observed: u32,
    timeout_ms: u64,
) -> bool {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_platform_wait_after(key, observed, timeout_ms);
    }

    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return crate::wait::platform_wait_after_for_vm(vm_id, key, observed, timeout_ms);
    }

    crate::wait::platform_wait_after(key, observed, timeout_ms)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wake_one(key: u64) -> bool {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, woke) =
            crate::hv::vmcall::guest_call(crate::hv::vmcall::OP_BP_PLATFORM_WAKE_ONE, key, 0);
        return status == crate::hv::vmcall::STATUS_OK && woke != 0;
    }
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return crate::wait::platform_wake_one_for_vm(vm_id, key);
    }
    crate::wait::platform_wake_one(key)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_wake_all(key: u64) -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, count) =
            crate::hv::vmcall::guest_call(crate::hv::vmcall::OP_BP_PLATFORM_WAKE_ALL, key, 0);
        return if status == crate::hv::vmcall::STATUS_OK {
            count as usize
        } else {
            0
        };
    }
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return crate::wait::platform_wake_all_for_vm(vm_id, key);
    }
    crate::wait::platform_wake_all(key)
}
