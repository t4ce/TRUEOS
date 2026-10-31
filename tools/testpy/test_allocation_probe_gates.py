#!/usr/bin/env python3
"""Check production allocator and ABI probe gates in four diagnostic images.

The actual free list, trace publishers, and thread ABI admission function are
compiled unchanged. Host arenas replace kernel backing, safe return-address
stubs count stack inspection, and a tiny diagnostic owner counts ABI admission.
Both flags are varied independently; allocator policy and locking are unchanged.
No kernel boot, network, full build, or application data is needed.
"""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def item(source: str, declaration: str) -> str:
    match = re.search(declaration, source, re.MULTILINE)
    if match is None:
        raise ValueError(f"missing production item: {declaration}")
    brace = source.index("{", match.end())
    ending = re.search(r"^}\s*\n", source[brace:], re.MULTILINE)
    if ending is None:
        raise ValueError(f"missing production closing brace: {declaration}")
    return source[match.start():brace + ending.end()]


HARNESS = r'''#![allow(dead_code, unsafe_op_in_unsafe_fn)]
extern crate alloc;
use core::alloc::Layout;
use core::mem::{align_of, size_of};
use core::ptr::{NonNull, null_mut};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

mod allcaps {
    pub mod hv {
        pub const CODEX_PROGRESS_PROBES_ENABLED: bool = true;
        pub const HV_ALLOC_DETAIL_PROBES_ENABLED: bool = cfg!(detail_probes_enabled);
        pub const CODEX_ALLOC_ABI_PROBES_ENABLED: bool = cfg!(alloc_abi_probes_enabled);
    }
}

#[macro_export]
macro_rules! log { ($($args:tt)*) => {}; }

mod hv {
    use super::*;
    pub static GUEST_CONTEXT: AtomicBool = AtomicBool::new(false);
    pub fn current_guest_execution_context_vm_id() -> Option<u8> {
        GUEST_CONTEXT.load(Ordering::Relaxed).then_some(1)
    }
}

static STACK_READS: AtomicUsize = AtomicUsize::new(0);
unsafe fn read_return_address(depth: usize) -> usize {
    STACK_READS.fetch_add(1, Ordering::Relaxed);
    0x1000 + depth
}
fn alloc_domain_tag(domain: AllocDomain) -> u8 {
    match domain { AllocDomain::Host => 0, AllocDomain::HvGuest(vm) => vm + 1 }
}
fn alloc_domain_from_tag(tag: &AllocTag) -> Option<AllocDomain> {
    Some(if tag.domain == 0 { AllocDomain::Host } else { AllocDomain::HvGuest(tag.domain - 1) })
}
fn log_alloc_domain_mismatch(_: *mut u8, _: Option<AllocDomain>, _: Option<u8>) {
    panic!("allocation domain mismatch")
}
fn release_hv_guest_dma_pin(_: HvGuestDmaPin) -> bool { true }

@TRACE@
@FREE_LIST@
@LAYOUT_HELPERS@

mod diagnostics {
    use super::*;
    @ABI_OPERATIONS@

    pub static ADMISSIONS: AtomicUsize = AtomicUsize::new(0);
    pub struct AbiGuard { pub active: bool }
    impl AbiGuard { pub fn inactive() -> Self { Self { active: false } } }
    pub struct Diagnostic;
    impl Diagnostic {
        pub unsafe fn abi_scope(&self, _: AbiOperation) -> AbiGuard {
            ADMISSIONS.fetch_add(1, Ordering::Relaxed);
            AbiGuard { active: true }
        }
    }
}

mod thread_boundary {
    use super::*;
    struct Owner { diagnostic: diagnostics::Diagnostic }
    struct Admission { _owner: Option<Owner> }
    struct Thread { timed: bool, _admission: Admission }
    static THREAD: Thread = Thread {
        timed: true,
        _admission: Admission { _owner: Some(Owner { diagnostic: diagnostics::Diagnostic }) },
    };
    pub static ACTIVE_READS: AtomicUsize = AtomicUsize::new(0);
    fn active() -> *mut Thread {
        ACTIVE_READS.fetch_add(1, Ordering::Relaxed);
        &THREAD as *const Thread as *mut Thread
    }

    @ABI_SCOPE@
}

struct Heap {
    storage: NonNull<u8>,
    layout: Layout,
    list: FreeList,
}
impl Heap {
    fn new() -> Self {
        let layout = Layout::from_size_align(64 * 1024, 4096).unwrap();
        let storage = NonNull::new(unsafe { std::alloc::alloc(layout) }).unwrap();
        let mut list = FreeList::new();
        list.install_heap(storage.as_ptr() as usize, 0, layout.size());
        unsafe { list.init_once() };
        Self { storage, layout, list }
    }
}
impl Drop for Heap {
    fn drop(&mut self) { unsafe { std::alloc::dealloc(self.storage.as_ptr(), self.layout) }; }
}

fn trace_state() -> [usize; 13] {
    let trace = last_alloc_trace();
    [trace.seq as usize, trace.caller_rip, trace.caller_rip_1, trace.caller_rip_2,
        trace.layout_size, trace.layout_align, trace.stage as usize, trace.head_ptr,
        trace.block_ptr, trace.block_size, trace.block_next, trace.payload_start,
        trace.aligned_used]
}

fn seed_trace() -> [usize; 13] {
    hv::GUEST_CONTEXT.store(false, Ordering::Relaxed);
    let block = FreeBlock { size: 4096, next: Some(NonNull::dangling()) };
    trace_alloc_entry(true, Layout::from_size_align(40, 32).unwrap(), Some(NonNull::dangling()));
    trace_alloc_block(true, &block, 0xAB00, 0xAC00, 256);
    trace_alloc_compare(true);
    trace_alloc_success(true);
    STACK_READS.store(0, Ordering::Relaxed);
    trace_state()
}

#[test]
fn disabled_publishers_leave_every_shared_trace_field_and_stack_untouched() {
    let expected = seed_trace();
    let block = FreeBlock { size: 1024, next: None };
    trace_alloc_entry(false, Layout::from_size_align(80, 8).unwrap(), None);
    trace_alloc_block(false, &block, 0xDD00, 0xDE00, 80);
    trace_alloc_compare(false);
    trace_alloc_success(false);
    trace_alloc_invalid_ptr(false, 0xFFFF);
    assert_eq!(trace_state(), expected);
    assert_eq!(STACK_READS.load(Ordering::Relaxed), 0);
}

#[test]
fn guest_success_and_failure_follow_detail_gate_and_keep_coarse_work() {
    let expected = seed_trace();
    let mut heap = Heap::new();
    let allocated = unsafe { heap.list.alloc(AllocDomain::HvGuest(1), Layout::from_size_align(80, 8).unwrap()) };
    assert!(!allocated.is_null());
    let failed = unsafe { heap.list.alloc(AllocDomain::HvGuest(1), Layout::from_size_align(128 * 1024, 8).unwrap()) };
    assert!(failed.is_null());
    assert_eq!(heap.list.alloc_calls, 2);
    assert_eq!(heap.list.alloc_nodes, 2);
    assert_eq!(heap.list.alloc_max_nodes, 1);
    assert_eq!(heap.list.alloc_failures, 1);
    if allcaps::hv::HV_ALLOC_DETAIL_PROBES_ENABLED {
        let trace = last_alloc_trace();
        assert_eq!(trace.seq as usize, expected[0] + 2);
        assert_eq!(trace.layout_size, 128 * 1024);
        assert_eq!(trace.stage, ALLOC_TRACE_STAGE_COMPARE);
        assert_eq!(STACK_READS.load(Ordering::Relaxed), 6);
    } else {
        assert_eq!(trace_state(), expected);
        assert_eq!(STACK_READS.load(Ordering::Relaxed), 0);
    }
}

#[test]
fn ordinary_host_allocations_retain_their_detail_snapshot() {
    let expected = seed_trace();
    let mut heap = Heap::new();
    let pointer = unsafe { heap.list.alloc(AllocDomain::Host, Layout::from_size_align(80, 8).unwrap()) };
    assert!(!pointer.is_null());
    let trace = last_alloc_trace();
    assert_eq!(trace.seq as usize, expected[0] + 1);
    assert_eq!(trace.layout_size, 80);
    assert_eq!(trace.stage, ALLOC_TRACE_STAGE_SUCCESS);
    assert_eq!(STACK_READS.load(Ordering::Relaxed), 3);
}

#[test]
fn host_allocation_inside_guest_context_keeps_existing_suppression() {
    let expected = seed_trace();
    hv::GUEST_CONTEXT.store(true, Ordering::Relaxed);
    let mut heap = Heap::new();
    let pointer = unsafe { heap.list.alloc(AllocDomain::Host, Layout::from_size_align(80, 8).unwrap()) };
    assert!(!pointer.is_null());
    assert_eq!(trace_state(), expected);
    assert_eq!(STACK_READS.load(Ordering::Relaxed), 0);
}

#[test]
fn allocation_breadcrumb_gate_returns_before_thread_identity_or_admission() {
    diagnostics::ADMISSIONS.store(0, Ordering::Relaxed);
    thread_boundary::ACTIVE_READS.store(0, Ordering::Relaxed);
    for operation in [diagnostics::AbiOperation::Alloc, diagnostics::AbiOperation::Dealloc] {
        let guard = unsafe { thread_boundary::abi_scope(operation) };
        assert_eq!(guard.active, allcaps::hv::CODEX_ALLOC_ABI_PROBES_ENABLED);
    }
    let expected = if allcaps::hv::CODEX_ALLOC_ABI_PROBES_ENABLED { 2 } else { 0 };
    assert_eq!(diagnostics::ADMISSIONS.load(Ordering::Relaxed), expected);
    assert_eq!(thread_boundary::ACTIVE_READS.load(Ordering::Relaxed), expected);
}

#[test]
fn other_abi_breadcrumbs_remain_admitted_in_every_image() {
    diagnostics::ADMISSIONS.store(0, Ordering::Relaxed);
    thread_boundary::ACTIVE_READS.store(0, Ordering::Relaxed);
    for operation in [diagnostics::AbiOperation::Mutex, diagnostics::AbiOperation::Condvar,
        diagnostics::AbiOperation::Stat, diagnostics::AbiOperation::Mkdir,
        diagnostics::AbiOperation::Wait, diagnostics::AbiOperation::Sleep,
        diagnostics::AbiOperation::Yield] {
        assert!(unsafe { thread_boundary::abi_scope(operation) }.active);
    }
    assert_eq!(diagnostics::ADMISSIONS.load(Ordering::Relaxed), 7);
    assert_eq!(thread_boundary::ACTIVE_READS.load(Ordering::Relaxed), 7);
}
'''


def main() -> None:
    allcaps = (ROOT / "src/allcaps.rs").read_text()
    for name in ("HV_ALLOC_DETAIL_PROBES_ENABLED", "CODEX_ALLOC_ABI_PROBES_ENABLED"):
        assert re.search(rf"pub const {name}: bool = false;", allcaps), name
    allocator = (ROOT / "src/allocators.rs").read_text()
    trace_declarations = "\n".join(re.findall(
        r"^(?:const|static) ALLOC_TRACE_[^;]+;", allocator, re.MULTILINE
    ))
    trace = trace_declarations + "\n" + item(allocator, r"^pub struct AllocTrace\b")
    for name in ("trace_alloc_entry", "trace_alloc_block", "trace_alloc_compare",
                 "trace_alloc_success", "trace_alloc_invalid_ptr", "last_alloc_trace"):
        trace += "\n" + item(allocator, rf"^(?:pub )?fn {name}\b")
    start = allocator.index("#[repr(C)]\nstruct FreeBlock")
    end = allocator.index("\n#[cfg(test)]\nmod tests;", start)
    abi = (ROOT / "src/r/blocking/abi_breadcrumb.rs").read_text()
    operations = abi[abi.index("#[derive(Clone, Copy)]"):abi.index("pub(super) struct State")]
    source = HARNESS.replace("@TRACE@", trace)
    source = source.replace("@FREE_LIST@", allocator[start:end])
    source = source.replace("@LAYOUT_HELPERS@", allocator[allocator.index("const fn minimum_block_size()"):])
    source = source.replace("@ABI_OPERATIONS@", operations)
    source = source.replace("@ABI_SCOPE@", item(
        (ROOT / "src/r/threads.rs").read_text(), r"^pub\(crate\) unsafe fn abi_scope\b"
    ))
    with tempfile.TemporaryDirectory(prefix="trueos-allocation-probe-gates-") as temporary:
        directory = Path(temporary)
        harness = directory / "harness.rs"
        harness.write_text(source)
        for detail in (False, True):
            for breadcrumbs in (False, True):
                binary = directory / f"detail-{int(detail)}-abi-{int(breadcrumbs)}"
                flags = []
                if detail:
                    flags.extend(("--cfg", "detail_probes_enabled"))
                if breadcrumbs:
                    flags.extend(("--cfg", "alloc_abi_probes_enabled"))
                print(f"detail={int(detail)} allocation_abi={int(breadcrumbs)}", flush=True)
                subprocess.run([
                    "rustc", "--edition=2024", "--test", *flags, str(harness), "-o", str(binary),
                ], check=True, timeout=60)
                subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=30)


if __name__ == "__main__":
    main()
