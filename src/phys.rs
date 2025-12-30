use core::{ptr, sync::atomic::{AtomicU64, Ordering}};
use heapless::Vec;
use limine::memory_map::EntryType;
use spin::Mutex;

use crate::debugconf;

extern "C" {
    static kernel_end: u8;
}

static HEAP_VIRT_BASE: AtomicU64 = AtomicU64::new(0);
static HEAP_PHYS_BASE: AtomicU64 = AtomicU64::new(0);
static HEAP_LEN: AtomicU64 = AtomicU64::new(0);
static HHDM_BASE: AtomicU64 = AtomicU64::new(0);
static KERNEL_VIRT_BASE: AtomicU64 = AtomicU64::new(0);
static KERNEL_PHYS_BASE: AtomicU64 = AtomicU64::new(0);
static KERNEL_LEN: AtomicU64 = AtomicU64::new(0);

const PAGE_SIZE: u64 = 4096;
const MIN_USABLE_BASE: u64 = 0x0010_0000; // keep lower memory for firmware/BIOS data
const MAX_PMM_REGIONS: usize = 256;

#[derive(Copy, Clone, Debug)]
pub struct HeapArena {
    pub phys_start: u64,
    pub virt_start: usize,
    pub length: usize,
}

#[derive(Copy, Clone, Debug)]
struct PhysRegion {
    start: u64,
    end: u64,
}

struct PmmState {
    regions: Vec<PhysRegion, MAX_PMM_REGIONS>,
}

static PMM: Mutex<Option<PmmState>> = Mutex::new(None);

pub fn register_heap(virt_base: usize, phys_base: usize, length: usize) {
    HEAP_VIRT_BASE.store(virt_base as u64, Ordering::SeqCst);
    HEAP_PHYS_BASE.store(phys_base as u64, Ordering::SeqCst);
    HEAP_LEN.store(length as u64, Ordering::SeqCst);
}

pub fn register_hhdm_base(base: usize) {
    HHDM_BASE.store(base as u64, Ordering::SeqCst);
}

pub fn register_kernel_image(virt_base: usize, phys_base: usize, length: usize) {
    KERNEL_VIRT_BASE.store(virt_base as u64, Ordering::SeqCst);
    KERNEL_PHYS_BASE.store(phys_base as u64, Ordering::SeqCst);
    KERNEL_LEN.store(length as u64, Ordering::SeqCst);
}

/// Capture Limine-provided mapping metadata for later address translation.
pub fn register_memory_metadata() {
    if let Some(hhdm) = crate::limine::hhdm_offset() {
        register_hhdm_base(hhdm as usize);
    }

    if let Some((virt_base, phys_base)) = crate::limine::executable_address_bases() {
        let virt_base = virt_base as usize;
        let phys_base = phys_base as usize;
        let kernel_len = (ptr::addr_of!(kernel_end) as usize).saturating_sub(virt_base);
        register_kernel_image(virt_base, phys_base, kernel_len);
    }
}

pub fn init_pmm_from_limine() {
    let Some(entries) = crate::limine::memmap_entries() else {
        debugconf!("pmm: no Limine memmap; PMM disabled\n");
        return;
    };

    let hhdm = HHDM_BASE.load(Ordering::Relaxed);
    if hhdm == 0 {
        debugconf!("pmm: no HHDM registered; cannot hand out virtual heap arena\n");
        return;
    }

    let mut state = PmmState::new();
    for entry in entries {
        if entry.entry_type != EntryType::USABLE {
            continue;
        }

        let base = entry.base;
        let end = entry.base.saturating_add(entry.length);
        let start = align_up_u64(base.max(MIN_USABLE_BASE), PAGE_SIZE);
        let end = align_down_u64(end, PAGE_SIZE);
        if end <= start {
            continue;
        }

        if state.add_region(start, end).is_err() {
            debugconf!(
                "pmm: region table full dropping 0x{:X}..0x{:X}\n",
                start,
                end
            );
        }
    }

    state.finalize();
    let total_bytes = state.total_bytes();
    let region_count = state.region_count();
    let mut guard = PMM.lock();
    if region_count == 0 {
        *guard = None;
        debugconf!("pmm: no usable regions available\n");
    } else {
        let first = state.regions.first().copied().unwrap();
        let last = state.regions.last().copied().unwrap();
        *guard = Some(state);
        debugconf!(
            "pmm: regions={} span=0x{:X}..0x{:X} total={} MiB\n",
            region_count,
            first.start,
            last.end,
            total_bytes / (1024 * 1024)
        );
    }
}

pub fn reserve_heap_arena(size: usize, align: usize) -> Option<HeapArena> {
    if size == 0 {
        return None;
    }

    if HHDM_BASE.load(Ordering::Relaxed) == 0 {
        debugconf!("pmm: cannot reserve heap arena without HHDM mapping\n");
        return None;
    }

    let align = align.max(PAGE_SIZE as usize);
    let phys = alloc_phys_range(size, align, MIN_USABLE_BASE, None)?;
    let virt = phys_to_virt(phys as usize);
    Some(HeapArena {
        phys_start: phys,
        virt_start: virt,
        length: size,
    })
}

pub fn alloc_phys_range(
    size: usize,
    align: usize,
    min_phys: u64,
    max_phys: Option<u64>,
) -> Option<u64> {
    let size_u64 = u64::try_from(size).ok()?;
    let align_u64 = u64::try_from(align.max(1)).ok()?;
    let mut guard = PMM.lock();
    let state = guard.as_mut()?;
    state.allocate(size_u64, align_u64, min_phys, max_phys)
}

pub fn free_phys_range(start: u64, size: usize) -> bool {
    if size == 0 {
        return false;
    }
    let size_u64 = match u64::try_from(size) {
        Ok(v) if v != 0 => v,
        _ => return false,
    };
    let mut guard = PMM.lock();
    let Some(state) = guard.as_mut() else {
        return false;
    };
    state.release(start, size_u64)
}

/// Translate a physical address into a higher-half direct map (if present).
#[inline(always)]
pub fn phys_to_virt(phys: usize) -> usize {
    let hhdm = HHDM_BASE.load(Ordering::Relaxed);
    if hhdm != 0 {
        phys.checked_add(hhdm as usize).unwrap_or_else(|| {
            crate::debugconf!(
                "phys_to_virt: overflow translating phys=0x{:X} with hhdm=0x{:X}\n",
                phys,
                hhdm
            );
            phys
        })
    } else {
        phys
    }
}

#[inline(always)]
pub fn virt_to_phys_checked<T>(ptr: *const T) -> Option<u64> {
    translate_virt(ptr as usize)
}

fn translate_virt(addr: usize) -> Option<u64> {
    let addr_u64 = addr as u64;

    // Heap window registered from the allocator selection.
    let virt_base = HEAP_VIRT_BASE.load(Ordering::Relaxed);
    let len = HEAP_LEN.load(Ordering::Relaxed);
    if len != 0 {
        let virt_end = virt_base + len;
        if addr_u64 >= virt_base && addr_u64 < virt_end {
            let phys_base = HEAP_PHYS_BASE.load(Ordering::Relaxed);
            return Some(phys_base + (addr_u64 - virt_base));
        }
    }

    // Kernel image mapping (higher-half offset).
    let kern_base = KERNEL_VIRT_BASE.load(Ordering::Relaxed);
    let kern_len = KERNEL_LEN.load(Ordering::Relaxed);
    if kern_len != 0 && addr_u64 >= kern_base && addr_u64 < kern_base + kern_len {
        let phys_base = KERNEL_PHYS_BASE.load(Ordering::Relaxed);
        return Some(phys_base + (addr_u64 - kern_base));
    }

    // Higher-half direct map (HHDM) covers raw physical memory.
    let hhdm = HHDM_BASE.load(Ordering::Relaxed);
    if hhdm != 0 && addr_u64 >= hhdm {
        return Some(addr_u64 - hhdm);
    }

    None
}

impl PmmState {
    fn new() -> Self {
        Self {
            regions: Vec::new(),
        }
    }

    fn add_region(&mut self, start: u64, end: u64) -> Result<(), ()> {
        let region = PhysRegion { start, end };
        self.regions.push(region).map_err(|_| ())
    }

    fn finalize(&mut self) {
        let slice = self.regions.as_mut_slice();
        slice.sort_by(|a, b| a.start.cmp(&b.start));
        self.merge_regions();
    }

    fn merge_regions(&mut self) {
        if self.regions.len() < 2 {
            return;
        }

        let mut idx = 1;
        while idx < self.regions.len() {
            let prev = self.regions[idx - 1];
            let curr = self.regions[idx];
            if prev.end >= curr.start {
                let new_end = prev.end.max(curr.end);
                self.regions[idx - 1].end = new_end;
                self.regions.remove(idx);
            } else {
                idx += 1;
            }
        }
    }

    fn allocate(
        &mut self,
        size: u64,
        align: u64,
        min_phys: u64,
        max_phys: Option<u64>,
    ) -> Option<u64> {
        for idx in 0..self.regions.len() {
            let region = self.regions[idx];
            let mut start = region.start.max(min_phys);
            start = align_up_u64(start, align);

            if let Some(max_addr) = max_phys {
                if start >= max_addr {
                    continue;
                }
            }

            let end = start.checked_add(size)?;
            if end > region.end {
                continue;
            }
            if let Some(max_addr) = max_phys {
                if end > max_addr {
                    continue;
                }
            }

            self.regions.remove(idx);
            let mut insert_pos = idx;
            if region.start < start {
                let _ = self.regions.insert(
                    insert_pos,
                    PhysRegion {
                        start: region.start,
                        end: start,
                    },
                );
                insert_pos += 1;
            }
            if end < region.end {
                let _ = self.regions.insert(
                    insert_pos,
                    PhysRegion {
                        start: end,
                        end: region.end,
                    },
                );
            }

            return Some(start);
        }
        None
    }

    fn release(&mut self, start: u64, size: u64) -> bool {
        if size == 0 {
            return false;
        }
        let end = match start.checked_add(size) {
            Some(v) if v > start => v,
            _ => return false,
        };

        let mut idx = 0;
        while idx < self.regions.len() && self.regions[idx].start < start {
            idx += 1;
        }

        if self.regions.insert(idx, PhysRegion { start, end }).is_err() {
            return false;
        }

        self.merge_regions();
        true
    }

    fn total_bytes(&self) -> u64 {
        self.regions
            .iter()
            .map(|r| r.end.saturating_sub(r.start))
            .sum()
    }

    fn region_count(&self) -> usize {
        self.regions.len()
    }
}

#[inline(always)]
const fn align_up_u64(value: u64, align: u64) -> u64 {
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
const fn align_down_u64(value: u64, align: u64) -> u64 {
    if align <= 1 {
        return value;
    }
    value - (value % align)
}
