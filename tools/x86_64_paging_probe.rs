//! Software-only tests. These page tables are NEVER installed in CR3.
//! Ignoring flush tokens is valid only for these inactive, private tables.
#![no_std]

use x86_64::{
    VirtAddr,
    structures::paging::{
        OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
        mapper::{Mapper, MapperFlush, UnmapError, UnmappedFrame},
    },
};

/// Compile the constructor used by TRUEOS's MMIO mapper on the custom target.
///
/// # Safety
/// The caller must satisfy OffsetPageTable::from_phys_offset's requirements.
pub unsafe fn mapper(root: &mut PageTable, offset: VirtAddr) -> OffsetPageTable<'_> {
    unsafe { OffsetPageTable::from_phys_offset(root, offset) }
}

pub fn unmap(
    mapper: &mut OffsetPageTable<'_>,
    page: Page<Size4KiB>,
) -> Result<(PhysFrame<Size4KiB>, PageTableFlags, MapperFlush<Size4KiB>), UnmapError> {
    mapper.unmap(page)
}

pub fn clear(
    mapper: &mut OffsetPageTable<'_>,
    page: Page<Size4KiB>,
) -> Result<UnmappedFrame<Size4KiB>, UnmapError> {
    mapper.clear(page)
}

pub fn display(out: &mut impl core::fmt::Write, mapper: &OffsetPageTable<'_>) -> core::fmt::Result {
    write!(out, "{}", mapper.display())
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::NonNull;
    use std::{boxed::Box, string::String, vec::Vec};
    use x86_64::{
        PhysAddr,
        structures::paging::{FrameAllocator, FrameDeallocator, mapper::CleanUp},
    };

    // Own every table separately. Never use this allocator for TRUEOS's
    // aggregate GuestTables/EptTables arenas or for live hardware tables.
    #[derive(Default)]
    struct Tables(Vec<NonNull<PageTable>>);

    unsafe impl FrameAllocator<Size4KiB> for Tables {
        fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
            let ptr = NonNull::new(Box::into_raw(Box::new(PageTable::new()))).unwrap();
            self.0.push(ptr);
            Some(PhysFrame::from_start_address(PhysAddr::new(ptr.as_ptr() as u64)).unwrap())
        }
    }

    impl FrameDeallocator<Size4KiB> for Tables {
        unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
            let index = self.0.iter().position(|p| p.as_ptr() as u64 == frame.start_address().as_u64())
                .expect("attempt to free a leaf, root, shared, or unowned frame");
            let ptr = self.0.swap_remove(index);
            unsafe { drop(Box::from_raw(ptr.as_ptr())) };
        }
    }

    impl Drop for Tables {
        fn drop(&mut self) {
            for ptr in self.0.drain(..) {
                unsafe { drop(Box::from_raw(ptr.as_ptr())) };
            }
        }
    }

    fn page() -> Page<Size4KiB> {
        Page::from_start_address(VirtAddr::new(0xFFFF_FF00_0000_0000)).unwrap()
    }

    // Merely a PTE value: the test never accesses this physical frame.
    fn frame() -> PhysFrame<Size4KiB> {
        PhysFrame::from_start_address(PhysAddr::new(0x2000)).unwrap()
    }

    fn mmio_flags() -> PageTableFlags {
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE
            | PageTableFlags::NO_CACHE | PageTableFlags::WRITE_THROUGH
    }

    #[test]
    fn mmio_flags_display_unmap_and_table_ownership() {
        let mut root = Box::new(PageTable::new());
        let mut tables = Tables::default();
        {
            // In this test only, 'physical' table addresses are host pointers.
            let mut map = unsafe { mapper(&mut root, VirtAddr::zero()) };
            unsafe {
                map.map_to_with_table_flags(page(), frame(), mmio_flags(),
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE, &mut tables)
                    .unwrap().ignore();
            }
            assert_eq!(map.translate_page(page()).unwrap(), frame());
            assert_eq!(tables.0.len(), 3);
            let mut text = String::new();
            display(&mut text, &map).unwrap();
            assert!(text.contains("NO_CACHE") && text.contains("NO_EXECUTE"));
            let (old_frame, old_flags, flush) = unmap(&mut map, page()).unwrap();
            assert_eq!(old_frame, frame());
            assert_eq!(old_flags, mmio_flags());
            flush.ignore();
            assert_eq!(tables.0.len(), 3, "unmap must not free table frames");
            unsafe { map.clean_up_addr_range(Page::range_inclusive(page(), page()), &mut tables) };
            assert!(tables.0.is_empty(), "empty P1-P3 tables should be reclaimed");
        }
        assert!(root.is_empty(), "root remains owned by the test, not the deallocator");
    }

    #[test]
    fn clear_retains_information_for_non_present_entries() {
        let mut root = Box::new(PageTable::new());
        let mut tables = Tables::default();
        let mut map = unsafe { mapper(&mut root, VirtAddr::zero()) };
        let flags = PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        unsafe {
            map.map_to_with_table_flags(page(), frame(), flags,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE, &mut tables)
                .unwrap().ignore();
        }
        assert!(matches!(unmap(&mut map, page()), Err(UnmapError::PageNotMapped)));
        match clear(&mut map, page()).unwrap() {
            UnmappedFrame::NotPresent { entry } => {
                assert_eq!(entry.addr(), frame().start_address());
                assert_eq!(entry.flags(), flags);
            }
            UnmappedFrame::Present { flush, .. } => {
                flush.ignore();
                panic!("non-present entry reported as present");
            }
        }
        // No allocation has been released by clear; do not infer frame ownership.
        assert_eq!(tables.0.len(), 3);
        unsafe { map.clean_up_addr_range(Page::range_inclusive(page(), page()), &mut tables) };
        assert!(tables.0.is_empty());
    }

    #[test]
    fn encryption_feature_alone_preserves_default_address_mask() {
        // Do NOT enable the process-global encryption configuration here.
        assert!(PhysAddr::try_new((1u64 << 52) - 1).is_ok());
        assert!(PhysAddr::try_new(1u64 << 52).is_err());
        assert_eq!(PhysAddr::new_truncate((1u64 << 52) | 0x1234).as_u64(), 0x1234);
    }
}
