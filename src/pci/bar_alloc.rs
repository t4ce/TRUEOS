use spin::Mutex;

use ::limine::memmap;

// Minimal physical MMIO allocator for PCI BAR assignment.
//
// Important constraints in TRUEOS today:
// - We do NOT discover the root-bridge MMIO apertures (no ACPI _CRS parsing).
// - So we only allocate from a configured, assumed-safe physical window.
// - This is currently intended for our own endpoint(s), not arbitrary devices.

// Conservative 32-bit PCI MMIO hole below ECAM (0xE000_0000) and above normal RAM growth
// seen in our guest setups. We allocate downward to preserve large aligned holes.
const MMIO32_BASE: u64 = 0xC000_0000;
const MMIO32_LIMIT: u64 = 0xE000_0000; // 512MiB

struct MmioAlloc32 {
    next_top: u64,
}

static MMIO32: Mutex<MmioAlloc32> = Mutex::new(MmioAlloc32 {
    next_top: MMIO32_LIMIT,
});

fn align_down(val: u64, align: u64) -> u64 {
    if align <= 1 {
        return val;
    }
    (val / align) * align
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
        if e.type_ != memmap::MEMMAP_USABLE {
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
    let raw_base = lock.next_top.checked_sub(size)?;
    let base = align_down(raw_base, align);
    let end = base.checked_add(size)?;

    if base < MMIO32_BASE || end > MMIO32_LIMIT {
        return None;
    }

    // Basic safety: never hand out an address that overlaps usable RAM.
    // (We cannot fully validate against the real PCI host bridge apertures yet.)
    if overlaps_usable_ram(base, size) {
        return None;
    }

    lock.next_top = base;
    Some(base as u32)
}
