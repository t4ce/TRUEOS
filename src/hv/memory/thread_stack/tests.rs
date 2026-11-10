use super::*;
use crate::test_platform as platform;

const STACK_BYTES: usize = 16 * 1024 * 1024;

fn start(vm: u8) {
    unsafe {
        release_thread_stack_tables_for_vm(vm).unwrap();
    }
    platform::next_generation(vm);
    platform::reset_allocator_faults(vm);
}

fn map(vm: u8) -> u64 {
    let base = platform::fresh_alias(STACK_BYTES);
    map_thread_stack(vm, base, platform::stack_physical(vm), STACK_BYTES).unwrap();
    base
}

fn value(vm: u8, address: u64) -> u64 {
    let _publication = crate::test_platform::PAGE_TABLE_LOCK.lock();
    let guest = guest_tables_ptr_for_vm(vm).unwrap();
    let root = unsafe { core::ptr::addr_of_mut!((*guest).pml4.0) };
    let mut tables = TABLES[vm as usize].lock();
    unsafe {
        leaf(vm, root, address, &mut tables, false)
            .unwrap()
            .map(|entry| core::ptr::read_volatile(entry))
            .unwrap_or(0)
    }
}

#[test]
fn two_thousand_serial_large_stacks_outlive_the_image_table_pool() {
    let _serial = platform::SERIAL.lock();
    start(0);
    let physical = platform::stack_physical(0);
    let mut previous = 0;
    for sequence in 0..2048 {
        let base = map(0);
        assert!(base > previous);
        previous = base;
        assert_eq!(value(0, base - PAGE_SIZE_4K as u64), 0);
        assert_eq!(value(0, base + STACK_BYTES as u64), 0);
        assert_eq!(
            value(0, base),
            physical | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE | PT_ENTRY_NO_EXECUTE
        );
        assert_eq!(
            value(0, base + STACK_BYTES as u64 - PAGE_SIZE_4K as u64) & ADDRESS,
            physical + STACK_BYTES as u64 - PAGE_SIZE_4K as u64
        );
        unmap_thread_stack(0, base, physical, STACK_BYTES).unwrap();
        assert_eq!(value(0, base), 0);
        if sequence > 128 {
            assert!(platform::live_pages(0) > 1024);
        }
        assert_eq!(
            platform::live_bytes(0),
            platform::live_pages(0) * PAGE_SIZE_4K
        );
    }
    // One 16 MiB body is reused only after retirement; metadata grows by
    // about 32 KiB per lifetime stack, never by another 16 MiB stack body.
    assert!(platform::live_pages(0) > 16_384);
    assert!(platform::live_bytes(0) < 70 * 1024 * 1024);
    let retained = platform::live_pages(0);
    assert_eq!(retained, TABLES[0].lock().pages.len());
    unsafe {
        release_thread_stack_tables_for_vm(0).unwrap();
    }
    assert_eq!(platform::live_pages(0), 0);
    assert_eq!(platform::root_entry(0), 0);
    assert!(TABLES[0].lock().pages.is_empty());
    println!(
        "thread-stack-churn: lifetimes=2048 live_stack_bytes={} retained_metadata_pages={} freed_on_vm_retirement=1",
        STACK_BYTES, retained
    );
}

#[test]
fn allocation_failure_at_every_initial_level_leaves_no_stack_leaves() {
    let _serial = platform::SERIAL.lock();
    for accepted_pages in 0..3 {
        start(0);
        platform::fail_after_allocations(0, accepted_pages);
        let base = platform::fresh_alias(STACK_BYTES);
        assert_eq!(
            map_thread_stack(0, base, platform::stack_physical(0), STACK_BYTES),
            Err("thread stack table allocation")
        );
        assert_eq!(value(0, base), 0);
        assert_eq!(value(0, base + STACK_BYTES as u64 - PAGE_SIZE_4K as u64), 0);
        assert_eq!(platform::live_pages(0), accepted_pages);
        platform::reset_allocator_faults(0);
        // A failed alias is never assigned to a later stack. Empty owned
        // tables remain valid and can cover fresh addresses in their region.
        let fresh = map(0);
        assert!(fresh > base);
        unmap_thread_stack(0, fresh, platform::stack_physical(0), STACK_BYTES).unwrap();
        unsafe {
            release_thread_stack_tables_for_vm(0).unwrap();
        }
        assert_eq!(platform::live_pages(0), 0);
    }
}

#[test]
fn later_table_failure_rolls_back_admission_before_publishing_any_body_page() {
    let _serial = platform::SERIAL.lock();
    start(0);
    platform::fail_after_allocations(0, 6);
    let base = platform::fresh_alias(STACK_BYTES);
    assert_eq!(
        map_thread_stack(0, base, platform::stack_physical(0), STACK_BYTES),
        Err("thread stack table allocation")
    );
    for delta in (0..STACK_BYTES).step_by(PAGE_SIZE_4K) {
        assert_eq!(value(0, base + delta as u64), 0);
    }
    assert_eq!(platform::live_pages(0), 6);
    platform::reset_allocator_faults(0);
    let fresh = map(0);
    unmap_thread_stack(0, fresh, platform::stack_physical(0), STACK_BYTES).unwrap();
    unsafe {
        release_thread_stack_tables_for_vm(0).unwrap();
    }
    assert_eq!(platform::live_pages(0), 0);
}

#[test]
fn rejected_physical_translation_returns_unpublished_allocation() {
    let _serial = platform::SERIAL.lock();
    start(0);
    platform::fail_physical_translation(true);
    let base = platform::fresh_alias(STACK_BYTES);
    assert_eq!(
        map_thread_stack(0, base, platform::stack_physical(0), STACK_BYTES),
        Err("thread stack table pa")
    );
    platform::fail_physical_translation(false);
    assert_eq!(platform::live_pages(0), 0);
    assert_eq!(platform::root_entry(0), 0);
}

#[test]
fn wrong_vm_data_and_foreign_table_roots_cannot_steal_aliases_or_release_pages() {
    let _serial = platform::SERIAL.lock();
    start(0);
    start(1);
    let first = map(0);
    let second = map(1);
    let first_root = platform::root_entry(0);
    let second_root = platform::root_entry(1);
    assert_ne!(first_root, second_root);
    let base = platform::fresh_alias(STACK_BYTES);
    assert_eq!(
        map_thread_stack(1, base, platform::stack_physical(0), STACK_BYTES),
        Err("thread stack ownership geometry")
    );
    let first_pages = platform::live_pages(0);
    platform::set_root_entry(0, second_root);
    assert_eq!(
        map_thread_stack(0, base, platform::stack_physical(0), STACK_BYTES),
        Err("thread stack root ownership")
    );
    assert_eq!(
        unsafe { release_thread_stack_tables_for_vm(0) },
        Err("thread stack root ownership")
    );
    assert_eq!(platform::live_pages(0), first_pages);
    assert_ne!(value(1, second), 0);
    platform::set_root_entry(0, first_root);
    unmap_thread_stack(0, first, platform::stack_physical(0), STACK_BYTES).unwrap();
    unmap_thread_stack(1, second, platform::stack_physical(1), STACK_BYTES).unwrap();
    unsafe {
        release_thread_stack_tables_for_vm(0).unwrap();
        release_thread_stack_tables_for_vm(1).unwrap();
    }
    assert_eq!(platform::live_pages(0) + platform::live_pages(1), 0);
}

#[test]
fn failed_ownership_validation_preserves_every_live_stack_leaf() {
    let _serial = platform::SERIAL.lock();
    start(0);
    let first = map(0);
    let physical = platform::stack_physical(0);
    assert_eq!(
        map_thread_stack(0, first, physical, STACK_BYTES),
        Err("thread stack alias conflict")
    );
    assert_eq!(
        unmap_thread_stack(0, first, physical + PAGE_SIZE_4K as u64, STACK_BYTES),
        Err("thread stack alias ownership")
    );
    assert_eq!(value(0, first) & ADDRESS, physical);
    assert_eq!(
        value(0, first + STACK_BYTES as u64 - PAGE_SIZE_4K as u64) & ADDRESS,
        physical + STACK_BYTES as u64 - PAGE_SIZE_4K as u64
    );
    unmap_thread_stack(0, first, physical, STACK_BYTES).unwrap();
    unsafe {
        release_thread_stack_tables_for_vm(0).unwrap();
    }
}

#[test]
fn retained_branch_rebinds_only_in_its_own_generation_and_cleanup_allows_restart() {
    let _serial = platform::SERIAL.lock();
    start(0);
    let base = map(0);
    let branch = platform::root_entry(0);
    platform::set_root_entry(0, 0);
    let guest = guest_tables_ptr_for_vm(0).unwrap();
    let root = unsafe { core::ptr::addr_of_mut!((*guest).pml4.0) };
    unsafe {
        restore_root_for_vm(0, root).unwrap();
    }
    assert_eq!(platform::root_entry(0), branch);
    assert_ne!(value(0, base), 0);
    unmap_thread_stack(0, base, platform::stack_physical(0), STACK_BYTES).unwrap();
    let pages = platform::live_pages(0);
    platform::next_generation(0);
    assert_eq!(
        map_thread_stack(
            0,
            platform::fresh_alias(STACK_BYTES),
            platform::stack_physical(0),
            STACK_BYTES
        ),
        Err("thread stack table generation")
    );
    assert_eq!(platform::live_pages(0), pages);
    platform::set_root_entry(0, 0);
    assert_eq!(
        unsafe { restore_root_for_vm(0, root) },
        Err("thread stack table generation")
    );
    unsafe {
        release_thread_stack_tables_for_vm(0).unwrap();
    }
    assert_eq!(platform::live_pages(0), 0);
    let next = map(0);
    assert!(next > base);
    unmap_thread_stack(0, next, platform::stack_physical(0), STACK_BYTES).unwrap();
    unsafe {
        release_thread_stack_tables_for_vm(0).unwrap();
    }
    assert_eq!(platform::live_pages(0), 0);
}

#[test]
fn hardware_accessed_and_leaf_dirty_bits_preserve_owned_mapping_lifecycle() {
    let _serial = platform::SERIAL.lock();
    start(0);
    let first = map(0);
    let physical = platform::stack_physical(0);
    let root = platform::root_entry(0);
    platform::set_root_entry(0, root | TABLE_ACCESSED);
    {
        let tables = TABLES[0].lock();
        for page in &tables.pages {
            let entries = page.pointer as *mut [u64; 512];
            for index in 0..512 {
                let entry = unsafe { core::ptr::addr_of_mut!((*entries)[index]) };
                let value = unsafe { core::ptr::read_volatile(entry) };
                if value & PT_ENTRY_PRESENT != 0 {
                    // Leaf data entries can additionally gain Dirty; parent
                    // entries gain Accessed only.
                    let flags = TABLE_ACCESSED
                        | if value & PT_ENTRY_NO_EXECUTE != 0 {
                            1 << 6
                        } else {
                            0
                        };
                    unsafe { core::ptr::write_volatile(entry, value | flags) };
                }
            }
        }
    }
    let second = map(0);
    unmap_thread_stack(0, first, physical, STACK_BYTES).unwrap();
    unmap_thread_stack(0, second, physical, STACK_BYTES).unwrap();
    // A same-frame root with a different permission or Large bit remains a
    // conflict. Cleanup must retain backing until ownership is restored.
    for unexpected in [1 << 2, 1 << 6, PT_ENTRY_LARGE_PAGE, PT_ENTRY_NO_EXECUTE] {
        platform::set_root_entry(0, root | TABLE_ACCESSED | unexpected);
        assert_eq!(
            unsafe { release_thread_stack_tables_for_vm(0) },
            Err("thread stack root ownership")
        );
        assert!(platform::live_pages(0) > 0);
    }
    platform::set_root_entry(0, root | TABLE_ACCESSED);
    let guest = guest_tables_ptr_for_vm(0).unwrap();
    let root_pointer = unsafe { core::ptr::addr_of_mut!((*guest).pml4.0) };
    unsafe {
        restore_root_for_vm(0, root_pointer).unwrap();
    }
    platform::set_root_entry(0, root | TABLE_ACCESSED);
    unsafe {
        release_thread_stack_tables_for_vm(0).unwrap();
    }
    assert_eq!(platform::live_pages(0), 0);
    assert_eq!(platform::root_entry(0), 0);
}
