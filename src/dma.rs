use core::alloc::Layout;
use core::sync::atomic::{AtomicBool, Ordering};

const MIN_DMA_BASE: u64 = 64 * 1024;
const DMA_MAX_PHYS: u64 = 0x1_0000_0000;

static DMA_READY: AtomicBool = AtomicBool::new(false);

pub fn init_from_limine() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("dma: no HHDM, cannot init\n");
        return;
    }

    DMA_READY.store(true, Ordering::Release);
    if crate::logflag::BOOT_INFO_LOGS {
        crate::log!("dma: pmm-backed DMA allocator active\n");
    }
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

    alloc_from_pmm(size, layout.align(), max_phys_exclusive)
}

pub fn dealloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 || !ensure_ready() {
        return;
    }

    let Some(phys) = crate::phys::virt_to_phys_checked(ptr) else {
        crate::log!("dma: dealloc ptr not translatable ptr=0x{:X}\n", ptr as usize);
        return;
    };

    if !crate::phys::free_phys_range(phys, size) {
        crate::log!("dma: failed to free PMM region phys=0x{:X} size=0x{:X}\n", phys, size);
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
) -> Option<(u64, *mut u8)> {
    let phys = crate::phys::alloc_phys_range(size, align.max(1), MIN_DMA_BASE, max_phys_exclusive)?;
    let virt = crate::phys::phys_to_virt(phys as usize) as *mut u8;
    Some((phys, virt))
}
