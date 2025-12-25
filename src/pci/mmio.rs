use core::{cmp, ptr::NonNull};

use spin::Mutex;
use x86_64::{
    registers::control::Cr3,
    structures::paging::{
        mapper::{MapToError, Mapper},
        FrameAllocator, OffsetPageTable, Page, PageSize, PageTable, PageTableFlags, PhysFrame,
        Size4KiB,
    },
    PhysAddr, VirtAddr,
};

use crate::limine;

use super::dma;

const DEFAULT_MMIO_WINDOW: usize = 0x10_000;
const PAGE_SIZE: u64 = Size4KiB::SIZE;

static PAGING_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub enum MapError {
    NoHhdm,
    InvalidArgs,
    InvalidPointer,
    FrameAllocationFailed,
}

pub fn map_mmio_region(phys_base: u64, size: usize) -> Result<NonNull<u8>, MapError> {
    let requested = cmp::max(size, DEFAULT_MMIO_WINDOW);
    let hhdm = limine::hhdm_offset().ok_or(MapError::NoHhdm)?;

    let phys_start = phys_base & !(PAGE_SIZE - 1);
    let offset = (phys_base - phys_start) as usize;
    let span = requested.checked_add(offset).ok_or(MapError::InvalidArgs)? as u64;
    let total = align_up(span, PAGE_SIZE);

    let _guard = PAGING_LOCK.lock();
    let phys_offset = VirtAddr::new(hhdm);
    let mut mapper = unsafe { active_mapper(phys_offset)? };
    let mut allocator = PageTableAllocator;

    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH;
    let table_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for delta in (0..total).step_by(PAGE_SIZE as usize) {
        let phys_addr = phys_start + delta;
        let virt_addr = phys_addr + hhdm;

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

    let virt_ptr = (phys_base + hhdm) as *mut u8;
    NonNull::new(virt_ptr).ok_or(MapError::InvalidPointer)
}

fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        return value;
    }
    ((value + align - 1) / align) * align
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
