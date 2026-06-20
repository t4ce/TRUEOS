//! Rust ABI hooks shared by TRUEOS-aware vendored crates.
//!
//! This module is the common Platform/Core contract for vendored Rust crates.
//! It intentionally names OS-shaped services in Rust terms instead of exposing
//! POSIX symbols. `core` supplies atomics and memory rules; TRUEOS supplies the
//! execution environment that `std` would normally assume: time, topology,
//! sleep/yield, and eventually wait-aware synchronization.

use core::sync::atomic::{AtomicU64, Ordering};

static TOKIO_SEMANTIC_GAPS_LOGGED: AtomicU64 = AtomicU64::new(0);
static TOKIO_CPU_COUNT_LOGGED: AtomicU64 = AtomicU64::new(0);

const SEMANTIC_GAP_MUTEX_SPIN: u32 = 1;
const SEMANTIC_GAP_RUNTIME_PARK_POLL: u32 = 2;
const SEMANTIC_GAP_BLOCKING_POOL_POLL: u32 = 3;
const SEMANTIC_GAP_MULTI_THREAD_PARK_POLL: u32 = 4;
const SEMANTIC_GAP_BARRIER_POLL: u32 = 5;
const TRUEOS_DEBUG_BUILD_DRIVER_NEW: u32 = 6;
const TRUEOS_DEBUG_BUILD_BLOCKING_POOL: u32 = 7;
const TRUEOS_DEBUG_BUILD_CURRENT_THREAD: u32 = 8;
const TRUEOS_DEBUG_BUILD_CURRENT_THREAD_READY: u32 = 9;
const TRUEOS_LOG_INFO: u32 = 3;
const TRUEOS_LOG_WARN: u32 = 4;
const TRUEOS_LOG_ERROR: u32 = 5;

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
pub extern "Rust" fn trueos_tokio_worker_carriers_enabled() -> bool {
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return crate::hv::blueprint_exposed_cpu_count(vm_id) > 1;
    }

    crate::workers::app_visible_parallelism() > 1
}

fn tokio_semantic_gap_message(code: u32) -> &'static str {
    match code {
        SEMANTIC_GAP_MUTEX_SPIN => {
            "tokio-platform: loom Mutex uses a Core spin lock; no parking/fairness/wait queue yet"
        }
        SEMANTIC_GAP_RUNTIME_PARK_POLL => {
            "tokio-platform: runtime parker uses Platform sleep/poll; no kernel parker wait queue yet"
        }
        SEMANTIC_GAP_BLOCKING_POOL_POLL => {
            "tokio-platform: blocking pool idle wait uses Platform sleep/poll; no worker condvar yet"
        }
        SEMANTIC_GAP_MULTI_THREAD_PARK_POLL => {
            "tokio-platform: multi-thread scheduler condvar fallback uses Platform sleep/poll"
        }
        SEMANTIC_GAP_BARRIER_POLL => {
            "tokio-platform: loom Barrier uses Platform sleep/poll; no kernel barrier wait queue yet"
        }
        TRUEOS_DEBUG_BUILD_DRIVER_NEW => {
            "tokio-platform: debug current_thread build entering Driver::new"
        }
        TRUEOS_DEBUG_BUILD_BLOCKING_POOL => {
            "tokio-platform: debug current_thread build entering blocking::create_blocking_pool"
        }
        TRUEOS_DEBUG_BUILD_CURRENT_THREAD => {
            "tokio-platform: debug current_thread build entering CurrentThread::new"
        }
        TRUEOS_DEBUG_BUILD_CURRENT_THREAD_READY => {
            "tokio-platform: debug current_thread build CurrentThread::new returned"
        }
        _ => "tokio-platform: unknown semantic gap marker",
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_log_semantic_gap(code: u32) {
    if code >= 64 {
        return;
    }

    let mask = 1u64 << code;
    if TOKIO_SEMANTIC_GAPS_LOGGED.fetch_or(mask, Ordering::AcqRel) & mask == 0 {
        crate::log_warn!(target: "tokio"; "{}\n", tokio_semantic_gap_message(code));
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_log(level: u32, bytes: *const u8, len: usize) {
    if bytes.is_null() || len == 0 {
        return;
    }
    let bytes = unsafe { core::slice::from_raw_parts(bytes, len) };
    let text = match core::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => "<non-utf8 tokio log>\n",
    };
    match level {
        TRUEOS_LOG_ERROR => crate::log_error!(target: "tokio"; "{}", text),
        TRUEOS_LOG_WARN => crate::log_warn!(target: "tokio"; "{}", text),
        TRUEOS_LOG_INFO => crate::log_info!(target: "tokio"; "{}", text),
        _ => crate::log_info!(target: "tokio"; "{}", text),
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_platform_monotonic_nanos() -> u64 {
    trueos_platform_monotonic_nanos()
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
