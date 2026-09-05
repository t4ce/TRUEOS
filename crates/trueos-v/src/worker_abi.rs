//! Native worker Rust ABI. Both sides must use the pinned TRUEOS toolchain.
//! 0 accepts and owns the closure. A nonzero result consumes/drops it without
//! executing it: -2 unavailable/closing, -5 invalid raw job, -6 transport.
//! The capacity query is advisory and may return zero.

use alloc::boxed::Box;

unsafe extern "Rust" {
    pub fn trueos_service_lane_submit_job(job: Box<dyn FnOnce() + Send + 'static>) -> i32;
    pub fn trueos_service_lane_available_capacity() -> usize;
}
