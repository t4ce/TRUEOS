//! Host page tables for one VM-owned synchronous thread.
//!
//! The Hull and its host-carried threads retain identical main-stack
//! pointers. Only its stack and communication-page branches replace the
//! corresponding branches of the host map; kernel globals, heap, and all
//! guarded thread stacks keep their ordinary host backing.

use super::{
    GUEST_COMM_PAGE_VA, GUEST_STACK_VA_BASE, GuestPage, PAGE_SIZE_4K,
    active_guest_stack_bytes_for_vm, guest_tables_ptr_for_vm,
};
use crate::phys::HeapArena;
use x86_64::registers::control::Cr3;

#[path = "carrier/tables.rs"]
mod tables;

const TABLE_BYTES: usize = 3 * PAGE_SIZE_4K;

/// Its guest leaf-table references remain valid because the caller retains
/// the VM's accepted-job reservation until after this object is destroyed.
pub(crate) struct CarrierAddressSpace {
    arena: HeapArena,
    flags: u64,
}

// No mutable table access occurs after construction; the owning thread stays
// on its assigned carrier after its first poll.
unsafe impl Send for CarrierAddressSpace {}

impl CarrierAddressSpace {
    /// Construct after allocating the thread's guarded stack, so the shared
    /// host stack branch has already been published into the active root.
    ///
    /// # Safety
    /// The caller must hold an accepted VM-job reservation through this
    /// object's destruction. Drop only after restoring the parent's CR3.
    /// The active root must be the host map or an overlay for this same VM.
    pub(crate) unsafe fn new(vm_id: u8) -> Result<Self, &'static str> {
        let guest = guest_tables_ptr_for_vm(vm_id)?;
        let arena = crate::phys::reserve_heap_arena(TABLE_BYTES, PAGE_SIZE_4K)
            .ok_or("thread carrier tables alloc")?;
        let flags = u64::from(Cr3::read_raw().1 & 0xFFF);
        let result = crate::pci::mmio::with_page_table_lock(|| {
            let hhdm = crate::limine::hhdm_offset().ok_or("thread carrier hhdm")?;
            let (root, _) = Cr3::read();
            let parent = unsafe { &*((hhdm + root.start_address().as_u64()) as *const [u64; 512]) };
            let pages = unsafe { &mut *(arena.virt_start as *mut [GuestPage; 3]) };
            let guest_low_pd = unsafe { &(*guest).low_pd.0 };
            tables::populate(
                pages,
                arena.phys_start,
                parent,
                guest_low_pd,
                GUEST_STACK_VA_BASE,
                active_guest_stack_bytes_for_vm(vm_id),
                GUEST_COMM_PAGE_VA,
                crate::hv::vmcall::COMM_PAGE_PAGES * PAGE_SIZE_4K,
                |physical| {
                    if physical == 0 || physical & (PAGE_SIZE_4K as u64 - 1) != 0 {
                        return Err("thread carrier parent table");
                    }
                    Ok(unsafe { *((hhdm + physical) as *const [u64; 512]) })
                },
            )
        });
        if let Err(error) = result {
            assert!(
                crate::phys::free_phys_range(arena.phys_start, arena.length),
                "failed to release rejected carrier tables"
            );
            return Err(error);
        }
        Ok(Self { arena, flags })
    }

    pub(crate) fn cr3(&self) -> u64 {
        self.arena.phys_start | self.flags
    }
}

impl Drop for CarrierAddressSpace {
    fn drop(&mut self) {
        let (active, _) = Cr3::read();
        assert_ne!(
            active.start_address().as_u64(),
            self.arena.phys_start,
            "active carrier address space retired"
        );
        assert!(
            crate::phys::free_phys_range(self.arena.phys_start, self.arena.length),
            "failed to release carrier tables"
        );
    }
}
