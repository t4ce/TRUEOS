use alloc::alloc::{alloc, dealloc};
use core::alloc::Layout;
use core::cmp;
use core::ffi::{c_char, c_int, c_void};
use core::mem::{align_of, size_of};
use core::ptr;

#[repr(C)]
#[derive(Copy, Clone)]
struct AllocTag {
    block_start: usize,
    block_size: usize,
}

#[inline]
unsafe fn usable_size(ptr: *const u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *const AllocTag;
    let tag = *tag_ptr;
    let offset = (ptr as usize).saturating_sub(tag.block_start);
    tag.block_size.saturating_sub(offset)
}

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    if size == 0 {
        return ptr::null_mut();
    }
    let layout = Layout::from_size_align_unchecked(size, align_of::<usize>());
    alloc(layout) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    let total = nmemb.saturating_mul(size);
    if total == 0 {
        return ptr::null_mut();
    }
    let layout = Layout::from_size_align_unchecked(total, align_of::<usize>());
    let ptr = alloc(layout);
    if !ptr.is_null() {
        ptr::write_bytes(ptr, 0, total);
    }
    ptr as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let usable = usable_size(ptr as *const u8);
    let layout = Layout::from_size_align_unchecked(usable.max(1), align_of::<usize>());
    dealloc(ptr as *mut u8, layout);
}

#[no_mangle]
pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    if ptr.is_null() {
        return malloc(size);
    }
    if size == 0 {
        free(ptr);
        return ptr::null_mut();
    }

    let old_ptr = ptr as *mut u8;
    let old_usable = usable_size(old_ptr);

    let layout = Layout::from_size_align_unchecked(size, align_of::<usize>());
    let new_ptr = alloc(layout);
    if new_ptr.is_null() {
        return ptr::null_mut();
    }

    let copy_len = cmp::min(old_usable, size);
    ptr::copy_nonoverlapping(old_ptr, new_ptr, copy_len);
    free(ptr);
    new_ptr as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn malloc_usable_size(ptr: *const c_void) -> usize {
    usable_size(ptr as *const u8)
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    ptr::copy_nonoverlapping(src as *const u8, dest as *mut u8, n);
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    ptr::copy(src as *const u8, dest as *mut u8, n);
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    ptr::write_bytes(s as *mut u8, c as u8, n);
    s
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    let a = a as *const u8;
    let b = b as *const u8;
    for i in 0..n {
        let av = *a.add(i);
        let bv = *b.add(i);
        if av != bv {
            return av as c_int - bv as c_int;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void {
    let s = s as *const u8;
    let needle = c as u8;
    for i in 0..n {
        if *s.add(i) == needle {
            return s.add(i) as *mut c_void;
        }
    }
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const c_char) -> usize {
    if s.is_null() {
        return 0;
    }
    let mut len = 0usize;
    while *s.add(len) != 0 {
        len += 1;
    }
    len
}

#[no_mangle]
pub unsafe extern "C" fn strcmp(a: *const c_char, b: *const c_char) -> c_int {
    let mut i = 0usize;
    loop {
        let av = *a.add(i) as u8;
        let bv = *b.add(i) as u8;
        if av != bv {
            return av as c_int - bv as c_int;
        }
        if av == 0 {
            return 0;
        }
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn strncmp(a: *const c_char, b: *const c_char, n: usize) -> c_int {
    let mut i = 0usize;
    while i < n {
        let av = *a.add(i) as u8;
        let bv = *b.add(i) as u8;
        if av != bv {
            return av as c_int - bv as c_int;
        }
        if av == 0 {
            return 0;
        }
        i += 1;
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    let mut i = 0usize;
    let needle = c as u8;
    loop {
        let v = *s.add(i) as u8;
        if v == needle {
            return s.add(i) as *mut c_char;
        }
        if v == 0 {
            return ptr::null_mut();
        }
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn strrchr(s: *const c_char, c: c_int) -> *mut c_char {
    let mut last: *mut c_char = ptr::null_mut();
    let mut i = 0usize;
    let needle = c as u8;
    loop {
        let v = *s.add(i) as u8;
        if v == needle {
            last = s.add(i) as *mut c_char;
        }
        if v == 0 {
            return last;
        }
        i += 1;
    }
}
