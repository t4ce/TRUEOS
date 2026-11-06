//! TRUEOS runs Rust runtime cleanup after native and C TLS value destructors.
//!
//! The execution owns this callback directly; no synthetic TLS key or deferred
//! destructor rounds are needed just to arrange final thread-handle cleanup.

unsafe extern "Rust" {
    fn trueos_platform_thread_local_register_cleanup(cleanup: unsafe extern "C" fn()) -> i32;
}

pub(crate) fn enable() {
    unsafe extern "C" fn cleanup() { crate::rt::thread_cleanup(); }
    let result = unsafe { trueos_platform_thread_local_register_cleanup(cleanup) };
    rtassert!(result == 0);
}
