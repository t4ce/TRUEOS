//! Small C/loader compatibility functions shared by Blueprint REL images.
//!
//! These are deliberately generic: they are not a JavaScript runtime and do
//! not own any Blueprint state.  Keeping them in the kernel avoids making the
//! generic ELF loader depend on an application Blueprint merely for libc-like
//! import addresses.

use core::ffi::{c_char, c_int, c_long, c_void};

#[repr(C)]
pub struct TimeSpec {
    tv_sec: i64,
    tv_nsec: i64,
}

pub unsafe extern "C" fn abort() -> ! {
    unsafe { crate::std_abi_shim::sys_halt() }
}

pub unsafe extern "C" fn __assert_fail(
    _assertion: *const c_char,
    _file: *const c_char,
    _line: c_int,
    _function: *const c_char,
) -> ! {
    unsafe { crate::std_abi_shim::sys_halt() }
}

pub unsafe extern "C" fn malloc_usable_size(ptr: *const c_void) -> usize {
    unsafe { crate::std_abi_shim::trueos_cabi_malloc_usable_size(ptr.cast()) }
}

pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    unsafe { core::ptr::copy_nonoverlapping(src.cast::<u8>(), dest.cast::<u8>(), n) };
    dest
}

pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    unsafe { core::ptr::copy(src.cast::<u8>(), dest.cast::<u8>(), n) };
    dest
}

pub unsafe extern "C" fn memset(dest: *mut c_void, value: c_int, n: usize) -> *mut c_void {
    unsafe { core::ptr::write_bytes(dest.cast::<u8>(), value as u8, n) };
    dest
}

pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    let a = a.cast::<u8>();
    let b = b.cast::<u8>();
    for offset in 0..n {
        let left = unsafe { *a.add(offset) };
        let right = unsafe { *b.add(offset) };
        if left != right {
            return c_int::from(left) - c_int::from(right);
        }
    }
    0
}

pub unsafe extern "C" fn strlen(value: *const c_char) -> usize {
    if value.is_null() {
        return 0;
    }
    let mut length = 0usize;
    while unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    length
}

pub unsafe extern "C" fn lrint(value: f64) -> c_long {
    libm::rint(value) as c_long
}

pub unsafe extern "C" fn modf(value: f64, integer: *mut f64) -> f64 {
    let whole = libm::trunc(value);
    if !integer.is_null() {
        unsafe { integer.write(whole) };
    }
    value - whole
}

pub unsafe extern "C" fn acosh(value: f64) -> f64 {
    libm::acosh(value)
}

pub unsafe extern "C" fn asinh(value: f64) -> f64 {
    libm::asinh(value)
}

pub unsafe extern "C" fn atanh(value: f64) -> f64 {
    libm::atanh(value)
}

pub unsafe extern "C" fn clock_gettime(clock: c_int, out: *mut TimeSpec) -> c_int {
    if out.is_null() {
        return -1;
    }
    let ticks = embassy_time_driver::now();
    let hz = u64::from(embassy_time_driver::TICK_HZ.max(1));
    let elapsed_secs = ticks / hz;
    let elapsed_nanos = (ticks % hz).saturating_mul(1_000_000_000) / hz;
    let seconds = match clock {
        0 => crate::std_abi_shim::trueos_cabi_boot_timestamp_secs().saturating_add(elapsed_secs),
        1 => elapsed_secs,
        _ => return -1,
    };
    unsafe {
        out.write(TimeSpec {
            tv_sec: seconds as i64,
            tv_nsec: elapsed_nanos as i64,
        });
    }
    0
}
