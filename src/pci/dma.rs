use core::{mem, ptr::NonNull, sync::atomic::{AtomicBool, AtomicU64, Ordering}};

use spin::Mutex;

const MIN_DMA_BASE: u64 = 64 * 1024;
const DMA_MAX_PHYS: u64 = 0x1_0000_0000; // stay under 4 GiB for legacy devices

const DMA_POOL_LOW_SIZE: usize = 16 * 1024 * 1024;
const DMA_POOL_ANY_SIZE: usize = 8 * 1024 * 1024;
const DMA_POOL_ALIGN: usize = 4096;

const MIN_BLOCK_ALIGN: usize = 16;
const HEADER_SIZE: usize = MIN_BLOCK_ALIGN;

static DMA_READY: AtomicBool = AtomicBool::new(false);

static DMA_LOW_POOL: Mutex<Option<DmaPool>> = Mutex::new(None);
static DMA_ANY_POOL: Mutex<Option<DmaPool>> = Mutex::new(None);

static POOL_ALLOCS: AtomicU64 = AtomicU64::new(0);
static POOL_FREES: AtomicU64 = AtomicU64::new(0);
static POOL_BYTES: AtomicU64 = AtomicU64::new(0);
static PMM_ALLOCS: AtomicU64 = AtomicU64::new(0);
static PMM_FREES: AtomicU64 = AtomicU64::new(0);
static PMM_BYTES: AtomicU64 = AtomicU64::new(0);

#[repr(C)]
struct FreeBlock {
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

#[repr(C)]
struct AllocHeader {
    size: usize,
}

struct DmaPool {
    phys_base: u64,
    virt_base: *mut u8,
    size: usize,
    head: Option<NonNull<FreeBlock>>,
    bytes_used: usize,
}

// Safety: DMA pools manage stable physical memory and are guarded by a Mutex.
unsafe impl Send for DmaPool {}
unsafe impl Sync for DmaPool {}

impl DmaPool {
    fn new(phys_base: u64, virt_base: *mut u8, size: usize) -> Self {
        let size = align_down(size, MIN_BLOCK_ALIGN);
        let mut pool = Self {
            phys_base,
            virt_base,
            size,
            head: None,
            bytes_used: 0,
        };

        if size >= mem::size_of::<FreeBlock>() {
            unsafe {
                let block = virt_base as *mut FreeBlock;
                (*block).size = size;
                (*block).next = None;
                pool.head = Some(NonNull::new_unchecked(block));
            }
        }

        pool
    }

    fn contains(&self, ptr: *mut u8) -> bool {
        let start = self.virt_base as usize;
        let end = start.saturating_add(self.size);
        let addr = ptr as usize;
        addr >= start && addr < end
    }

    fn alloc(&mut self, size: usize, align: usize) -> Option<(u64, *mut u8)> {
        if size == 0 {
            return None;
        }

        let align = align.max(MIN_BLOCK_ALIGN);
        let size = align_up(size, MIN_BLOCK_ALIGN);

        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut cur = self.head;
        while let Some(block_ptr) = cur {
            unsafe {
                let block = block_ptr.as_ref();
                let block_start = block_ptr.as_ptr() as usize;
                let block_end = block_start.saturating_add(block.size);

                let user_ptr = align_up(block_start.saturating_add(HEADER_SIZE), align);
                let header_ptr = user_ptr.saturating_sub(HEADER_SIZE);
                let alloc_end = align_up(user_ptr.saturating_add(size), MIN_BLOCK_ALIGN);

                if header_ptr < block_start || alloc_end > block_end {
                    prev = cur;
                    cur = block.next;
                    continue;
                }

                let prefix_size = header_ptr.saturating_sub(block_start);
                let suffix_size = block_end.saturating_sub(alloc_end);
                let next = block.next;
                let mut new_head = next;

                if suffix_size >= mem::size_of::<FreeBlock>() {
                    let suffix_ptr = alloc_end as *mut FreeBlock;
                    (*suffix_ptr).size = suffix_size;
                    (*suffix_ptr).next = new_head;
                    new_head = Some(NonNull::new_unchecked(suffix_ptr));
                }

                if prefix_size >= mem::size_of::<FreeBlock>() {
                    let prefix_ptr = block_ptr.as_ptr();
                    (*prefix_ptr).size = prefix_size;
                    (*prefix_ptr).next = new_head;
                    new_head = Some(NonNull::new_unchecked(prefix_ptr));
                }

                if let Some(prev_ptr) = prev {
                    (*prev_ptr.as_ptr()).next = new_head;
                } else {
                    self.head = new_head;
                }

                let header = header_ptr as *mut AllocHeader;
                (*header).size = alloc_end.saturating_sub(header_ptr);
                self.bytes_used = self.bytes_used.saturating_add(alloc_end.saturating_sub(header_ptr));

                let phys = self.phys_base.saturating_add((user_ptr - self.virt_base as usize) as u64);
                return Some((phys, user_ptr as *mut u8));
            }
        }

        None
    }

    fn dealloc(&mut self, ptr: *mut u8) -> bool {
        if ptr.is_null() {
            return false;
        }

        let start = ptr as usize;
        let pool_start = self.virt_base as usize;
        let pool_end = pool_start.saturating_add(self.size);
        if start < pool_start.saturating_add(HEADER_SIZE) || start >= pool_end {
            return false;
        }

        let header_ptr = start.saturating_sub(HEADER_SIZE) as *mut AllocHeader;
        let size = unsafe { (*header_ptr).size };
        if size == 0 {
            return false;
        }

        let block_start = header_ptr as usize;
        let block_end = block_start.saturating_add(size);
        if block_start < pool_start || block_end > pool_end {
            return false;
        }

        unsafe {
            let new_block = block_start as *mut FreeBlock;
            (*new_block).size = size;
            (*new_block).next = None;

            let mut prev: Option<NonNull<FreeBlock>> = None;
            let mut cur = self.head;
            while let Some(block_ptr) = cur {
                if block_ptr.as_ptr() as usize > block_start {
                    break;
                }
                prev = cur;
                cur = block_ptr.as_ref().next;
            }

            (*new_block).next = cur;
            let new_nn = Some(NonNull::new_unchecked(new_block));
            if let Some(prev_ptr) = prev {
                (*prev_ptr.as_ptr()).next = new_nn;
            } else {
                self.head = new_nn;
            }

            self.coalesce(prev, new_nn.unwrap());
        }

        self.bytes_used = self.bytes_used.saturating_sub(size);
        true
    }

    unsafe fn coalesce(&mut self, prev: Option<NonNull<FreeBlock>>, mut curr: NonNull<FreeBlock>) {
        if let Some(next) = curr.as_ref().next {
            let curr_end = curr.as_ptr() as usize + curr.as_ref().size;
            let next_start = next.as_ptr() as usize;
            if curr_end == next_start {
                let next_size = next.as_ref().size;
                let next_next = next.as_ref().next;
                curr.as_mut().size += next_size;
                curr.as_mut().next = next_next;
            }
        }

        if let Some(mut prev_ptr) = prev {
            let prev_end = prev_ptr.as_ptr() as usize + prev_ptr.as_ref().size;
            let curr_start = curr.as_ptr() as usize;
            if prev_end == curr_start {
                prev_ptr.as_mut().size += curr.as_ref().size;
                prev_ptr.as_mut().next = curr.as_ref().next;
            }
        }
    }
}

pub fn init_from_limine() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("dma: no HHDM, cannot init\n");
        return;
    }

    DMA_READY.store(true, Ordering::Release);
    init_pools();
    crate::log!(
        "dma: PMM-backed allocations active span=0x{:X}..0x{:X}\n",
        MIN_DMA_BASE,
        DMA_MAX_PHYS
    );
}

fn ensure_ready() -> bool {
    if DMA_READY.load(Ordering::Acquire) {
        true
    } else {
        crate::log!("dma: not initialized\n");
        false
    }
}

pub fn alloc(size: usize, align: usize) -> Option<(u64, *mut u8)> {
    alloc_with_max(size, align, Some(DMA_MAX_PHYS))
}

/// Allocate a DMA buffer with an explicit upper physical limit.
///
/// `max_phys_exclusive` matches `phys::alloc_phys_range` semantics (end address must be `< max`).
/// Pass `None` to allow allocating anywhere in physical memory.
pub fn alloc_with_max(
    size: usize,
    align: usize,
    max_phys_exclusive: Option<u64>,
) -> Option<(u64, *mut u8)> {
    if size == 0 || !ensure_ready() {
        return None;
    }

    let align = align.max(1);
    if let Some((phys, virt)) = alloc_from_pools(size, align, max_phys_exclusive) {
        POOL_ALLOCS.fetch_add(1, Ordering::Relaxed);
        POOL_BYTES.fetch_add(size as u64, Ordering::Relaxed);
        return Some((phys, virt));
    }
    let phys = crate::phys::alloc_phys_range(size, align, MIN_DMA_BASE, max_phys_exclusive)?;
    let virt = crate::phys::phys_to_virt(phys as usize) as *mut u8;
    PMM_ALLOCS.fetch_add(1, Ordering::Relaxed);
    PMM_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    Some((phys, virt))
}

/// Allocate respecting a conventional DMA mask (e.g. `0xFFFF_FFFF` for 32-bit).
pub fn alloc_with_mask(size: usize, align: usize, dma_mask: u64) -> Option<(u64, *mut u8)> {
    // Convert inclusive mask to exclusive upper bound.
    let max_exclusive = if dma_mask == u64::MAX {
        None
    } else {
        dma_mask.checked_add(1)
    };
    alloc_with_max(size, align, max_exclusive)
}

pub fn dealloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 || !ensure_ready() {
        return;
    }

    if dealloc_to_pools(ptr) {
        POOL_FREES.fetch_add(1, Ordering::Relaxed);
        POOL_BYTES.fetch_sub(size as u64, Ordering::Relaxed);
        return;
    }

    let Some(phys) = crate::phys::virt_to_phys_checked(ptr as *const u8) else {
        crate::log!(
            "dma: dealloc pointer outside known mappings virt=0x{:X}\n",
            ptr as usize
        );
        return;
    };

    if !crate::phys::free_phys_range(phys, size) {
        crate::log!(
            "dma: failed to free region phys=0x{:X} size=0x{:X}\n",
            phys,
            size
        );
    } else {
        PMM_FREES.fetch_add(1, Ordering::Relaxed);
        PMM_BYTES.fetch_sub(size as u64, Ordering::Relaxed);
    }
}

pub fn virt_to_phys(ptr: *const u8) -> Option<u64> {
    crate::phys::virt_to_phys_checked(ptr)
}

pub fn alloc_test_once() {
    if !ensure_ready() {
        return;
    }

    let Some((p1, v1)) = alloc(4096, 4096) else {
        crate::log!("dma: alloc test (4K) failed\n");
        return;
    };

    let Some((p2, v2)) = alloc(256, 64) else {
        crate::log!("dma: alloc test (256B) failed\n");
        return;
    };

    crate::log!("dma: alloc1 phys=0x{:X} virt=0x{:X}\n", p1, v1 as usize);
    crate::log!("dma: alloc2 phys=0x{:X} virt=0x{:X}\n", p2, v2 as usize);
}

fn init_pools() {
    if DMA_POOL_LOW_SIZE > 0 {
        let low_phys = crate::phys::alloc_phys_range(
            DMA_POOL_LOW_SIZE,
            DMA_POOL_ALIGN,
            MIN_DMA_BASE,
            Some(DMA_MAX_PHYS),
        );
        if let Some(phys) = low_phys {
            let virt = crate::phys::phys_to_virt(phys as usize) as *mut u8;
            let pool = DmaPool::new(phys, virt, DMA_POOL_LOW_SIZE);
            *DMA_LOW_POOL.lock() = Some(pool);
            crate::log!(
                "dma: low pool phys=0x{:X} size={} MiB\n",
                phys,
                DMA_POOL_LOW_SIZE / (1024 * 1024)
            );
        } else {
            crate::log!("dma: low pool init failed\n");
        }
    }

    if DMA_POOL_ANY_SIZE > 0 {
        let any_phys = crate::phys::alloc_phys_range(
            DMA_POOL_ANY_SIZE,
            DMA_POOL_ALIGN,
            MIN_DMA_BASE,
            None,
        );
        if let Some(phys) = any_phys {
            let virt = crate::phys::phys_to_virt(phys as usize) as *mut u8;
            let pool = DmaPool::new(phys, virt, DMA_POOL_ANY_SIZE);
            *DMA_ANY_POOL.lock() = Some(pool);
            crate::log!(
                "dma: any pool phys=0x{:X} size={} MiB\n",
                phys,
                DMA_POOL_ANY_SIZE / (1024 * 1024)
            );
        } else {
            crate::log!("dma: any pool init failed\n");
        }
    }
}

fn alloc_from_pools(
    size: usize,
    align: usize,
    max_phys_exclusive: Option<u64>,
) -> Option<(u64, *mut u8)> {
    if let Some(max_phys) = max_phys_exclusive {
        if max_phys <= DMA_MAX_PHYS {
            return DMA_LOW_POOL
                .lock()
                .as_mut()
                .and_then(|pool| pool.alloc(size, align));
        }
    }

    if let Some((phys, virt)) = DMA_ANY_POOL
        .lock()
        .as_mut()
        .and_then(|pool| pool.alloc(size, align))
    {
        return Some((phys, virt));
    }

    DMA_LOW_POOL
        .lock()
        .as_mut()
        .and_then(|pool| pool.alloc(size, align))
}

fn dealloc_to_pools(ptr: *mut u8) -> bool {
    {
        let mut guard = DMA_ANY_POOL.lock();
        if let Some(pool) = guard.as_mut() {
            if pool.contains(ptr) {
                if !pool.dealloc(ptr) {
                    crate::log!("dma: any pool dealloc failed ptr=0x{:X}\n", ptr as usize);
                }
                return true;
            }
        }
    }

    {
        let mut guard = DMA_LOW_POOL.lock();
        if let Some(pool) = guard.as_mut() {
            if pool.contains(ptr) {
                if !pool.dealloc(ptr) {
                    crate::log!("dma: low pool dealloc failed ptr=0x{:X}\n", ptr as usize);
                }
                return true;
            }
        }
    }

    false
}

#[inline(always)]
fn align_up(value: usize, align: usize) -> usize {
    if align <= 1 {
        return value;
    }
    let rem = value % align;
    if rem == 0 {
        value
    } else {
        value + (align - rem)
    }
}

#[inline(always)]
fn align_down(value: usize, align: usize) -> usize {
    if align <= 1 {
        return value;
    }
    value - (value % align)
}
