use alloc::alloc::alloc;
use core::alloc::{GlobalAlloc, Layout};
use core::mem::{align_of, size_of};
use core::ptr::{addr_of, addr_of_mut, null_mut, NonNull};
use spin::Mutex;

use crate::{
    debugconf,
    phys::{self, HeapArena},
};

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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HeapSourceKind {
    Fallback,
    Arena,
}

struct FreeList {
    head: Option<NonNull<FreeBlock>>,
    initialized: bool,
    heap_virt_start: usize,
    heap_len: usize,
    heap_phys_start: usize,
    heap_source: HeapSourceKind,
}

unsafe impl Send for FreeList {}

impl FreeList {
    const fn new() -> Self {
        Self {
            head: None,
            initialized: false,
            heap_virt_start: 0,
            heap_len: 0,
            heap_phys_start: 0,
            heap_source: HeapSourceKind::Fallback,
        }
    }

    unsafe fn init_once(&mut self) {
        if self.initialized {
            return;
        }

        let (heap_start, heap_len) = self.ensure_heap_backing();
        let heap_end = heap_start + heap_len;

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

            // If we split, the next free-list node must be properly aligned for `FreeBlock`.
            // This padding is accounted to the allocated block size.
            let aligned_used = align_up(total_used, align_of::<FreeBlock>());

            if aligned_used > block.size {
                prev = Some(block_ptr);
                current = block.next;
                continue;
            }

            let mut remaining = block.size.saturating_sub(aligned_used);

            let next_block = if remaining >= minimum_block_size() {
                let next_start = block_start + aligned_used;
                let next_ptr = next_start as *mut FreeBlock;
                next_ptr.write(FreeBlock {
                    size: remaining,
                    next: block.next,
                });
                Some(NonNull::new_unchecked(next_ptr))
            } else {
                remaining = 0;
                block.next
            };
            let alloc_block_size = if remaining == 0 { block.size } else { aligned_used };
            block.size = alloc_block_size;

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
        let block_ptr = block_start as *mut FreeBlock;
        block_ptr.write(FreeBlock { size: block_size, next: None });

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

        self.try_merge_with_next(new_node);

        if let Some(p) = prev {
            self.try_merge_with_next(p);
        }
    }

    fn install_heap(&mut self, virt_start: usize, phys_start: usize, len: usize) {
        self.heap_virt_start = virt_start;
        self.heap_len = len;
        self.heap_phys_start = phys_start;
        self.heap_source = HeapSourceKind::Arena;
    }

    fn ensure_heap_backing(&mut self) -> (usize, usize) {
        if self.heap_len == 0 {
            let start = unsafe { addr_of_mut!(FALLBACK_HEAP) as *mut u8 as usize };
            self.heap_virt_start = start;
            self.heap_len = FALLBACK_HEAP_SIZE;
            self.heap_phys_start = 0;
            self.heap_source = HeapSourceKind::Fallback;
        }
        (self.heap_virt_start, self.heap_len)
    }

    unsafe fn try_merge_with_next(&mut self, mut node: NonNull<FreeBlock>) {
        let node_size = node.as_ref().size;
        let node_end = (node.as_ptr() as usize).saturating_add(node_size);

        if let Some(next_ptr) = node.as_ref().next {
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

#[derive(Copy, Clone, Debug)]
pub struct HeapStats {
    pub heap_start: usize,
    pub heap_end: usize,
    pub phys_start: usize,
    pub usable_start: usize,
    pub usable_total: usize,
    pub free_bytes: usize,
    pub largest_free_block: usize,
    pub free_blocks: usize,
    pub initialized: bool,
    pub source: HeapSourceKind,
}

pub fn heap_stats() -> HeapStats {
    let mut guard = ALLOCATOR.lock();
    unsafe {
        if !guard.initialized {
            guard.init_once();
        }
    }

    let (heap_start, heap_len) = guard.ensure_heap_backing();
    let heap_end = heap_start.saturating_add(heap_len);
    let usable_start = align_up(heap_start, align_of::<FreeBlock>());
    let usable_total = heap_end.saturating_sub(usable_start);

    let mut free_bytes = 0usize;
    let mut largest_free_block = 0usize;
    let mut free_blocks = 0usize;
    let mut current = guard.head;
    while let Some(block_ptr) = current {
        // Safety: free list nodes are managed by the allocator.
        let block = unsafe { block_ptr.as_ref() };
        free_blocks += 1;
        free_bytes = free_bytes.saturating_add(block.size);
        if block.size > largest_free_block {
            largest_free_block = block.size;
        }
        current = block.next;
    }

    HeapStats {
        heap_start,
        heap_end,
        phys_start: guard.heap_phys_start,
        usable_start,
        usable_total,
        free_bytes,
        largest_free_block,
        free_blocks,
        initialized: guard.initialized,
        source: guard.heap_source,
    }
}

pub fn install_heap_arena(arena: HeapArena) -> bool {
    if arena.length < minimum_block_size() {
        debugconf!(
            "heap: requested arena too small size={} bytes (need >= {})\n",
            arena.length,
            minimum_block_size()
        );
        return false;
    }

    let mut guard = ALLOCATOR.lock();
    if guard.initialized {
        debugconf!("heap: allocator already initialized; cannot swap backing\n");
        return false;
    }

    guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    phys::register_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    debugconf!(
        "heap: arena virt=0x{:X} phys=0x{:X} size={} MiB\n",
        arena.virt_start,
        arena.phys_start,
        arena.length / (1024 * 1024)
    );
    true
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
