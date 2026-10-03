//! Dedicated, NX stack mappings with an absent page at each boundary.
//!
//! The virtual alias is first accessed on its eventual execution carrier, and
//! its owner retires it after returning to the parent stack. Safe Rust may
//! share references into a stack with other carriers. Until TRUEOS provides
//! synchronous remote TLB invalidation, retired virtual aliases are never
//! reused: no remote stale translation can become a later thread's stack.
//! Physical backing is reclaimed and cleared through HHDM before use.
//! The 512 GiB virtual arena admits 261,123 default 2 MiB stacks per boot
//! (including both guards); exhausting it rejects further allocation. Empty
//! page tables are retained, bounded by the arena's 1 GiB of leaf table frames
//! plus intermediate tables and allocator bookkeeping.

use core::ptr::NonNull;

const PAGE_BYTES: usize = 4096;
const MIN_STACK_BYTES: usize = 64 * 1024;
pub(super) const MAX_STACK_BYTES: usize = 64 * 1024 * 1024;

fn usable_bytes(bytes: usize) -> Option<usize> {
    let bytes = bytes.max(MIN_STACK_BYTES);
    if bytes > MAX_STACK_BYTES {
        return None;
    }
    bytes
        .checked_add(PAGE_BYTES - 1)
        .map(|n| n & !(PAGE_BYTES - 1))
}

pub(super) struct Stack {
    backing: backing::Mapping,
    len: usize,
}

// The scheduler may move an untouched stack to its designated carrier.
// `Mapping::ptr` checks that a stack's execution owner never migrates.
unsafe impl Send for Stack {}

impl Stack {
    pub(super) fn new(bytes: usize) -> Option<Self> {
        let len = usable_bytes(bytes)?;
        Some(Self {
            backing: backing::Mapping::new(len)?,
            len,
        })
    }

    /// The first call activates the alias on its permanent carrier. Call only
    /// from that carrier's first task poll, never from the submitting CPU.
    pub(super) fn ptr(&self) -> NonNull<u8> {
        self.backing.ptr()
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }
}

#[cfg(not(target_os = "linux"))]
mod backing {
    use super::*;
    use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const VIRTUAL_BASE: u64 = 0xFFFF_FE00_0000_0000;
    const VIRTUAL_LIMIT: u64 = 0xFFFF_FE80_0000_0000;
    const NO_CARRIER: usize = usize::MAX;
    static NEXT_VIRTUAL: AtomicU64 = AtomicU64::new(VIRTUAL_BASE);

    pub(super) struct Mapping {
        ptr: NonNull<u8>,
        phys: u64,
        len: usize,
        carrier: AtomicUsize,
    }

    impl Mapping {
        pub(super) fn new(len: usize) -> Option<Self> {
            let arena = crate::phys::reserve_heap_arena(len, PAGE_BYTES)?;
            let total = (len + 2 * PAGE_BYTES) as u64;
            let reserved = NEXT_VIRTUAL.try_update(Ordering::AcqRel, Ordering::Acquire, |next| {
                next.checked_add(total).filter(|end| *end <= VIRTUAL_LIMIT)
            });
            let base = match reserved {
                Ok(base) => base + PAGE_BYTES as u64,
                Err(_) => {
                    assert!(
                        crate::phys::free_phys_range(arena.phys_start, len),
                        "failed to release stack backing after virtual arena exhaustion"
                    );
                    return None;
                }
            };
            // Dedicated PMM pages, outside the ordinary heap allocator.
            // HHDM initialization does not touch the execution-owned alias.
            unsafe { core::ptr::write_bytes(arena.virt_start as *mut u8, 0, len) };
            match unsafe { crate::pci::mmio::map_guarded_stack(base, arena.phys_start, len) } {
                Ok(ptr) => Some(Self {
                    ptr,
                    phys: arena.phys_start,
                    len,
                    carrier: AtomicUsize::new(NO_CARRIER),
                }),
                Err(_) => {
                    assert!(
                        crate::phys::free_phys_range(arena.phys_start, len),
                        "failed to release rejected stack backing"
                    );
                    None
                }
            }
        }

        pub(super) fn ptr(&self) -> NonNull<u8> {
            let current = crate::percpu::current_slot();
            let previous = self.carrier.compare_exchange(
                NO_CARRIER,
                current,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            assert!(
                previous.is_ok() || previous == Err(current),
                "activated stack migrated between carriers"
            );
            self.ptr
        }
    }

    impl Drop for Mapping {
        fn drop(&mut self) {
            let carrier = self.carrier.load(Ordering::Acquire);
            assert!(
                carrier == NO_CARRIER || carrier == crate::percpu::current_slot(),
                "stack retired on a different carrier"
            );
            // Verification failures retain PMM backing: never
            // recycle pages while a live alias could still address them.
            unsafe {
                crate::pci::mmio::unmap_guarded_stack(self.ptr.as_ptr() as u64, self.phys, self.len)
            }
            .expect("failed to retire owned guarded stack");
            assert!(
                crate::phys::free_phys_range(self.phys, self.len),
                "failed to release retired stack backing"
            );
        }
    }
}

// Real page guards for the standalone Linux context-switch/stack tests. Linux
// performs the required TLB invalidation when mmap/mprotect/munmap change maps.
#[cfg(target_os = "linux")]
mod backing {
    use super::*;
    use core::ffi::{c_int, c_void};

    unsafe extern "C" {
        fn mmap(
            addr: *mut c_void,
            len: usize,
            prot: c_int,
            flags: c_int,
            fd: c_int,
            offset: i64,
        ) -> *mut c_void;
        fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int;
        fn munmap(addr: *mut c_void, len: usize) -> c_int;
    }

    pub(super) struct Mapping {
        reservation: NonNull<u8>,
        total: usize,
    }

    impl Mapping {
        pub(super) fn new(len: usize) -> Option<Self> {
            let total = len.checked_add(2 * PAGE_BYTES)?;
            let raw = unsafe { mmap(core::ptr::null_mut(), total, 0, 2 | 0x20, -1, 0) };
            if raw as usize == usize::MAX {
                return None;
            }
            let reservation = NonNull::new(raw.cast::<u8>())?;
            let mapping = Self { reservation, total };
            let usable = unsafe { reservation.as_ptr().add(PAGE_BYTES) };
            if unsafe { mprotect(usable.cast(), len, 1 | 2) } != 0 {
                return None;
            }
            Some(mapping)
        }

        pub(super) fn ptr(&self) -> NonNull<u8> {
            unsafe { NonNull::new_unchecked(self.reservation.as_ptr().add(PAGE_BYTES)) }
        }
    }

    impl Drop for Mapping {
        fn drop(&mut self) {
            assert_eq!(unsafe { munmap(self.reservation.as_ptr().cast(), self.total) }, 0);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    unsafe extern "C" {
        fn fork() -> i32;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        fn _exit(status: i32) -> !;
    }

    #[test]
    fn size_rounding_and_limits() {
        assert_eq!(usable_bytes(0), Some(MIN_STACK_BYTES));
        assert_eq!(usable_bytes(MIN_STACK_BYTES + 1), Some(MIN_STACK_BYTES + PAGE_BYTES));
        assert_eq!(usable_bytes(MAX_STACK_BYTES), Some(MAX_STACK_BYTES));
        assert_eq!(usable_bytes(MAX_STACK_BYTES + 1), None);
        assert_eq!(usable_bytes(usize::MAX), None);
    }

    #[test]
    fn usable_pages_are_zeroed_and_writable() {
        let stack = Stack::new(80 * 1024 + 1).unwrap();
        let bytes = unsafe { core::slice::from_raw_parts_mut(stack.ptr().as_ptr(), stack.len()) };
        assert!(bytes.iter().all(|byte| *byte == 0));
        bytes[0] = 17;
        bytes[bytes.len() - 1] = 23;
        assert_eq!(bytes[0], 17);
        assert_eq!(bytes[bytes.len() - 1], 23);
    }

    fn assert_guard_fault(address: *mut u8) {
        let child = unsafe { fork() };
        assert!(child >= 0);
        if child == 0 {
            unsafe {
                address.write_volatile(1);
                _exit(0);
            }
        }
        let mut status = 0;
        assert_eq!(unsafe { waitpid(child, &mut status, 0) }, child);
        assert_eq!(status & 0x7f, 11, "guard access did not raise SIGSEGV");
    }

    #[test]
    fn both_boundaries_fault_before_touching_other_memory() {
        let stack = Stack::new(MIN_STACK_BYTES).unwrap();
        assert_guard_fault(unsafe { stack.ptr().as_ptr().sub(1) });
        assert_guard_fault(unsafe { stack.ptr().as_ptr().add(stack.len()) });
    }
}
