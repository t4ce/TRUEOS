#![allow(dead_code)]

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;

/// Default fallback heap size used when no Limine-provided region is usable.
pub const FALLBACK_HEAP_SIZE: usize = 256 * 1024;

static mut FALLBACK_HEAP: [u8; FALLBACK_HEAP_SIZE] = [0; FALLBACK_HEAP_SIZE];
static mut NEXT: usize = 0;

struct FallbackAllocator;

unsafe impl GlobalAlloc for FallbackAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align_mask = layout.align().saturating_sub(1);
        let mut start = NEXT;
        start = (start + align_mask) & !align_mask;
        let end = match start.checked_add(layout.size()) {
            Some(v) => v,
            None => return null_mut(),
        };
        if end > FALLBACK_HEAP_SIZE {
            return null_mut();
        }
        NEXT = end;
        unsafe { FALLBACK_HEAP.as_mut_ptr().add(start) }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator: free is a no-op.
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: FallbackAllocator = FallbackAllocator;

/// Initialise the heap; no-op for the simple bump allocator.
pub fn init_linked_list_heap(_start_virt: usize, _length: usize) {}

/// Expose the fallback heap span for compatibility with callers.
pub fn fallback_heap_span() -> (*mut u8, usize) {
    let ptr = unsafe { core::ptr::addr_of_mut!(FALLBACK_HEAP).cast::<u8>() };
    (ptr, FALLBACK_HEAP_SIZE)
}
