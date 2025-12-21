use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;

pub const FALLBACK_HEAP_SIZE: usize = 256 * 1024;

static mut FALLBACK_HEAP: [u8; FALLBACK_HEAP_SIZE] = [0; FALLBACK_HEAP_SIZE];
static mut NEXT: usize = 0;

struct Allocator;

unsafe impl GlobalAlloc for Allocator {
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

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static GLOBAL_ALLOCATOR: Allocator = Allocator;
