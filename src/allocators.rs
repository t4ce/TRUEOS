use alloc::alloc::alloc;
use core::alloc::{GlobalAlloc, Layout};
use core::mem::{align_of, size_of};
use core::ptr::{addr_of_mut, null_mut, NonNull};
use spin::Mutex;

use crate::debugconf;

pub const FALLBACK_HEAP_SIZE: usize = 256 * 1024;

static mut FALLBACK_HEAP: [u8; FALLBACK_HEAP_SIZE] = [0; FALLBACK_HEAP_SIZE];

#[repr(C)]
struct FreeBlock {
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct AllocTag {
    block_start: usize,
    block_size: usize,
}

struct FreeList {
    head: Option<NonNull<FreeBlock>>,
    initialized: bool,
}

unsafe impl Send for FreeList {}

impl FreeList {
    const fn new() -> Self {
        Self {
            head: None,
            initialized: false,
        }
    }

    unsafe fn init_once(&mut self) {
        if self.initialized {
            return;
        }

        let heap_start = FALLBACK_HEAP.as_ptr() as usize;
        let heap_end = heap_start + FALLBACK_HEAP_SIZE;

        let block_start = align_up(heap_start, align_of::<FreeBlock>());
        if block_start >= heap_end {
            return;
        }

        let size = heap_end - block_start;
        if size < minimum_block_size() {
            return;
        }

        let block = block_start as *mut FreeBlock;
        block.write(FreeBlock { size, next: None });
        self.head = Some(NonNull::new_unchecked(block));
        self.initialized = true;
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        if !self.initialized {
            self.init_once();
        }

        let mut current = self.head;
        let mut prev: Option<NonNull<FreeBlock>> = None;

        while let Some(mut block_ptr) = current {
            let block = block_ptr.as_mut();

            let block_start = block as *mut FreeBlock as usize;
            let block_end = block_start + block.size;

            let payload_start = match aligned_payload(block_start, layout) {
                Some(v) => v,
                None => {
                    prev = Some(block_ptr);
                    current = block.next;
                    continue;
                }
            };

            let total_used = match payload_start
                .checked_add(layout.size())
                .and_then(|end| end.checked_sub(block_start))
            {
                Some(v) => v,
                None => {
                    prev = Some(block_ptr);
                    current = block.next;
                    continue;
                }
            };

            let mut remaining = block.size.saturating_sub(total_used);

            if total_used > block.size {
                prev = Some(block_ptr);
                current = block.next;
                continue;
            }

            // Split tail if we have room for another free block.
            let next_block = if remaining >= minimum_block_size() {
                let next_start = block_start + total_used;
                let next_ptr = next_start as *mut FreeBlock;
                next_ptr.write(FreeBlock {
                    size: remaining,
                    next: block.next,
                });
                Some(NonNull::new_unchecked(next_ptr))
            } else {
                // Hand out the whole block to avoid leaking tiny fragments.
                remaining = 0;
                block.next
            };
            let alloc_block_size = if remaining == 0 { block.size } else { total_used };
            block.size = alloc_block_size;

            // Remove current from free list.
            match prev {
                Some(mut p) => p.as_mut().next = next_block,
                None => self.head = next_block,
            }

            let tag_ptr = payload_start - size_of::<AllocTag>();
            (tag_ptr as *mut AllocTag).write(AllocTag {
                block_start,
                block_size: alloc_block_size,
            });

            return payload_start as *mut u8;
        }

        null_mut()
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }

        let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
        let tag = *tag_ptr;
        let block_size = tag.block_size;
        let block_start = tag.block_start;
        let mut block_ptr = block_start as *mut FreeBlock;
        block_ptr.write(FreeBlock { size: block_size, next: None });

        // Insert sorted by address.
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut current = self.head;

        while let Some(mut node) = current {
            if (node.as_ptr() as usize) > block_start {
                break;
            }
            prev = current;
            current = node.as_ref().next;
        }

        let mut new_node = NonNull::new_unchecked(block_ptr);
        {
            let new_block = new_node.as_mut();
            new_block.next = current;
        }

        if let Some(mut p) = prev {
            p.as_mut().next = Some(new_node);
        } else {
            self.head = Some(new_node);
        }

        // Coalesce with next.
        self.try_merge_with_next(new_node);

        // Coalesce with previous if adjacent.
        if let Some(mut p) = prev {
            self.try_merge_with_next(p);
        }
    }

    unsafe fn try_merge_with_next(&mut self, mut node: NonNull<FreeBlock>) {
        let node_size = node.as_ref().size;
        let node_end = (node.as_ptr() as usize).saturating_add(node_size);

        if let Some(mut next_ptr) = node.as_ref().next {
            let next_start = next_ptr.as_ptr() as usize;
            if node_end == next_start {
                let next_size = next_ptr.as_ref().size;
                let next_next = next_ptr.as_ref().next;
                let new_size = node_size + next_size;
                let node_mut = node.as_mut();
                node_mut.size = new_size;
                node_mut.next = next_next;
            }
        }
    }
}

struct Allocator;

static ALLOCATOR: Mutex<FreeList> = Mutex::new(FreeList::new());

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATOR.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        ALLOCATOR.lock().dealloc(ptr)
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: Allocator = Allocator;

pub fn alloc_demo() {
    let layout = Layout::from_size_align(512, 1).unwrap();
    let ptr = unsafe { alloc(layout) };
    if ptr.is_null() {
        debugconf!("alloc demo: failed\n");
        return;
    }
    unsafe {
        core::ptr::write(ptr, 0xFFu8);
    }
    let first = unsafe { core::ptr::read(ptr) };
    debugconf!("alloc demo: ptr=0x{:X} first={:02X}\n", ptr as usize, first);
}

const fn minimum_block_size() -> usize {
    size_of::<FreeBlock>() + size_of::<AllocTag>()
}

fn align_up(addr: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (addr + mask) & !mask
}

fn aligned_payload(block_start: usize, layout: Layout) -> Option<usize> {
    let payload_start = align_up(block_start + size_of::<FreeBlock>() + size_of::<AllocTag>(), layout.align());
    if payload_start > usize::MAX - layout.size() {
        None
    } else {
        Some(payload_start)
    }
}
