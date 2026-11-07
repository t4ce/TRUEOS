#![no_std]
//! Native TRUEOS execution primitives, independent of C and Unix runtime state.
//!
//! All waits operate on the current logical execution identity. This crate uses
//! panic-abort: thread/task panic recovery requires an unwind runtime and is not
//! part of the native ABI.
#![cfg_attr(not(test), deny(unsafe_op_in_unsafe_fn))]
extern crate alloc;

pub use trueos_io as io;
pub mod env;
pub mod net;
pub mod sync;
pub mod thread;
pub mod time;

pub(crate) mod abi {
    unsafe extern "Rust" {
        pub fn trueos_tokio_platform_wait_observe(key: u64) -> u32;
        pub fn trueos_tokio_platform_wait_after(key: u64, observed: u32, timeout_ms: u64) -> bool;
        pub fn trueos_tokio_platform_wake_one(key: u64) -> bool;
        pub fn trueos_tokio_platform_wake_all(key: u64) -> usize;
        pub fn trueos_platform_monotonic_nanos() -> u64;
        pub fn trueos_platform_thread_local_get(key: usize) -> *mut u8;
        pub fn trueos_platform_thread_local_register_cleanup(
            cleanup: unsafe extern "C" fn(),
        ) -> i32;
        pub fn trueos_platform_thread_local_set(
            key: usize,
            value: *mut u8,
            destructor: Option<unsafe extern "C" fn(*mut u8)>,
        ) -> i32;
    }
}

/// Terminate the current Blueprint through its native lifecycle contract.
/// A kernel that returns from shutdown cannot safely resume a panicked runtime.
pub fn abort() -> ! {
    unsafe extern "C" {
        fn trueos_cabi_blueprint_shutdown(ptr: *const u8, len: usize) -> i32;
    }
    const REASON: &[u8] = b"native runtime aborted";
    unsafe {
        trueos_cabi_blueprint_shutdown(REASON.as_ptr(), REASON.len());
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Obtain a seed from the kernel entropy source, without a C random registry.
pub fn random_seed() -> u64 {
    unsafe extern "C" {
        fn sys_rand(words: *mut u32, len: usize);
    }
    let mut words = [0u32; 2];
    unsafe {
        sys_rand(words.as_mut_ptr(), words.len());
    }
    u64::from(words[0]) | (u64::from(words[1]) << 32)
}
