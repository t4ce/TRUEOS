//! Native worker Rust ABI. Both sides must use the pinned TRUEOS toolchain.
//! 0 accepts and owns the closure. A nonzero result consumes it without
//! executing it: -2 unavailable/closing, -5 invalid raw job, -6 transport.
//! Guest closures are dropped in their guest allocation realm; if that realm
//! has already disappeared, the kernel retains the raw closure for teardown.
//! The capacity query is advisory and may return zero.

use alloc::boxed::Box;

unsafe extern "Rust" {
    pub fn trueos_service_lane_submit_job(job: Box<dyn FnOnce() + Send + 'static>) -> i32;
    pub fn trueos_service_lane_available_capacity() -> usize;
    pub fn trueos_service_lane_cancellation_requested() -> bool;
    pub fn trueos_guest_compute_submit_job(job: Box<dyn FnMut() -> bool + Send + 'static>) -> i32;
    pub fn trueos_guest_compute_capacity() -> usize;
}
