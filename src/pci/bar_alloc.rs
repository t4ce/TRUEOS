use spin::Mutex;

use ::limine::memory_map::EntryType;

// Minimal physical MMIO allocator for PCI BAR assignment.
//
// Important constraints in TRUEOS today:
// - We do NOT discover the root-bridge MMIO apertures (no ACPI _CRS parsing).
// - So we only allocate from a configured, assumed-safe physical window.
// - This is currently intended for our own endpoint(s), not arbitrary devices.

// Default window used for TGA BAR0 assignment.
// This matches the prior known-good fixed base used during bring-up.
const TGA_MMIO32_BASE: u64 = 0x5340_0000;
const TGA_MMIO32_LIMIT: u64 = 0x5350_0000; // 16MiB

struct MmioAlloc32 {
    next: u64,
}

static MMIO32: Mutex<MmioAlloc32> = Mutex::new(MmioAlloc32 { next: TGA_MMIO32_BASE });

fn align_up(val: u64, align: u64) -> u64 {
    if align == 0 {
        return val;
    }
    (val + (align - 1)) & !(align - 1)
}

fn overlaps(a_base: u64, a_len: u64, b_base: u64, b_len: u64) -> bool {
    if a_len == 0 || b_len == 0 {
        return false;
    }
    let a_end = a_base.saturating_add(a_len);
    let b_end = b_base.saturating_add(b_len);
    a_base < b_end && b_base < a_end
}

fn overlaps_usable_ram(base: u64, len: u64) -> bool {
    let Some(entries) = crate::limine::memmap_entries() else {
        return false;
    };

    for e in entries {
        if e.entry_type != EntryType::USABLE {
            continue;
        }
        if overlaps(base, len, e.base, e.length) {
            return true;
        }
    }

    false
}

/// Allocate a 32-bit MMIO base address from the configured window.
///
/// Returns `None` if the request cannot be satisfied.
pub fn alloc_mmio32(size: u64, align: u64) -> Option<u32> {
    let size = size.max(0x1000);
    let align = align.max(0x1000);

    let mut lock = MMIO32.lock();
    let base = align_up(lock.next, align);
    let end = base.checked_add(size)?;

    if base < TGA_MMIO32_BASE || end > TGA_MMIO32_LIMIT {
        return None;
    }

    // Basic safety: never hand out an address that overlaps usable RAM.
    // (We cannot fully validate against the real PCI host bridge apertures yet.)
    if overlaps_usable_ram(base, size) {
        return None;
    }

    lock.next = end;
    Some(base as u32)
}

/// Reserve a stable BAR0 base for the TGA endpoint.
///
/// This is intentionally simple: TRUEOS currently models a single TGA.
pub fn alloc_tga_bar0_base(size: u64) -> Option<u32> {
    // Our FPGA BAR0 is currently small (1KiB), but align to the reported BAR size.
    let align = size.max(0x1000);
    alloc_mmio32(size, align)
}
