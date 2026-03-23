use core::alloc::Layout;
use core::sync::atomic::{AtomicBool, Ordering};

use heapless::Vec;
use spin::Mutex;

const MIN_DMA_BASE: u64 = 64 * 1024;
const DMA_MAX_PHYS: u64 = 0x1_0000_0000;
const MAX_DMA_ALLOCS: usize = 2048;

static DMA_READY: AtomicBool = AtomicBool::new(false);
static DMA_ALLOCS: Mutex<Vec<DmaAlloc, MAX_DMA_ALLOCS>> = Mutex::new(Vec::new());

#[derive(Copy, Clone)]
enum DmaAllocOrigin {
    Pmm,
}

#[derive(Copy, Clone)]
struct DmaAlloc {
    virt: usize,
    phys: u64,
    size: usize,
    origin: DmaAllocOrigin,
}

pub fn init_from_limine() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("dma: no HHDM, cannot init\n");
        return;
    }

    DMA_READY.store(true, Ordering::Release);
    crate::log!("dma: pmm-backed DMA allocator active\n");
}

pub fn alloc(size: usize, align: usize) -> Option<(u64, *mut u8)> {
    alloc_with_max(size, align, Some(DMA_MAX_PHYS))
}

pub fn alloc_with_max(
    size: usize,
    align: usize,
    max_phys_exclusive: Option<u64>,
) -> Option<(u64, *mut u8)> {
    if size == 0 || !ensure_ready() {
        return None;
    }

    let layout = Layout::from_size_align(size, align.max(1)).ok()?;

    if let Some((phys, virt, origin)) = alloc_from_pmm(size, layout.align(), max_phys_exclusive) {
        if register_alloc(virt, phys, size, origin) {
            return Some((phys, virt));
        }

        unsafe {
            match origin {
                DmaAllocOrigin::Pmm => {
                    let _ = crate::phys::free_phys_range(phys, size);
                }
            }
        }
    }

    None
}

pub fn dealloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 || !ensure_ready() {
        return;
    }

    let Some(alloc) = unregister_alloc(ptr) else {
        crate::log!(
            "dma: unknown dealloc ptr=0x{:X} size=0x{:X}\n",
            ptr as usize,
            size
        );
        return;
    };

    match alloc.origin {
        DmaAllocOrigin::Pmm => {
            if !crate::phys::free_phys_range(alloc.phys, alloc.size) {
                crate::log!(
                    "dma: failed to free PMM region phys=0x{:X} size=0x{:X}\n",
                    alloc.phys,
                    alloc.size
                );
            }
        }
    }
}

fn ensure_ready() -> bool {
    if DMA_READY.load(Ordering::Acquire) {
        true
    } else {
        crate::log!("dma: not initialized\n");
        false
    }
}

fn alloc_from_pmm(
    size: usize,
    align: usize,
    max_phys_exclusive: Option<u64>,
) -> Option<(u64, *mut u8, DmaAllocOrigin)> {
    let phys = crate::phys::alloc_phys_range(size, align.max(1), MIN_DMA_BASE, max_phys_exclusive)?;
    let virt = crate::phys::phys_to_virt(phys as usize) as *mut u8;
    Some((phys, virt, DmaAllocOrigin::Pmm))
}

fn fits_range(phys: u64, size: usize, max_phys_exclusive: Option<u64>) -> bool {
    let Some(end) = phys.checked_add(size.saturating_sub(1) as u64) else {
        return false;
    };

    match max_phys_exclusive {
        Some(max) => end < max,
        None => true,
    }
}

fn register_alloc(virt: *mut u8, phys: u64, size: usize, origin: DmaAllocOrigin) -> bool {
    let mut allocs = DMA_ALLOCS.lock();
    allocs
        .push(DmaAlloc {
            virt: virt as usize,
            phys,
            size,
            origin,
        })
        .is_ok()
}

fn unregister_alloc(ptr: *mut u8) -> Option<DmaAlloc> {
    let mut allocs = DMA_ALLOCS.lock();
    let idx = allocs.iter().position(|alloc| alloc.virt == ptr as usize)?;
    Some(allocs.swap_remove(idx))
}
