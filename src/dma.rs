use spin::Mutex;

use crate::debugconf;

const LIMINE_MEMMAP_USABLE: u64 = 0;
const LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE: u64 = 5;
const LIMINE_MEMMAP_ACPI_RECLAIMABLE: u64 = 2;

// Keep away from the very bottom; anything above 64 KiB is fine for now.
const MIN_DMA_BASE: u64 = 64 * 1024;
// Minimum length we accept before falling back to the largest usable chunk.
const MIN_DMA_LEN: u64 = 4 * 1024;

#[derive(Copy, Clone)]
struct DmaBump {
    next: u64,
    end: u64,
    hhdm: u64,
}

static DMA: Mutex<Option<DmaBump>> = Mutex::new(None);

#[inline(always)]
const fn align_up(value: u64, align: u64) -> u64 {
    if align <= 1 {
        return value;
    }
    let mask = align - 1;
    (value + mask) & !mask
}

/// Initialize a simple physical bump allocator for DMA buffers.
///
/// Uses Limine memmap to find a usable RAM region and returns HHDM-backed
/// virtual addresses for CPU access.
pub fn init_from_limine() {
    let Some(hhdm) = crate::limine::hhdm_offset() else {
        debugconf!("dma: no HHDM, cannot init\n");
        return;
    };

    let Some(entries) = crate::limine::memmap_entries() else {
        debugconf!("dma: no memmap, cannot init\n");
        return;
    };

    debugconf!("dma: scanning memmap entries={} (usable, bootloader reclaim, ACPI reclaim)\n", entries.len());


}

/// Allocate a physically contiguous DMA buffer.
///
/// Returns (physical_address, virtual_pointer_in_hhdm).
pub fn alloc(size: usize, align: usize) -> Option<(u64, *mut u8)> {
    let mut lock = DMA.lock();
    let bump = lock.as_mut()?;

    let size_u64 = u64::try_from(size).ok()?;
    let align_u64 = u64::try_from(align.max(1)).ok()?;

    let phys = align_up(bump.next, align_u64);
    let next = phys.checked_add(size_u64)?;
    if next > bump.end {
        return None;
    }

    bump.next = next;
    let virt = phys.wrapping_add(bump.hhdm) as *mut u8;
    Some((phys, virt))
}

pub fn alloc_test_once() {
    if DMA.lock().is_none() {
        debugconf!("dma: not initialized\n");
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
