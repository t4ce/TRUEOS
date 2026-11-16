//! Native worker Rust ABI. Both sides must use the pinned TRUEOS toolchain.
//! 0 accepts and owns the closure. A nonzero result consumes it without
//! executing it: -2 unavailable/closing, -5 invalid raw job, -6 transport.
//! Guest closures are dropped in their guest allocation realm; if that realm
//! has already disappeared, the kernel retains the raw closure for teardown.
//! The capacity query is advisory and may return zero.

use alloc::boxed::Box;

pub const CORE_PREFERENCE_ANY: u32 = 0;
pub const CORE_PREFERENCE_PREFER_PERFORMANCE: u32 = 1;

/// Packed placement: logical CPU slot in bits 0..31, registered core kind in
/// bits 32..39 (0 unknown, 1 performance, 2 efficiency). All other bits are zero.
/// `u64::MAX` means that placement could not be obtained.
pub const PLACEMENT_UNAVAILABLE: u64 = u64::MAX;

unsafe extern "Rust" {
    pub fn trueos_service_lane_submit_job(job: Box<dyn FnOnce() + Send + 'static>) -> i32;
    pub fn trueos_service_lane_submit_job_with_preference(
        job: Box<dyn FnOnce() + Send + 'static>,
        preference: u32,
    ) -> i32;
    pub fn trueos_service_lane_available_capacity() -> usize;
    pub fn trueos_service_lane_cancellation_requested() -> bool;
    pub fn trueos_service_lane_current_placement() -> u64;
}
