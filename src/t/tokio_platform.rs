//! Rust ABI hooks consumed by the vendored Tokio TRUEOS platform layer.

use core::sync::atomic::{AtomicU64, Ordering};

static SEMANTIC_GAPS_LOGGED: AtomicU64 = AtomicU64::new(0);

const SEMANTIC_GAP_MUTEX_SPIN: u32 = 1;
const SEMANTIC_GAP_RUNTIME_PARK_POLL: u32 = 2;
const SEMANTIC_GAP_BLOCKING_POOL_POLL: u32 = 3;
const SEMANTIC_GAP_MULTI_THREAD_PARK_POLL: u32 = 4;
const SEMANTIC_GAP_BARRIER_POLL: u32 = 5;
const TRUEOS_DEBUG_BUILD_DRIVER_NEW: u32 = 6;
const TRUEOS_DEBUG_BUILD_BLOCKING_POOL: u32 = 7;
const TRUEOS_DEBUG_BUILD_CURRENT_THREAD: u32 = 8;
const TRUEOS_DEBUG_BUILD_CURRENT_THREAD_READY: u32 = 9;

fn semantic_gap_message(code: u32) -> &'static str {
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
    if SEMANTIC_GAPS_LOGGED.fetch_or(mask, Ordering::AcqRel) & mask == 0 {
        crate::log_warn!(target: "tokio"; "{}\n", semantic_gap_message(code));
    }
}

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
