//! VM-owned guarded aliases for safe references between Hull and carriers.
//! Both tables and data come from the private, already EPT-admitted VM heap.
//! Table backing grows with the monotonic alias arena, independently of the
//! fixed image table pool. Retired aliases and empty tables are never reused
//! during a VM incarnation: another carrier may retain a paging-cache entry.
//! Tables retire only after the existing offline VM execution barrier.
use super::*;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

const STACK_BASE: u64 = 0xFFFF_FE00_0000_0000;
const STACK_LIMIT: u64 = 0xFFFF_FE80_0000_0000;
const ADDRESS: u64 = 0x000F_FFFF_FFFF_F000;
const TABLE_ACCESSED: u64 = 1 << 5;
static MAP_FAILURES: AtomicUsize = AtomicUsize::new(0);

// Hardware sets Accessed on every walked parent. It does not change that
// table's frame ownership or give permission to alter any other flag.
fn same_root(current: u64, owned: u64) -> bool {
    current == owned || (owned != 0 && current & !TABLE_ACCESSED == owned)
}

struct OwnedPage {
    pointer: usize,
    physical: u64,
}

impl OwnedPage {
    fn allocate(vm: u8) -> Result<Self, &'static str> {
        let layout = core::alloc::Layout::from_size_align(PAGE_SIZE_4K, PAGE_SIZE_4K)
            .map_err(|_| "thread stack table layout")?;
        let pointer = unsafe { crate::allocators::alloc_raw_hv_guest(vm, layout) };
        if pointer.is_null() {
            return Err("thread stack table allocation");
        }
        let prepared = (|| {
            let physical =
                crate::phys::virt_to_phys_checked(pointer).ok_or("thread stack table pa")?;
            validate_heap_span(vm, physical, PAGE_SIZE_4K)?;
            unsafe { core::ptr::write_bytes(pointer, 0, PAGE_SIZE_4K) };
            Ok(Self {
                pointer: pointer as usize,
                physical,
            })
        })();
        if prepared.is_err() {
            unsafe { crate::allocators::dealloc_raw(pointer) };
        }
        prepared
    }
}

impl Drop for OwnedPage {
    fn drop(&mut self) {
        unsafe { crate::allocators::dealloc_raw(self.pointer as *mut u8) };
    }
}

struct Tables {
    generation: u64,
    root_entry: u64,
    // Physical ordering permits checked O(log n) walks. Never follow a guest
    // page-table pointer into a host page or another VM's private allocation.
    pages: Vec<OwnedPage>,
}

impl Tables {
    const fn new() -> Self {
        Self {
            generation: 0,
            root_entry: 0,
            pages: Vec::new(),
        }
    }

    fn for_generation(&mut self, generation: u64) -> Result<(), &'static str> {
        if generation == 0 {
            return Err("thread stack generation absent");
        }
        if self.pages.is_empty() {
            self.generation = generation;
        }
        if self.generation != generation {
            return Err("thread stack table generation");
        }
        Ok(())
    }

    fn page(&self, physical: u64) -> Result<*mut [u64; 512], &'static str> {
        let index = self
            .pages
            .binary_search_by_key(&physical, |page| page.physical)
            .map_err(|_| "thread stack table ownership")?;
        Ok(self.pages[index].pointer as *mut [u64; 512])
    }

    fn allocate(&mut self, vm: u8) -> Result<u64, &'static str> {
        self.pages
            .try_reserve(1)
            .map_err(|_| "thread stack table registry allocation")?;
        let page = OwnedPage::allocate(vm)?;
        let physical = page.physical;
        let index = self
            .pages
            .binary_search_by_key(&physical, |page| page.physical)
            .expect_err("VM allocator returned an already live table page");
        self.pages.insert(index, page);
        Ok(physical)
    }

    unsafe fn check_root(&self, root: *mut [u64; 512]) -> Result<(), &'static str> {
        let value = unsafe {
            core::ptr::read_volatile(core::ptr::addr_of!((*root)[pml4_index(STACK_BASE)]))
        };
        if !same_root(value, self.root_entry) {
            return Err("thread stack root ownership");
        }
        if value != 0 {
            self.page(value & ADDRESS)?;
        }
        Ok(())
    }
}

static TABLES: StaticSlots<Tables, { crate::allcaps::hv::VM_ID_LIMIT }> =
    StaticSlots::from_slots([const { Mutex::new(Tables::new()) }; crate::allcaps::hv::VM_ID_LIMIT]);

fn validate_heap_span(vm: u8, physical: u64, bytes: usize) -> Result<(), &'static str> {
    let end = physical
        .checked_add(bytes as u64)
        .ok_or("thread stack backing overflow")?;
    let heap =
        crate::allocators::hv_guest_heap_stats_if_configured(vm).ok_or("thread stack VM heap")?;
    if physical % PAGE_SIZE_4K as u64 != 0
        || !heap.initialized
        || physical < heap.phys_start as u64
        || end
            > (heap.phys_start as u64)
                .saturating_add(heap.heap_end.saturating_sub(heap.heap_start) as u64)
    {
        return Err("thread stack ownership geometry");
    }
    Ok(())
}

fn validate(vm: u8, base: u64, physical: u64, bytes: usize) -> Result<(), &'static str> {
    let end = base
        .checked_add(bytes as u64)
        .ok_or("thread stack alias overflow")?;
    if bytes == 0
        || bytes % PAGE_SIZE_4K != 0
        || base % PAGE_SIZE_4K as u64 != 0
        || base <= STACK_BASE
        || end >= STACK_LIMIT
    {
        return Err("thread stack ownership geometry");
    }
    validate_heap_span(vm, physical, bytes)
}

// Called under the page-table publication lock and this VM's pool lock.
unsafe fn leaf(
    vm: u8,
    root: *mut [u64; 512],
    va: u64,
    tables: &mut Tables,
    create: bool,
) -> Result<Option<*mut u64>, &'static str> {
    let mut table = root;
    for (level, index) in [pml4_index(va), pdpt_index(va), pd_index(va)]
        .into_iter()
        .enumerate()
    {
        let entry = unsafe { core::ptr::addr_of_mut!((*table)[index]) };
        let mut value = unsafe { core::ptr::read_volatile(entry) };
        if value & PT_ENTRY_PRESENT == 0 {
            if value != 0 {
                return Err("thread stack table flags");
            }
            if !create {
                return Ok(None);
            }
            let pa = tables.allocate(vm)?;
            value = pa | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE;
            if level == 0 {
                tables.root_entry = value;
            }
            unsafe { core::ptr::write_volatile(entry, value) };
        }
        if value & PT_ENTRY_LARGE_PAGE != 0 {
            return Err("thread stack large parent");
        }
        if value & !(ADDRESS | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE | TABLE_ACCESSED) != 0
            || value & (PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE)
                != PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE
        {
            return Err("thread stack table flags");
        }
        table = tables.page(value & ADDRESS)?;
    }
    Ok(Some(unsafe {
        core::ptr::addr_of_mut!((*table)[pt_index(va)])
    }))
}

pub(crate) fn map_thread_stack(
    vm: u8,
    base: u64,
    physical: u64,
    bytes: usize,
) -> Result<(), &'static str> {
    let result = crate::allocators::with_host_alloc_domain_strong(|| {
        (|| {
            validate(vm, base, physical, bytes)?;
            let generation = crate::hv::vm_run_generation(vm).ok_or("thread stack VM")?;
            let mut leaves = Vec::new();
            leaves
                .try_reserve_exact(bytes / PAGE_SIZE_4K)
                .map_err(|_| "thread stack leaf registry allocation")?;
            crate::pci::mmio::with_page_table_lock(|| {
                let guest = guest_tables_ptr_for_vm(vm)?;
                let root = unsafe { core::ptr::addr_of_mut!((*guest).pml4.0) };
                let mut tables = TABLES.get(vm as usize).ok_or("thread stack VM")?.lock();
                tables.for_generation(generation)?;
                unsafe {
                    tables.check_root(root)?;
                }
                // Provision first, publish leaves only after every guard/body leaf is
                // known absent. Partial table allocation leaves an empty, reusable tree.
                for va in (base - PAGE_SIZE_4K as u64..=base + bytes as u64).step_by(PAGE_SIZE_4K) {
                    let entry = unsafe { leaf(vm, root, va, &mut tables, true)? }.unwrap();
                    if unsafe { core::ptr::read_volatile(entry) } != 0 {
                        return Err("thread stack alias conflict");
                    }
                    if va >= base && va < base + bytes as u64 {
                        leaves.push(entry);
                    }
                }
                // Every fallible operation and ownership check precedes publication.
                // Rejected admission cannot leave leaves targeting freed stack data.
                for (page, entry) in leaves.into_iter().enumerate() {
                    let delta = page * PAGE_SIZE_4K;
                    unsafe {
                        core::ptr::write_volatile(
                            entry,
                            physical + delta as u64
                                | PT_ENTRY_PRESENT
                                | PT_ENTRY_WRITABLE
                                | PT_ENTRY_NO_EXECUTE,
                        )
                    };
                }
                Ok(())
            })
        })()
    });
    if let Err(reason) = result {
        let sequence = MAP_FAILURES.fetch_add(1, Ordering::Relaxed);
        if sequence < 32 || sequence % 64 == 0 {
            crate::log_warn!(target: "blueprint";
                "thread-stack-map vm={} reason={} stack_bytes={} failure_seq={}\n",
                vm, reason, bytes, sequence);
        }
    }
    result
}

pub(crate) fn unmap_thread_stack(
    vm: u8,
    base: u64,
    physical: u64,
    bytes: usize,
) -> Result<(), &'static str> {
    validate(vm, base, physical, bytes)?;
    crate::pci::mmio::with_page_table_lock(|| {
        let tables = guest_tables_ptr_for_vm(vm)?;
        let root = unsafe { core::ptr::addr_of_mut!((*tables).pml4.0) };
        let generation = crate::hv::vm_run_generation(vm).ok_or("thread stack VM")?;
        let mut tables = TABLES.get(vm as usize).ok_or("thread stack VM")?.lock();
        tables.for_generation(generation)?;
        unsafe {
            tables.check_root(root)?;
        }
        for delta in (0..bytes).step_by(PAGE_SIZE_4K) {
            let entry = unsafe { leaf(vm, root, base + delta as u64, &mut tables, false)? }
                .ok_or("thread stack alias absent")?;
            let value = unsafe { core::ptr::read_volatile(entry) };
            if value & PT_ENTRY_PRESENT == 0 || value & ADDRESS != physical + delta as u64 {
                return Err("thread stack alias ownership");
            }
        }
        for delta in (0..bytes).step_by(PAGE_SIZE_4K) {
            let entry =
                unsafe { leaf(vm, root, base + delta as u64, &mut tables, false)? }.unwrap();
            unsafe { core::ptr::write_volatile(entry, 0) };
        }
        // The globally monotonic alias is never reused; no live reference may
        // remain when the owning thread finishes. Empty tables stay VM-owned.
        Ok(())
    })
}

/// Reattach the same incarnation's retained branch after rebuilding its CR3.
pub(super) unsafe fn restore_root_for_vm(
    vm: u8,
    root: *mut [u64; 512],
) -> Result<(), &'static str> {
    crate::pci::mmio::with_page_table_lock(|| {
        let tables = TABLES.get(vm as usize).ok_or("thread stack VM")?.lock();
        if tables.pages.is_empty() {
            return Ok(());
        }
        if crate::hv::vm_run_generation(vm) != Some(tables.generation) {
            return Err("thread stack table generation");
        }
        tables.page(tables.root_entry & ADDRESS)?;
        let entry = unsafe { core::ptr::addr_of_mut!((*root)[pml4_index(STACK_BASE)]) };
        let current = unsafe { core::ptr::read_volatile(entry) };
        if current != 0 && !same_root(current, tables.root_entry) {
            return Err("thread stack root ownership");
        }
        unsafe { core::ptr::write_volatile(entry, tables.root_entry) };
        Ok(())
    })
}

/// Release all table frames before releasing the private VM heap.
///
/// # Safety
/// The VM must be offline, admission closed, and every Hull/native execution
/// owner and carrier retired. Paused continuations do not satisfy this barrier.
/// No retained CR3 may still access these tables after this call.
pub(crate) unsafe fn release_thread_stack_tables_for_vm(vm: u8) -> Result<(), &'static str> {
    crate::allocators::with_host_alloc_domain_strong(|| {
        let retired = crate::pci::mmio::with_page_table_lock(|| {
            let mut tables = TABLES.get(vm as usize).ok_or("thread stack VM")?.lock();
            if tables.pages.is_empty() {
                return Ok(Vec::new());
            }
            tables.page(tables.root_entry & ADDRESS)?;
            let guest = guest_tables_ptr_for_vm(vm)?;
            let entry = unsafe { core::ptr::addr_of_mut!((*guest).pml4.0[pml4_index(STACK_BASE)]) };
            let current = unsafe { core::ptr::read_volatile(entry) };
            if current != 0 && !same_root(current, tables.root_entry) {
                return Err("thread stack root ownership");
            }
            unsafe { core::ptr::write_volatile(entry, 0) };
            tables.root_entry = 0;
            tables.generation = 0;
            Ok(core::mem::take(&mut tables.pages))
        })?;
        // The whole subtree is unreachable before its first allocation is
        // returned. Host registry and VM page backing drop in their own realms.
        drop(retired);
        Ok(())
    })
}

#[cfg(all(test, thread_stack_harness))]
#[path = "thread_stack/tests.rs"]
mod tests;
