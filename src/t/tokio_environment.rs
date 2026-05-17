//! Narrow `std::env` ABI bridge for TRUEOS launch contexts.
//!
//! `getenv` is process-shaped in POSIX, but TRUEOS currently has more precise
//! owners: VMX blueprint/app launch contexts and host worker-lane contexts.
//! `crate::r::io::env` already models that stack, so this bridge only exposes
//! values from the active TRUEOS launch context. With no active owner, the
//! kernel process environment is intentionally empty.

use core::ffi::c_char;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getenv(name: *const c_char) -> *mut c_char {
    unsafe { crate::r::io::env::getenv(name) }
}
