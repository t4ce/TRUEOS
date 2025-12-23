use heapless::Vec;
use spin::Mutex;

use crate::debugconf;

const LIMINE_MEMMAP_USABLE: u64 = 0;
const LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE: u64 = 5;
const LIMINE_MEMMAP_ACPI_RECLAIMABLE: u64 = 2;

// Keep away from the very bottom; anything above 64 KiB is fine for now.
const MIN_DMA_BASE: u64 = 64 * 1024;
// Minimum length we accept before falling back to the largest usable chunk.
const MIN_DMA_LEN: u64 = 4 * 1024;

#[derive(Copy, Clone, Debug)]
struct DmaRegion {
    start: u64,
    end: u64,
}

#[derive(Clone)]
struct DmaAllocator {
    regions: Vec<DmaRegion, MAX_DMA_REGIONS>,
    hhdm: u64,
}

const MAX_DMA_REGIONS: usize = 64;

static DMA: Mutex<Option<DmaAllocator>> = Mutex::new(None);

#[inline(always)]
const fn align_up(value: u64, align: u64) -> u64 {
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

    let mut allocator = DmaAllocator {
        regions: Vec::new(),
        hhdm,
    };

    for &ptr in entries {
        if ptr.is_null() {
            continue;
        }
        let e = unsafe { &*ptr };
        let allowed = matches!(
            e.typ,
            LIMINE_MEMMAP_USABLE | LIMINE_MEMMAP_BOOTLOADER_RECLAIMABLE | LIMINE_MEMMAP_ACPI_RECLAIMABLE
        );
        if !allowed {
            continue;
        }

        let base = e.base;
        let len = e.length;
        if base < MIN_DMA_BASE || len < MIN_DMA_LEN {
            debugconf!("dma: skip usable base=0x{:X} len=0x{:X}\n", base, len);
            continue;
        }

        let start = align_up(base.max(MIN_DMA_BASE), 4096);
        let end = base.saturating_add(len);
        if end <= start {
            continue;
        }

        if allocator.regions.push(DmaRegion { start, end }).is_err() {
            debugconf!("dma: region list full, dropping 0x{:X}..0x{:X}\n", start, end);
        }
    }

    let slice = allocator.regions.as_mut_slice();
    slice.sort_by(|a, b| a.start.cmp(&b.start));

    let mut dma = DMA.lock();
    if allocator.regions.is_empty() {
        *dma = None;
        debugconf!("dma: no suitable regions\n");
    } else {
        let region_count = allocator.regions.len();
        let total_kib: u64 = allocator
            .regions
            .iter()
            .map(|r| r.end.saturating_sub(r.start))
            .sum::<u64>()
            / 1024;
        let first = allocator.regions[0];
        let last = allocator.regions[region_count - 1];
        *dma = Some(allocator);
        debugconf!(
            "dma: regions={} span=0x{:X}..0x{:X} total={} KiB\n",
            region_count,
            first.start,
            last.end,
            total_kib
        );
    }
}

/// Allocate a physically contiguous DMA buffer.
///
/// Returns (physical_address, virtual_pointer_in_hhdm).
pub fn alloc(size: usize, align: usize) -> Option<(u64, *mut u8)> {
    let mut lock = DMA.lock();
    let alloc = lock.as_mut()?;
    alloc.alloc(size, align)
}

/// Return DMA memory back to the allocator.
pub fn dealloc(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }

    let mut lock = DMA.lock();
    let Some(alloc) = lock.as_mut() else {
        return;
    };

    let Some(phys) = alloc.virt_to_phys(ptr as usize) else {
        debugconf!("dma: dealloc pointer outside HHDM virt=0x{:X}\n", ptr as usize);
        return;
    };

    alloc.free_region(phys, size as u64);
}

/// Translate an HHDM virtual pointer back to its physical address.
pub fn virt_to_phys(ptr: *const u8) -> Option<u64> {
    if ptr.is_null() {
        return None;
    }
    let lock = DMA.lock();
    let alloc = lock.as_ref()?;
    alloc.virt_to_phys(ptr as usize)
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

impl DmaAllocator {
    fn alloc(&mut self, size: usize, align: usize) -> Option<(u64, *mut u8)> {
        let size_u64 = u64::try_from(size).ok()?;
        let align_u64 = u64::try_from(align.max(1)).ok()?;

        for idx in 0..self.regions.len() {
            let region = self.regions[idx];
            let phys = align_up(region.start, align_u64);
            let end = phys.checked_add(size_u64)?;
            if end > region.end {
                continue;
            }

            // Remove current region and reinsert leftovers (before/after).
            self.regions.remove(idx);
            let mut insert_pos = idx;
            if region.start < phys {
                let _ = self.regions.insert(insert_pos, DmaRegion { start: region.start, end: phys });
                insert_pos += 1;
            }
            if end < region.end {
                let _ = self.regions.insert(insert_pos, DmaRegion { start: end, end: region.end });
            }

            let virt = phys.wrapping_add(self.hhdm) as *mut u8;
            return Some((phys, virt));
        }
        None
    }

    fn virt_to_phys(&self, virt: usize) -> Option<u64> {
        let virt_u64 = virt as u64;
        if virt_u64 < self.hhdm {
            return None;
        }
        Some(virt_u64 - self.hhdm)
    }

    fn free_region(&mut self, phys: u64, size: u64) {
        if size == 0 {
            return;
        }

        let start = phys;
        let end = start.saturating_add(size);
        if end <= start {
            return;
        }

        let mut idx = 0;
        while idx < self.regions.len() && self.regions[idx].start < start {
            idx += 1;
        }

        if self.regions.insert(idx, DmaRegion { start, end }).is_err() {
            debugconf!("dma: free list full dropping 0x{:X}..0x{:X}\n", start, end);
            return;
        }

        self.merge_regions();
    }

    fn merge_regions(&mut self) {
        if self.regions.len() < 2 {
            return;
        }

        let mut idx = 1;
        while idx < self.regions.len() {
            let (prev_start, prev_end) = {
                let prev = self.regions[idx - 1];
                (prev.start, prev.end)
            };
            let curr = self.regions[idx];
            if prev_end >= curr.start {
                let new_end = prev_end.max(curr.end);
                self.regions[idx - 1].end = new_end;
                self.regions.remove(idx);
            } else {
                idx += 1;
            }
        }
    }
}
