//! Production address-space ownership against PMM and control-register fixtures.
#![allow(dead_code)]
extern crate self as x86_64;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
static CURRENT: AtomicU64 = AtomicU64::new(0);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);

pub mod registers {
    pub mod control {
        pub struct Frame(pub u64);
        impl Frame {
            pub fn start_address(&self) -> Address {
                Address(self.0)
            }
        }
        pub struct Address(pub u64);
        impl Address {
            pub fn as_u64(&self) -> u64 {
                self.0
            }
        }
        pub struct Cr3;
        impl Cr3 {
            pub fn read() -> (Frame, ()) {
                (Frame(super::super::CURRENT.load(super::super::Ordering::SeqCst) & !0xfff), ())
            }
            pub fn read_raw() -> (Frame, u16) {
                let raw = super::super::CURRENT.load(super::super::Ordering::SeqCst);
                (Frame(raw & !0xfff), (raw & 0xfff) as u16)
            }
        }
    }
}

mod phys {
    #[derive(Copy, Clone)]
    pub struct HeapArena {
        pub phys_start: u64,
        pub virt_start: usize,
        pub length: usize,
    }
    pub fn reserve_heap_arena(len: usize, align: usize) -> Option<HeapArena> {
        let layout = std::alloc::Layout::from_size_align(len, align).unwrap();
        let pointer = unsafe { std::alloc::alloc(layout) };
        assert!(!pointer.is_null());
        super::ALLOCATED.fetch_add(1, super::Ordering::SeqCst);
        Some(HeapArena {
            phys_start: pointer as u64,
            virt_start: pointer as usize,
            length: len,
        })
    }
    pub fn free_phys_range(physical: u64, len: usize) -> bool {
        let layout = std::alloc::Layout::from_size_align(len, 4096).unwrap();
        unsafe { std::alloc::dealloc(physical as *mut u8, layout) };
        super::FREED.fetch_add(1, super::Ordering::SeqCst);
        true
    }
}

mod limine {
    pub fn hhdm_offset() -> Option<u64> {
        Some(0)
    }
}
mod pci {
    pub mod mmio {
        pub fn with_page_table_lock<T>(f: impl FnOnce() -> T) -> T {
            f()
        }
    }
}
mod hv {
    pub mod vmcall {
        pub const COMM_PAGE_PAGES: usize = 2;
    }
    pub mod memory {
        #[repr(C, align(4096))]
        #[derive(Copy, Clone)]
        pub struct GuestPage(pub [u64; 512]);
        pub const PAGE_SIZE_4K: usize = 4096;
        pub const GUEST_STACK_VA_BASE: u64 = 0x400000;
        pub const GUEST_COMM_PAGE_VA: u64 = 0x20400000;
        pub struct GuestTables {
            pub low_pd: GuestPage,
        }
        pub fn active_guest_stack_bytes_for_vm(_: u8) -> usize {
            8 * 1024 * 1024
        }
        pub fn guest_tables_ptr_for_vm(_: u8) -> Result<*mut GuestTables, &'static str> {
            static TABLES: super::super::OnceLock<usize> = super::super::OnceLock::new();
            let pointer = *TABLES.get_or_init(|| {
                Box::into_raw(Box::new(GuestTables {
                    low_pd: GuestPage(core::array::from_fn(|i| 0x100000 + i as u64 * 4096 | 3)),
                })) as usize
            });
            Ok(pointer as *mut GuestTables)
        }
        pub mod carrier;
    }
}

fn initialize() -> u64 {
    static ROOT: OnceLock<u64> = OnceLock::new();
    let parent = *ROOT.get_or_init(|| {
        let layout = std::alloc::Layout::from_size_align(4096, 4096).unwrap();
        let pointer = unsafe { std::alloc::alloc_zeroed(layout) };
        assert!(!pointer.is_null());
        pointer as u64
    });
    CURRENT.store(parent | 0x018, Ordering::SeqCst);
    parent
}

#[test]
fn carrier_retirement_frees_exactly_its_three_pages_after_parent_restore() {
    let parent = initialize();
    let before = FREED.load(Ordering::SeqCst);
    let address = unsafe { hv::memory::carrier::CarrierAddressSpace::new(0) }.unwrap();
    assert_ne!(address.cr3() & !0xfff, parent);
    assert_eq!(address.cr3() & 0xfff, 0x018);
    let root = address.cr3();
    CURRENT.store(root, Ordering::SeqCst);
    CURRENT.store(parent | 0x018, Ordering::SeqCst);
    drop(address);
    assert_eq!(FREED.load(Ordering::SeqCst), before + 1);
}

#[test]
fn rejected_guest_branch_reclaims_all_owned_table_pages() {
    initialize();
    let guest = hv::memory::guest_tables_ptr_for_vm(0).unwrap();
    let before = FREED.load(Ordering::SeqCst);
    let prior = unsafe { (*guest).low_pd.0[2] };
    unsafe { (*guest).low_pd.0[2] = 0 };
    let failed = unsafe { hv::memory::carrier::CarrierAddressSpace::new(0) };
    unsafe { (*guest).low_pd.0[2] = prior };
    assert!(matches!(failed, Err("thread carrier guest branch absent")));
    assert_eq!(FREED.load(Ordering::SeqCst), before + 1);
}

#[test]
fn active_root_cannot_be_freed() {
    let parent = initialize();
    let address = unsafe { hv::memory::carrier::CarrierAddressSpace::new(0) }.unwrap();
    let root = address.cr3();
    let before = FREED.load(Ordering::SeqCst);
    CURRENT.store(root, Ordering::SeqCst);
    assert!(std::panic::catch_unwind(|| drop(address)).is_err());
    assert_eq!(FREED.load(Ordering::SeqCst), before);
    CURRENT.store(parent | 0x018, Ordering::SeqCst);
    assert!(phys::free_phys_range(root & !0xfff, 3 * 4096));
}
