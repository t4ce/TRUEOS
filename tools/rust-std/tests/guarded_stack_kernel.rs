//! Kernel stack ownership tests with PMM and page-map fixtures.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
static CPU: AtomicUsize = AtomicUsize::new(0);
static FAIL_ALLOCATION: AtomicBool = AtomicBool::new(false);
static FAIL_MAPPING: AtomicBool = AtomicBool::new(false);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);
static UNMAPPED: AtomicUsize = AtomicUsize::new(0);

mod percpu {
    pub fn current_slot() -> usize {
        super::CPU.load(super::Ordering::SeqCst)
    }
}

mod phys {
    pub struct HeapArena {
        pub phys_start: u64,
        pub virt_start: usize,
    }
    pub fn reserve_heap_arena(len: usize, align: usize) -> Option<HeapArena> {
        if super::FAIL_ALLOCATION.swap(false, super::Ordering::SeqCst) {
            return None;
        }
        let layout = std::alloc::Layout::from_size_align(len, align).unwrap();
        // Deliberately dirty: the real stack backend must clear backing itself.
        let pointer = unsafe { std::alloc::alloc(layout) };
        assert!(!pointer.is_null());
        unsafe { pointer.write_bytes(0xa5, len) };
        super::ALLOCATED.fetch_add(1, super::Ordering::SeqCst);
        Some(HeapArena {
            phys_start: pointer as u64,
            virt_start: pointer as usize,
        })
    }
    pub fn free_phys_range(physical: u64, len: usize) -> bool {
        let layout = std::alloc::Layout::from_size_align(len, 4096).unwrap();
        unsafe { std::alloc::dealloc(physical as *mut u8, layout) };
        super::FREED.fetch_add(1, super::Ordering::SeqCst);
        true
    }
}

mod pci {
    pub mod mmio {
        use std::collections::HashMap;
        use std::ptr::NonNull;
        use std::sync::{Mutex, OnceLock};
        static MAPPINGS: OnceLock<Mutex<HashMap<u64, (u64, usize)>>> = OnceLock::new();
        #[derive(Debug)]
        pub struct Error;
        pub unsafe fn map_guarded_stack(
            base: u64,
            physical: u64,
            len: usize,
        ) -> Result<NonNull<u8>, Error> {
            if super::super::FAIL_MAPPING.swap(false, super::super::Ordering::SeqCst) {
                return Err(Error);
            }
            let bytes = unsafe { std::slice::from_raw_parts(physical as *const u8, len) };
            assert!(bytes.iter().all(|byte| *byte == 0));
            assert_eq!(base % 4096, 0);
            assert_eq!(physical % 4096, 0);
            assert_eq!(len % 4096, 0);
            let old = MAPPINGS
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .unwrap()
                .insert(base, (physical, len));
            assert!(old.is_none());
            Ok(NonNull::new(base as *mut u8).unwrap())
        }
        pub unsafe fn unmap_guarded_stack(
            base: u64,
            physical: u64,
            len: usize,
        ) -> Result<(), Error> {
            assert_eq!(
                MAPPINGS.get().unwrap().lock().unwrap().remove(&base),
                Some((physical, len))
            );
            super::super::UNMAPPED.fetch_add(1, super::super::Ordering::SeqCst);
            Ok(())
        }
    }
}

#[test]
fn dedicated_backing_is_zeroed_and_retired_without_reusing_virtual_alias() {
    let freed = FREED.load(Ordering::SeqCst);
    let unmapped = UNMAPPED.load(Ordering::SeqCst);
    let first = stack::Stack::new(100_000).unwrap();
    let first_address = first.ptr().as_ptr() as usize;
    assert_eq!(first.len(), 102_400);
    drop(first);
    assert_eq!(FREED.load(Ordering::SeqCst), freed + 1);
    assert_eq!(UNMAPPED.load(Ordering::SeqCst), unmapped + 1);
    let second = stack::Stack::new(200_000).unwrap();
    assert_ne!(second.ptr().as_ptr() as usize, first_address);
    drop(second);
}

#[test]
fn simultaneous_stacks_reserve_distinct_guarded_windows() {
    let first = stack::Stack::new(65_536).unwrap();
    let second = stack::Stack::new(65_536).unwrap();
    assert_ne!(first.ptr(), second.ptr());
}

#[test]
fn mapping_rejection_rolls_back_physical_memory() {
    let allocated = ALLOCATED.load(Ordering::SeqCst);
    let freed = FREED.load(Ordering::SeqCst);
    FAIL_MAPPING.store(true, Ordering::SeqCst);
    assert!(stack::Stack::new(65_536).is_none());
    assert_eq!(ALLOCATED.load(Ordering::SeqCst), allocated + 1);
    assert_eq!(FREED.load(Ordering::SeqCst), freed + 1);
    let retry = stack::Stack::new(65_536).unwrap();
    drop(retry);
}

#[test]
fn allocation_rejection_does_not_leak_backing() {
    FAIL_ALLOCATION.store(true, Ordering::SeqCst);
    assert!(stack::Stack::new(65_536).is_none());
    let retry = stack::Stack::new(65_536).unwrap();
    drop(retry);
}

#[test]
fn activated_stack_rejects_access_from_another_carrier() {
    CPU.store(3, Ordering::SeqCst);
    let stack = stack::Stack::new(65_536).unwrap();
    stack.ptr();
    CPU.store(4, Ordering::SeqCst);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stack.ptr())).is_err());
    CPU.store(3, Ordering::SeqCst);
    drop(stack);
    CPU.store(0, Ordering::SeqCst);
}

#[test]
fn untouched_rejected_task_can_retire_on_another_carrier() {
    CPU.store(3, Ordering::SeqCst);
    let stack = stack::Stack::new(65_536).unwrap();
    CPU.store(4, Ordering::SeqCst);
    drop(stack);
    CPU.store(0, Ordering::SeqCst);
}
