use core::sync::atomic::{AtomicBool, Ordering};

use crate::debugconf;

const MIN_DMA_BASE: u64 = 64 * 1024;
const DMA_MAX_PHYS: u64 = 0x1_0000_0000; // stay under 4 GiB for legacy devices

static DMA_READY: AtomicBool = AtomicBool::new(false);

pub fn init_from_limine() {
    if crate::limine::hhdm_offset().is_none() {
        debugconf!("dma: no HHDM, cannot init\n");
        return;
    }

    DMA_READY.store(true, Ordering::Release);
    debugconf!(
        "dma: PMM-backed allocations active span=0x{:X}..0x{:X}\n",
        MIN_DMA_BASE,
        DMA_MAX_PHYS
    );
}

fn ensure_ready() -> bool {
    if DMA_READY.load(Ordering::Acquire) {
        true
    } else {
        debugconf!("dma: not initialized\n");
        false
    }
}

pub fn alloc(size: usize, align: usize) -> Option<(u64, *mut u8)> {
    if size == 0 || !ensure_ready() {
        return None;
    }

    let align = align.max(1);
    let phys = crate::phys::alloc_phys_range(size, align, MIN_DMA_BASE, Some(DMA_MAX_PHYS))?;
    let virt = crate::phys::phys_to_virt(phys as usize) as *mut u8;
    Some((phys, virt))
}

pub fn dealloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 || !ensure_ready() {
        return;
    }

    let Some(phys) = crate::phys::virt_to_phys_checked(ptr as *const u8) else {
        debugconf!(
            "dma: dealloc pointer outside known mappings virt=0x{:X}\n",
            ptr as usize
        );
        return;
    };

    if !crate::phys::free_phys_range(phys, size) {
        debugconf!(
            "dma: failed to free region phys=0x{:X} size=0x{:X}\n",
            phys,
            size
        );
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
        debugconf!("dma: alloc test (4K) failed\n");
        return;
    };

    let Some((p2, v2)) = alloc(256, 64) else {
        debugconf!("dma: alloc test (256B) failed\n");
        return;
    };

    debugconf!("dma: alloc1 phys=0x{:X} virt=0x{:X}\n", p1, v1 as usize);
    debugconf!("dma: alloc2 phys=0x{:X} virt=0x{:X}\n", p2, v2 as usize);
}
