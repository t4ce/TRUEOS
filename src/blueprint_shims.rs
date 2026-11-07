//! Small C/loader compatibility functions shared by Blueprint REL images.
//!
//! These are deliberately generic: they are not a JavaScript runtime and do
//! not own any Blueprint state.  Keeping them in the kernel avoids making the
//! generic ELF loader depend on an application Blueprint merely for libc-like
//! import addresses.

use alloc::vec::Vec;
use core::ffi::{c_char, c_double, c_int, c_long, c_void};
use core::{ptr, slice};

pub(crate) mod ctype;

pub unsafe extern "C" fn abort() -> ! {
    unsafe { crate::native_abi::sys_halt() }
}

pub unsafe extern "C" fn __assert_fail(
    _assertion: *const c_char,
    _file: *const c_char,
    _line: c_int,
    _function: *const c_char,
) -> ! {
    unsafe { crate::native_abi::sys_halt() }
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

pub unsafe extern "C" fn __memcpy_chk(
    dest: *mut c_void,
    src: *const c_void,
    len: usize,
    _dest_len: usize,
) -> *mut c_void {
    unsafe { ptr::copy_nonoverlapping(src.cast::<u8>(), dest.cast::<u8>(), len) };
    dest
}

pub unsafe extern "C" fn __memmove_chk(
    dest: *mut c_void,
    src: *const c_void,
    len: usize,
    _dest_len: usize,
) -> *mut c_void {
    unsafe { ptr::copy(src.cast::<u8>(), dest.cast::<u8>(), len) };
    dest
}

pub unsafe extern "C" fn __memset_chk(
    dest: *mut c_void,
    value: c_int,
    len: usize,
    _dest_len: usize,
) -> *mut c_void {
    unsafe { ptr::write_bytes(dest.cast::<u8>(), value as u8, len) };
    dest
}

pub unsafe extern "C" fn memchr(ptr: *const c_void, value: c_int, len: usize) -> *mut c_void {
    if ptr.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { slice::from_raw_parts(ptr.cast::<u8>(), len) };
    let needle = value as u8;
    bytes
        .iter()
        .position(|byte| *byte == needle)
        .map(|offset| unsafe { ptr.cast::<u8>().add(offset).cast_mut().cast::<c_void>() })
        .unwrap_or(ptr::null_mut())
}

pub unsafe extern "C" fn strcmp(left: *const c_char, right: *const c_char) -> c_int {
    unsafe { strncmp(left, right, usize::MAX) }
}

pub unsafe extern "C" fn strncmp(left: *const c_char, right: *const c_char, max: usize) -> c_int {
    if left.is_null() || right.is_null() {
        return if left == right { 0 } else { -1 };
    }
    let mut index = 0usize;
    while index < max {
        let a = unsafe { *left.cast::<u8>().add(index) };
        let b = unsafe { *right.cast::<u8>().add(index) };
        if a != b || a == 0 || b == 0 {
            return a as c_int - b as c_int;
        }
        index = index.saturating_add(1);
    }
    0
}

pub unsafe extern "C" fn strchr(text: *const c_char, needle: c_int) -> *mut c_char {
    if text.is_null() {
        return ptr::null_mut();
    }
    let needle = needle as u8;
    let mut index = 0usize;
    loop {
        let byte = unsafe { *text.cast::<u8>().add(index) };
        if byte == needle {
            return unsafe { text.add(index).cast_mut() };
        }
        if byte == 0 {
            return ptr::null_mut();
        }
        index = index.saturating_add(1);
    }
}

pub unsafe extern "C" fn strrchr(text: *const c_char, needle: c_int) -> *mut c_char {
    if text.is_null() {
        return ptr::null_mut();
    }
    let needle = needle as u8;
    let mut last = ptr::null_mut();
    let mut index = 0usize;
    loop {
        let byte = unsafe { *text.cast::<u8>().add(index) };
        if byte == needle {
            last = unsafe { text.add(index).cast_mut() };
        }
        if byte == 0 {
            return last;
        }
        index = index.saturating_add(1);
    }
}

pub unsafe extern "C" fn strspn(text: *const c_char, accept: *const c_char) -> usize {
    unsafe { str_span(text, accept, true) }
}

pub unsafe extern "C" fn strcspn(text: *const c_char, reject: *const c_char) -> usize {
    unsafe { str_span(text, reject, false) }
}

unsafe fn str_span(text: *const c_char, set: *const c_char, accept_mode: bool) -> usize {
    if text.is_null() || set.is_null() {
        return 0;
    }
    let mut len = 0usize;
    loop {
        let byte = unsafe { *text.cast::<u8>().add(len) };
        if byte == 0 {
            return len;
        }
        let contains = unsafe { cstr_contains_byte(set, byte) };
        if contains != accept_mode {
            return len;
        }
        len = len.saturating_add(1);
    }
}

unsafe fn cstr_contains_byte(text: *const c_char, needle: u8) -> bool {
    let mut index = 0usize;
    loop {
        let byte = unsafe { *text.cast::<u8>().add(index) };
        if byte == 0 {
            return false;
        }
        if byte == needle {
            return true;
        }
        index = index.saturating_add(1);
    }
}

pub unsafe extern "C" fn abs(value: c_int) -> c_int {
    value.wrapping_abs()
}

pub unsafe extern "C" fn ldexp(value: c_double, exponent: c_int) -> c_double {
    libm::ldexp(value, exponent)
}

pub unsafe extern "C" fn qsort(
    base: *mut c_void,
    count: usize,
    size: usize,
    compar: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>,
) {
    if base.is_null() || size == 0 || count < 2 {
        return;
    }
    let Some(compare) = compar else {
        return;
    };
    let mut scratch = Vec::new();
    scratch.resize(size, 0);
    let base = base.cast::<u8>();
    for _ in 0..count {
        let mut swapped = false;
        for index in 1..count {
            let left = unsafe { base.add((index - 1).saturating_mul(size)) };
            let right = unsafe { base.add(index.saturating_mul(size)) };
            if unsafe { compare(left.cast::<c_void>(), right.cast::<c_void>()) } > 0 {
                unsafe {
                    ptr::copy_nonoverlapping(left, scratch.as_mut_ptr(), size);
                    ptr::copy(right, left, size);
                    ptr::copy_nonoverlapping(scratch.as_ptr(), right, size);
                }
                swapped = true;
            }
        }
        if !swapped {
            break;
        }
    }
}
