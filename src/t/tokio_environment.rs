//! Narrow `std::env` ABI bridge for TRUEOS launch contexts.
//!
//! `getenv` is process-shaped in POSIX, but TRUEOS currently has more precise
//! owners: VMX blueprint/app launch contexts and host worker-lane contexts.
//! `crate::r::io::env` already models that stack, so this bridge only exposes
//! values from the active TRUEOS launch context. With no active owner, the
//! kernel process environment is intentionally empty.

extern crate alloc;

use alloc::vec::Vec;
use core::{ffi::c_char, ptr, slice, str};

unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }

    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len = len.saturating_add(1);
    }

    str::from_utf8(unsafe { slice::from_raw_parts(ptr.cast::<u8>(), len) }).ok()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn getenv(name: *const c_char) -> *mut c_char {
    let Some(key) = (unsafe { cstr_to_str(name) }) else {
        return ptr::null_mut();
    };

    let Some(value) = crate::r::io::env::var(key) else {
        return ptr::null_mut();
    };

    let mut bytes = Vec::with_capacity(value.len().saturating_add(1));
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);

    let ptr = bytes.as_mut_ptr();
    core::mem::forget(bytes);
    ptr.cast::<c_char>()
}
