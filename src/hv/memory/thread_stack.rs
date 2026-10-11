//! VM-owned guarded aliases for safe references between Hull and carriers.
//! Tables come from the VM's existing EPT-admitted table pool; data comes
//! from its private heap. No host or other VM stack branch is imported.
use super::*;

const STACK_BASE: u64 = 0xFFFF_FE00_0000_0000;
const STACK_LIMIT: u64 = 0xFFFF_FE80_0000_0000;
const ADDRESS: u64 = 0x000F_FFFF_FFFF_F000;

fn validate(vm: u8, base: u64, physical: u64, bytes: usize) -> Result<(), &'static str> {
    let end = base.checked_add(bytes as u64).ok_or("thread stack alias overflow")?;
    let physical_end = physical.checked_add(bytes as u64).ok_or("thread stack backing overflow")?;
    let heap = crate::allocators::hv_guest_heap_stats_if_configured(vm).ok_or("thread stack VM heap")?;
    if bytes == 0 || bytes % PAGE_SIZE_4K != 0 || base % PAGE_SIZE_4K as u64 != 0
        || physical % PAGE_SIZE_4K as u64 != 0 || base <= STACK_BASE || end >= STACK_LIMIT
        || !heap.initialized || physical < heap.phys_start as u64
        || physical_end > (heap.phys_start as u64).saturating_add(heap.heap_end.saturating_sub(heap.heap_start) as u64) {
        return Err("thread stack ownership geometry");
    }
    Ok(())
}

// Called under the page-table publication lock and this VM's pool lock.
unsafe fn leaf(vm: u8, root: *mut [u64;512], va: u64, next: &mut usize, create: bool)
    -> Result<Option<*mut u64>, &'static str> {
    let mut table = root;
    for index in [pml4_index(va), pdpt_index(va), pd_index(va)] {
        let entry = unsafe { core::ptr::addr_of_mut!((*table)[index]) };
        let mut value = unsafe { core::ptr::read_volatile(entry) };
        if value & PT_ENTRY_PRESENT == 0 {
            if !create { return Ok(None); }
            if *next >= GUEST_HIGH_IMAGE_PT_COUNT { return Err("thread stack table pool"); }
            let tables = guest_tables_ptr_for_vm(vm)?;
            let page = unsafe { core::ptr::addr_of_mut!((*tables).image_pts[*next].0) };
            unsafe { zero_guest_page(page) };
            let pa = host_va_to_pa(page as u64).ok_or("thread stack table pa")?;
            *next += 1;
            value = pa | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE;
            unsafe { core::ptr::write_volatile(entry, value) };
        }
        if value & PT_ENTRY_LARGE_PAGE != 0 { return Err("thread stack large parent"); }
        table = crate::phys::phys_to_virt((value & ADDRESS) as usize) as *mut [u64;512];
    }
    Ok(Some(unsafe { core::ptr::addr_of_mut!((*table)[pt_index(va)]) }))
}

pub(crate) fn map_thread_stack(vm: u8, base: u64, physical: u64, bytes: usize) -> Result<(), &'static str> {
    validate(vm, base, physical, bytes)?;
    crate::pci::mmio::with_page_table_lock(|| {
        let tables = guest_tables_ptr_for_vm(vm)?;
        let root = unsafe { core::ptr::addr_of_mut!((*tables).pml4.0) };
        let mut next = GUEST_DYNAMIC_PT_NEXT.get(vm as usize).ok_or("thread stack VM")?.lock();
        // Provision first, publish leaves only after every guard/body leaf is
        // known absent. Partial table allocation leaves an empty, reusable tree.
        for va in (base - PAGE_SIZE_4K as u64 ..= base + bytes as u64).step_by(PAGE_SIZE_4K) {
            let entry = unsafe { leaf(vm, root, va, &mut next, true)? }.unwrap();
            if unsafe { core::ptr::read_volatile(entry) } != 0 { return Err("thread stack alias conflict"); }
        }
        for delta in (0..bytes).step_by(PAGE_SIZE_4K) {
            let entry = unsafe { leaf(vm, root, base + delta as u64, &mut next, false)? }.unwrap();
            unsafe { core::ptr::write_volatile(entry, physical + delta as u64
                | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE | PT_ENTRY_NO_EXECUTE) };
        }
        Ok(())
    })
}

pub(crate) fn unmap_thread_stack(vm: u8, base: u64, physical: u64, bytes: usize) -> Result<(), &'static str> {
    validate(vm, base, physical, bytes)?;
    crate::pci::mmio::with_page_table_lock(|| {
        let tables = guest_tables_ptr_for_vm(vm)?;
        let root = unsafe { core::ptr::addr_of_mut!((*tables).pml4.0) };
        let mut unused = 0;
        for delta in (0..bytes).step_by(PAGE_SIZE_4K) {
            let entry = unsafe { leaf(vm, root, base + delta as u64, &mut unused, false)? }
                .ok_or("thread stack alias absent")?;
            let value = unsafe { core::ptr::read_volatile(entry) };
            if value & PT_ENTRY_PRESENT == 0 || value & ADDRESS != physical + delta as u64 {
                return Err("thread stack alias ownership");
            }
        }
        for delta in (0..bytes).step_by(PAGE_SIZE_4K) {
            let entry = unsafe { leaf(vm, root, base + delta as u64, &mut unused, false)? }.unwrap();
            unsafe { core::ptr::write_volatile(entry, 0) };
        }
        // The globally monotonic alias is never reused; no live reference may
        // remain when the owning thread finishes. Empty tables stay VM-owned.
        Ok(())
    })
}
