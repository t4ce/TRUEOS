extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, CStr};
use core::fmt::{self, Write};
use core::panic::PanicInfo;

use crate::{vcabi, vsys};

const ABI_ALLOC_ALIGN: usize = 16;
const OVERALIGN_HEADER_BYTES: usize = core::mem::size_of::<usize>();

pub struct TrueosAllocator;

#[inline]
fn is_direct_layout(layout: Layout) -> bool {
    layout.align() <= ABI_ALLOC_ALIGN
}

#[inline]
unsafe fn alloc_direct(size: usize) -> *mut u8 {
    unsafe { vcabi::trueos_cabi_alloc(size) }
}

#[inline]
unsafe fn free_direct(ptr: *mut u8) {
    unsafe { vcabi::trueos_cabi_free(ptr) }
}

#[inline]
unsafe fn realloc_direct(ptr: *mut u8, size: usize) -> *mut u8 {
    unsafe { vcabi::trueos_cabi_realloc(ptr, size) }
}

#[inline]
unsafe fn alloc_overaligned(layout: Layout) -> *mut u8 {
    let align = layout.align();
    let size = layout.size();
    let total = match size
        .checked_add(align)
        .and_then(|v| v.checked_add(OVERALIGN_HEADER_BYTES))
    {
        Some(v) => v,
        None => return core::ptr::null_mut(),
    };
    let base = unsafe { alloc_direct(total) };
    if base.is_null() {
        return core::ptr::null_mut();
    }
    let start = unsafe { base.add(OVERALIGN_HEADER_BYTES) } as usize;
    let aligned = (start + (align - 1)) & !(align - 1);
    let aligned_ptr = aligned as *mut u8;
    unsafe {
        (aligned_ptr as *mut usize).sub(1).write(base as usize);
    }
    aligned_ptr
}

#[inline]
unsafe fn free_overaligned(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let base = unsafe { (ptr as *mut usize).sub(1).read() as *mut u8 };
    unsafe { free_direct(base) };
}

unsafe impl GlobalAlloc for TrueosAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::null_mut();
        }
        if is_direct_layout(layout) {
            unsafe { alloc_direct(layout.size()) }
        } else {
            unsafe { alloc_overaligned(layout) }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }
        if is_direct_layout(layout) {
            unsafe { free_direct(ptr) };
        } else {
            unsafe { free_overaligned(ptr) };
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ptr.is_null() {
            let new_layout = match Layout::from_size_align(new_size, layout.align()) {
                Ok(v) => v,
                Err(_) => return core::ptr::null_mut(),
            };
            return unsafe { self.alloc(new_layout) };
        }
        if is_direct_layout(layout) {
            return unsafe { realloc_direct(ptr, new_size) };
        }

        let new_layout = match Layout::from_size_align(new_size, layout.align()) {
            Ok(v) => v,
            Err(_) => return core::ptr::null_mut(),
        };
        let new_ptr = unsafe { self.alloc(new_layout) };
        if new_ptr.is_null() {
            return core::ptr::null_mut();
        }
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(layout.size(), new_size));
            self.dealloc(ptr, layout);
        }
        new_ptr
    }
}

struct PanicWriter;

impl Write for PanicWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        vsys::write_stream(2, s.as_bytes());
        Ok(())
    }
}

pub fn panic_handler(info: &PanicInfo<'_>) -> ! {
    let _ = writeln!(PanicWriter, "panic: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

pub unsafe fn args_from_abi<'a>(argc: usize, argv: *const *const c_char) -> &'a [&'a str] {
    if argc == 0 || argv.is_null() {
        return &[];
    }

    let ptrs = unsafe { core::slice::from_raw_parts(argv, argc) };
    let mut strings = alloc::vec::Vec::with_capacity(argc);

    for &ptr in ptrs {
        if ptr.is_null() {
            strings.push("");
            continue;
        }

        let arg = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("");
        strings.push(arg);
    }

    alloc::boxed::Box::leak(strings.into_boxed_slice())
}
