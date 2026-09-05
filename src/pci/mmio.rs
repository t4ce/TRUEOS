use core::{
    cmp,
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering},
};

use spin::Mutex;
use x86_64::{
    PhysAddr, VirtAddr,
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame,
        Size4KiB,
        mapper::{MapToError, Mapper},
    },
};

use crate::dma;
use crate::limine;

const DEFAULT_MMIO_WINDOW: usize = 0x10_000;
const PAGE_SIZE: u64 = Size4KiB::SIZE;

// Dedicated virtual address space for MMIO mappings.
//
// Rationale:
// - Limine typically provides an HHDM direct map that may be backed by huge pages
//   and/or cacheable mappings.
// - Attempting to remap the same HHDM pages as uncached can fail (ParentEntryHugePage)
//   or be ignored (PageAlreadyMapped), leaving devices accessed through cacheable aliases.
// - Cacheable MMIO aliases can manifest as "device timeouts" (stale reads / posted writes).
//
// So we map MMIO into a separate, always-4KiB-mapped region with explicit flags.
const MMIO_VIRT_BASE: u64 = 0xFFFF_FF00_0000_0000;
const MMIO_VIRT_LIMIT: u64 = 0xFFFF_FF80_0000_0000;
pub(crate) const MMIO_PML4_SLOT: usize = ((MMIO_VIRT_BASE >> 39) & 0x1ff) as usize;

static MMIO_NEXT_VIRT: AtomicU64 = AtomicU64::new(MMIO_VIRT_BASE);

static PAGING_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub enum MapError {
    NoHhdm,
    NotPhysical,
    InvalidArgs,
    InvalidPointer,
    AddressConflict,
    FrameAllocationFailed,
}

/// Map an address that may be a physical address or an HHDM address.
///
/// This follows the kernel-side contract:
/// 1) Translate the address into a physical address via Limine metadata.
/// 2) Explicitly map the physical range before dereferencing.
pub fn map_limine_addr_exact(addr: u64, size: usize) -> Result<NonNull<u8>, MapError> {
    if size == 0 {
        return Err(MapError::InvalidArgs);
    }
    let phys = limine::try_as_phys_addr(addr).ok_or(MapError::NotPhysical)?;
    map_mmio_region_exact(phys, size)
}

pub fn map_limine_struct<T>(addr: u64) -> Result<NonNull<T>, MapError> {
    let size = core::mem::size_of::<T>();
    if size == 0 {
        return Err(MapError::InvalidArgs);
    }
    let align = core::mem::align_of::<T>();
    if align != 0 && !(addr as usize).is_multiple_of(align) {
        return Err(MapError::InvalidArgs);
    }
    let mapped = map_limine_addr_exact(addr, size)?;
    NonNull::new(mapped.as_ptr() as *mut T).ok_or(MapError::InvalidPointer)
}

pub fn map_limine_slice<T>(addr: u64, count: usize) -> Result<(NonNull<T>, usize), MapError> {
    if count == 0 {
        return Err(MapError::InvalidArgs);
    }
    let elem_size = core::mem::size_of::<T>();
    if elem_size == 0 {
        return Err(MapError::InvalidArgs);
    }
    let align = core::mem::align_of::<T>();
    if align != 0 && !(addr as usize).is_multiple_of(align) {
        return Err(MapError::InvalidArgs);
    }
    let byte_len = elem_size.checked_mul(count).ok_or(MapError::InvalidArgs)?;
    let mapped = map_limine_addr_exact(addr, byte_len)?;
    let ptr = NonNull::new(mapped.as_ptr() as *mut T).ok_or(MapError::InvalidPointer)?;
    Ok((ptr, count))
}

pub fn map_mmio_region(phys_base: u64, size: usize) -> Result<NonNull<u8>, MapError> {
    map_mmio_region_custom(phys_base, cmp::max(size, DEFAULT_MMIO_WINDOW))
}

/// Map exactly the requested size (no default window expansion).
pub fn map_mmio_region_exact(phys_base: u64, size: usize) -> Result<NonNull<u8>, MapError> {
    map_mmio_region_custom(phys_base, size)
}

/// Map ordinary cacheable RAM at a specific virtual address in the active
/// kernel pagemap. The virtual and physical page offsets must match.
///
/// This is the shared-address primitive used by the retained firmware-context
/// bridge: Limine leaves a tiny trampoline/control page mapped in its original
/// firmware address space, and TRUEOS maps the same physical pages at the same
/// virtual addresses before crossing CR3. It is deliberately not an MMIO path.
pub fn map_ram_region_at(
    virt_base: u64,
    phys_base: u64,
    size: usize,
    executable: bool,
) -> Result<NonNull<u8>, MapError> {
    if size == 0 || (virt_base & (PAGE_SIZE - 1)) != (phys_base & (PAGE_SIZE - 1)) {
        return Err(MapError::InvalidArgs);
    }

    // `VirtAddr::new` must never see a non-canonical value. TRUEOS currently
    // runs four-level paging, so accept the ordinary x86-64 canonical halves.
    let canonical = virt_base <= 0x0000_7FFF_FFFF_FFFF
        || virt_base >= 0xFFFF_8000_0000_0000;
    if !canonical {
        return Err(MapError::InvalidArgs);
    }

    let hhdm = limine::hhdm_offset().ok_or(MapError::NoHhdm)?;
    let page_offset = virt_base & (PAGE_SIZE - 1);
    let virt_start = virt_base - page_offset;
    let phys_start = phys_base - page_offset;
    let span = u64::try_from(size)
        .ok()
        .and_then(|size| size.checked_add(page_offset))
        .ok_or(MapError::InvalidArgs)?;
    let total = align_up(span, PAGE_SIZE);
    let virt_end = virt_start.checked_add(total).ok_or(MapError::InvalidArgs)?;
    if virt_end == 0 {
        return Err(MapError::InvalidArgs);
    }

    let _guard = PAGING_LOCK.lock();
    let phys_offset = VirtAddr::new(hhdm);
    let mut mapper = unsafe { active_mapper(phys_offset)? };
    let mut allocator = PageTableAllocator;

    let mut flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    if !executable {
        flags |= PageTableFlags::NO_EXECUTE;
    }
    let table_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for delta in (0..total as usize).step_by(PAGE_SIZE as usize) {
        let virt_addr = virt_start + delta as u64;
        let phys_addr = phys_start + delta as u64;
        let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(virt_addr));
        let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));

        unsafe {
            match mapper.map_to_with_table_flags(page, frame, flags, table_flags, &mut allocator) {
                Ok(flush) => flush.flush(),
                Err(MapToError::PageAlreadyMapped(existing)) => {
                    if existing != frame {
                        return Err(MapError::AddressConflict);
                    }
                }
                // A huge parent can only be accepted for an identity alias;
                // otherwise we cannot prove that it backs the requested frame.
                Err(MapToError::ParentEntryHugePage) => {
                    if virt_addr != phys_addr {
                        return Err(MapError::AddressConflict);
                    }
                }
                Err(MapToError::FrameAllocationFailed) => {
                    return Err(MapError::FrameAllocationFailed);
                }
            }
        }
    }

    NonNull::new(virt_base as *mut u8).ok_or(MapError::InvalidPointer)
}

/// Identity-map ordinary RAM at its physical address in the active kernel
/// pagemap. This is intentionally separate from MMIO mapping: retained UEFI
/// code/data is normal cacheable RAM and firmware function pointers still name
/// the pre-ExitBootServices identity address.
pub fn map_identity_ram_region(
    phys_base: u64,
    size: usize,
    executable: bool,
) -> Result<(), MapError> {
    map_ram_region_at(phys_base, phys_base, size, executable).map(|_| ())
}

fn map_mmio_region_custom(phys_base: u64, map_size: usize) -> Result<NonNull<u8>, MapError> {
    let requested = map_size;
    // We still require HHDM to access the active page tables (CR3 walk uses it).
    let _hhdm = limine::hhdm_offset().ok_or(MapError::NoHhdm)?;

    let phys_start = phys_base & !(PAGE_SIZE - 1);
    let offset = (phys_base - phys_start) as usize;
    let span = requested.checked_add(offset).ok_or(MapError::InvalidArgs)? as u64;
    let total = align_up(span, PAGE_SIZE);

    let total = total as usize;

    // Allocate a fresh virtual window for this MMIO mapping.
    let total_u64 = total as u64;
    let virt_start = MMIO_NEXT_VIRT.fetch_add(total_u64, Ordering::AcqRel);
    if virt_start < MMIO_VIRT_BASE || virt_start.saturating_add(total_u64) > MMIO_VIRT_LIMIT {
        return Err(MapError::FrameAllocationFailed);
    }

    let _guard = PAGING_LOCK.lock();
    let phys_offset = VirtAddr::new(_hhdm);
    let mut mapper = unsafe { active_mapper(phys_offset)? };
    let mut allocator = PageTableAllocator;

    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH;
    let table_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for delta in (0..total).step_by(PAGE_SIZE as usize) {
        let phys_addr = phys_start + delta as u64;
        let virt_addr = virt_start + delta as u64;

        let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(virt_addr));
        let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));

        unsafe {
            match mapper.map_to_with_table_flags(page, frame, flags, table_flags, &mut allocator) {
                Ok(flush) => flush.flush(),
                Err(MapToError::PageAlreadyMapped(_)) => {}
                Err(MapToError::ParentEntryHugePage) => {}
                Err(MapToError::FrameAllocationFailed) => {
                    return Err(MapError::FrameAllocationFailed);
                }
            }
        }
    }

    let virt_ptr = (virt_start + offset as u64) as *mut u8;
    NonNull::new(virt_ptr).ok_or(MapError::InvalidPointer)
}

fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    value.div_ceil(align) * align
}

unsafe fn active_mapper(offset: VirtAddr) -> Result<OffsetPageTable<'static>, MapError> {
    let (frame, _) = Cr3::read();
    let phys = frame.start_address().as_u64();
    let table_ptr = (phys + offset.as_u64()) as *mut PageTable;
    if table_ptr.is_null() {
        return Err(MapError::InvalidPointer);
    }
    Ok(OffsetPageTable::new(&mut *table_ptr, offset))
}

struct PageTableAllocator;

unsafe impl FrameAllocator<Size4KiB> for PageTableAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        const SIZE: usize = Size4KiB::SIZE as usize;
        let (phys, virt) = dma::alloc(SIZE, SIZE)?;
        unsafe {
            core::ptr::write_bytes(virt, 0, SIZE);
        }
        Some(PhysFrame::containing_address(PhysAddr::new(phys)))
    }
}
