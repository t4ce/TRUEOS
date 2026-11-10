#!/usr/bin/env python3
"""Exercise the production VM stack mapper with private-heap/page-table adapters.

Only heap allocation, physical translation, the root table and publication lock
are supplied by the Linux harness. Production ownership, tree walking, table
growth, mapping, retirement and rollback execute unchanged in both profiles.
"""

from pathlib import Path
import os
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
#![allow(dead_code, unexpected_cfgs)]
extern crate alloc;
extern crate self as spin;

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}

#[macro_export]
macro_rules! log_warn { (target: $target:literal; $($args:tt)*) => {{ let _ = format_args!($($args)*); }}; }

mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 2; } }
mod r {
    #[path = "@ROOT@/src/r/static_slots.rs"]
    pub mod static_slots;
}

mod test_platform {
    use super::Mutex;
    use std::alloc::{Layout, alloc_zeroed};
    use std::collections::BTreeMap;
    use std::sync::{OnceLock, atomic::{AtomicBool, AtomicU64, Ordering}};
    pub static SERIAL: Mutex<()> = Mutex::new(());
    pub static PAGE_TABLE_LOCK: Mutex<()> = Mutex::new(());
    pub const HEAP_BYTES: usize = 512 * 1024 * 1024;
    const TABLE_LIMIT: usize = 128 * 1024 * 1024;
    pub struct Heap {
        pub base: usize,
        next: usize,
        live: BTreeMap<usize, usize>,
        free: Vec<usize>,
        allocations_before_failure: Option<usize>,
    }
    impl Heap {
        fn new() -> Self {
            let base = unsafe { alloc_zeroed(Layout::from_size_align(HEAP_BYTES, 4096).unwrap()) } as usize;
            assert_ne!(base, 0);
            Self { base, next: 0, live: BTreeMap::new(), free: Vec::new(), allocations_before_failure: None }
        }
        pub fn allocate(&mut self, layout: Layout) -> *mut u8 {
            assert_eq!(layout.size(), 4096);
            assert_eq!(layout.align(), 4096);
            if let Some(remaining) = &mut self.allocations_before_failure {
                if *remaining == 0 { return std::ptr::null_mut(); }
                *remaining -= 1;
            }
            let offset = self.free.pop().unwrap_or_else(|| {
                let offset = self.next;
                self.next += 4096;
                assert!(self.next <= TABLE_LIMIT);
                offset
            });
            let pointer = self.base + offset;
            assert!(self.live.insert(pointer, 4096).is_none());
            pointer as *mut u8
        }
        pub fn deallocate(&mut self, pointer: usize) {
            assert_eq!(self.live.remove(&pointer), Some(4096), "table allocation must retire exactly once");
            // Poison retirement: following a freed page table in any later
            // test would expose an invalid parent/leaf immediately.
            unsafe { std::ptr::write_bytes(pointer as *mut u8, 0xa5, 4096) };
            self.free.push(pointer - self.base);
        }
    }
    pub fn heaps() -> &'static [Mutex<Heap>; 2] {
        static HEAPS: OnceLock<[Mutex<Heap>; 2]> = OnceLock::new();
        HEAPS.get_or_init(|| [Mutex::new(Heap::new()), Mutex::new(Heap::new())])
    }
    static NEXT_ALIAS: AtomicU64 = AtomicU64::new(0xffff_fe00_0000_0000);
    static GENERATIONS: [AtomicU64; 2] = [AtomicU64::new(1), AtomicU64::new(1)];
    static FAIL_TRANSLATION: AtomicBool = AtomicBool::new(false);
    pub fn fresh_alias(bytes: usize) -> u64 { NEXT_ALIAS.fetch_add((bytes + 8192) as u64, Ordering::SeqCst) + 4096 }
    pub fn generation(vm: u8) -> Option<u64> { GENERATIONS.get(vm as usize).map(|g| g.load(Ordering::SeqCst)) }
    pub fn next_generation(vm: u8) { GENERATIONS[vm as usize].fetch_add(1, Ordering::SeqCst); }
    pub fn stack_physical(vm: u8) -> u64 { (heaps()[vm as usize].lock().base + 256 * 1024 * 1024) as u64 }
    pub fn live_pages(vm: u8) -> usize { heaps()[vm as usize].lock().live.len() }
    pub fn live_bytes(vm: u8) -> usize { heaps()[vm as usize].lock().live.values().sum() }
    pub fn fail_after_allocations(vm: u8, count: usize) { heaps()[vm as usize].lock().allocations_before_failure = Some(count); }
    pub fn reset_allocator_faults(vm: u8) { heaps()[vm as usize].lock().allocations_before_failure = None; }
    pub fn fail_physical_translation(value: bool) { FAIL_TRANSLATION.store(value, Ordering::SeqCst); }
    pub fn physical(pointer: *const u8) -> Option<u64> {
        if FAIL_TRANSLATION.load(Ordering::SeqCst) { return None; }
        let pointer = pointer as usize;
        heaps().iter().any(|heap| {
            let heap = heap.lock();
            pointer >= heap.base && pointer < heap.base + HEAP_BYTES
        }).then_some(pointer as u64)
    }
    pub fn root_entry(vm: u8) -> u64 {
        let _lock = PAGE_TABLE_LOCK.lock();
        let tables = super::hv::memory::guest_tables_ptr_for_vm(vm).unwrap();
        unsafe { (*tables).pml4.0[super::hv::memory::pml4_index(0xffff_fe00_0000_0000)] }
    }
    pub fn set_root_entry(vm: u8, value: u64) {
        let _lock = PAGE_TABLE_LOCK.lock();
        let tables = super::hv::memory::guest_tables_ptr_for_vm(vm).unwrap();
        unsafe { (*tables).pml4.0[super::hv::memory::pml4_index(0xffff_fe00_0000_0000)] = value; }
    }
}

mod allocators {
    use crate::test_platform;
    pub struct HeapStats { pub heap_start: usize, pub heap_end: usize, pub phys_start: usize, pub initialized: bool }
    pub fn hv_guest_heap_stats_if_configured(vm: u8) -> Option<HeapStats> {
        let heap = test_platform::heaps().get(vm as usize)?.lock();
        Some(HeapStats { heap_start: heap.base, heap_end: heap.base + test_platform::HEAP_BYTES,
            phys_start: heap.base, initialized: true })
    }
    pub unsafe fn alloc_raw_hv_guest(vm: u8, layout: core::alloc::Layout) -> *mut u8 {
        test_platform::heaps()[vm as usize].lock().allocate(layout)
    }
    pub unsafe fn dealloc_raw(pointer: *mut u8) {
        let pointer = pointer as usize;
        for heap in test_platform::heaps() {
            let mut heap = heap.lock();
            if pointer >= heap.base && pointer < heap.base + test_platform::HEAP_BYTES {
                heap.deallocate(pointer); return;
            }
        }
        panic!("deallocation outside a private VM heap");
    }
    pub fn with_host_alloc_domain_strong<R>(f: impl FnOnce() -> R) -> R { f() }
}
mod phys { pub fn virt_to_phys_checked(pointer: *const u8) -> Option<u64> { crate::test_platform::physical(pointer) } }
mod pci { pub mod mmio {
    pub fn with_page_table_lock<R>(f: impl FnOnce() -> R) -> R { let _guard = crate::test_platform::PAGE_TABLE_LOCK.lock(); f() }
} }
mod hv {
    pub fn vm_run_generation(vm: u8) -> Option<u64> { crate::test_platform::generation(vm) }
    pub mod memory {
        use crate::Mutex;
        use crate::r::static_slots::StaticSlots;
        pub const PAGE_SIZE_4K: usize = 4096;
        const PT_ENTRY_PRESENT: u64 = 1;
        const PT_ENTRY_WRITABLE: u64 = 2;
        const PT_ENTRY_LARGE_PAGE: u64 = 128;
        const PT_ENTRY_NO_EXECUTE: u64 = 1 << 63;
        #[repr(C, align(4096))]
        pub struct GuestPage(pub [u64; 512]);
        pub struct GuestTables { pub pml4: GuestPage }
        pub fn pml4_index(address: u64) -> usize { ((address >> 39) & 511) as usize }
        fn pdpt_index(address: u64) -> usize { ((address >> 30) & 511) as usize }
        fn pd_index(address: u64) -> usize { ((address >> 21) & 511) as usize }
        fn pt_index(address: u64) -> usize { ((address >> 12) & 511) as usize }
        pub fn guest_tables_ptr_for_vm(vm: u8) -> Result<*mut GuestTables, &'static str> {
            static TABLES: std::sync::OnceLock<[usize; 2]> = std::sync::OnceLock::new();
            let pointers = TABLES.get_or_init(|| core::array::from_fn(|_| Box::into_raw(Box::new(GuestTables { pml4: GuestPage([0; 512]) })) as usize));
            pointers.get(vm as usize).map(|pointer| *pointer as *mut GuestTables).ok_or("unsupported VM")
        }
        #[path = "@ROOT@/src/hv/memory/thread_stack.rs"]
        pub mod thread_stack;
    }
}
'''


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    with tempfile.TemporaryDirectory(prefix="trueos-stack-tables-") as directory:
        work = Path(directory)
        source = work / "harness.rs"
        source.write_text(HARNESS.replace("@ROOT@", str(ROOT)))
        env = dict(os.environ)
        for profile, optimization in [("debug", "0"), ("optimized", "3")]:
            executable = work / profile
            subprocess.run(["rustup", "run", channel.group(1), "rustc", "--edition=2024", "--test",
                            "--cfg", "thread_stack_harness", "--target", "x86_64-unknown-linux-gnu",
                            "-C", f"opt-level={optimization}", str(source), "-o", str(executable)], check=True, env=env)
            print(f"thread-stack-table profile={profile}", flush=True)
            subprocess.run([str(executable), "--test-threads=1", "--nocapture"], check=True, env=env)


if __name__ == "__main__":
    main()
