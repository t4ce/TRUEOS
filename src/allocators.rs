use core::alloc::{GlobalAlloc, Layout};
#[cfg(target_arch = "x86_64")]
use core::arch::asm;
use core::mem::{align_of, size_of};
use core::ptr::{NonNull, null_mut};
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use crate::phys::{self, HeapArena};

const HV_GUEST_HEAP_ALIGN: usize = 2 * 1024 * 1024;
pub const HV_GUEST_HEAP_MIN_ARENA_SIZE: usize = 16 * 1024 * 1024;
pub const HV_GUEST_HEAP_MAX_ARENA_SIZE: usize = 512 * 1024 * 1024;
const HV_GUEST_HEAP_CANDIDATES: [usize; 4] = [
    HV_GUEST_HEAP_MAX_ARENA_SIZE,
    256 * 1024 * 1024,
    128 * 1024 * 1024,
    64 * 1024 * 1024,
];

const ALLOC_TRACE_STAGE_ENTRY: u32 = 1;
const ALLOC_TRACE_STAGE_BLOCK: u32 = 2;
const ALLOC_TRACE_STAGE_COMPARE: u32 = 3;
const ALLOC_TRACE_STAGE_SUCCESS: u32 = 4;
const ALLOC_TRACE_STAGE_INVALID_PTR: u32 = 5;
const HV_GUEST_ALLOC_BUCKET_SHIFT: usize = 24;
const HV_GUEST_ALLOC_BUCKET_INIT: u32 = u32::MAX;

static ALLOC_TRACE_SEQ: AtomicU64 = AtomicU64::new(0);
static ALLOC_TRACE_CALLER: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_SIZE: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_ALIGN: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_STAGE: AtomicU32 = AtomicU32::new(0);
static ALLOC_TRACE_RIP1: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_RIP2: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_HEAD: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_BLOCK_PTR: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_BLOCK_SIZE: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_BLOCK_NEXT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_PAYLOAD: AtomicUsize = AtomicUsize::new(0);
static ALLOC_TRACE_ALIGNED_USED: AtomicUsize = AtomicUsize::new(0);
static HOST_HEAP_VIRT_START: AtomicUsize = AtomicUsize::new(0);
static HOST_HEAP_VIRT_END: AtomicUsize = AtomicUsize::new(0);
static HV_GUEST_HOST_DEALLOC_LOGGED: AtomicU32 = AtomicU32::new(0);
static HV_GUEST_ALLOC_FREE_BUCKET_BY_VM: [AtomicU32; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicU32::new(HV_GUEST_ALLOC_BUCKET_INIT) }; crate::allcaps::hv::VM_ID_LIMIT];

#[derive(Copy, Clone, Debug)]
pub struct AllocTrace {
    pub seq: u64,
    pub caller_rip: usize,
    pub caller_rip_1: usize,
    pub caller_rip_2: usize,
    pub layout_size: usize,
    pub layout_align: usize,
    pub stage: u32,
    pub head_ptr: usize,
    pub block_ptr: usize,
    pub block_size: usize,
    pub block_next: usize,
    pub payload_start: usize,
    pub aligned_used: usize,
}

#[inline]
unsafe fn read_return_address(depth: usize) -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let _ = depth;
        return 0;
    }

    #[cfg(target_arch = "x86_64")]
    {
        #[inline]
        fn plausible_frame_ptr(ptr: usize) -> bool {
            if ptr < 0x1000 || !ptr.is_multiple_of(core::mem::align_of::<usize>()) {
                return false;
            }
            let sign = (ptr >> 47) & 1;
            let high = ptr >> 48;
            if sign == 0 {
                high == 0
            } else {
                high == 0xFFFF
            }
        }

        let rbp: usize;
        asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack, preserves_flags));
        let mut frame = rbp as *const usize;
        let mut remaining = depth;
        while remaining != 0 {
            let frame_addr = frame as usize;
            if !plausible_frame_ptr(frame_addr) {
                return 0;
            }
            let next = *frame as usize;
            if !plausible_frame_ptr(next) || next <= frame_addr {
                return 0;
            }
            frame = next as *const usize;
            remaining -= 1;
        }
        let frame_addr = frame as usize;
        return if !plausible_frame_ptr(frame_addr) {
            0
        } else {
            *frame.add(1)
        };
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        // ARMTODO: allocator trace return-address recovery currently walks an
        // x86 frame chain via `rbp`. Non-x86 builds keep tracing enabled but
        // report unknown callers until a platform-appropriate unwind/frame
        // strategy is added.
        let _ = depth;
        0
    }
}

#[inline]
fn trace_alloc_entry(trace_enabled: bool, layout: Layout, head: Option<NonNull<FreeBlock>>) {
    if !trace_enabled {
        return;
    }
    ALLOC_TRACE_SEQ.fetch_add(1, Ordering::AcqRel);
    ALLOC_TRACE_CALLER.store(unsafe { read_return_address(2) }, Ordering::Release);
    ALLOC_TRACE_RIP1.store(unsafe { read_return_address(3) }, Ordering::Release);
    ALLOC_TRACE_RIP2.store(unsafe { read_return_address(4) }, Ordering::Release);
    ALLOC_TRACE_SIZE.store(layout.size(), Ordering::Release);
    ALLOC_TRACE_ALIGN.store(layout.align(), Ordering::Release);
    ALLOC_TRACE_STAGE.store(ALLOC_TRACE_STAGE_ENTRY, Ordering::Release);
    ALLOC_TRACE_HEAD.store(head.map(|node| node.as_ptr() as usize).unwrap_or(0), Ordering::Release);
    ALLOC_TRACE_BLOCK_PTR.store(0, Ordering::Release);
    ALLOC_TRACE_BLOCK_SIZE.store(0, Ordering::Release);
    ALLOC_TRACE_BLOCK_NEXT.store(0, Ordering::Release);
    ALLOC_TRACE_PAYLOAD.store(0, Ordering::Release);
    ALLOC_TRACE_ALIGNED_USED.store(0, Ordering::Release);
}

#[inline]
fn trace_alloc_block(
    trace_enabled: bool,
    block: &FreeBlock,
    block_start: usize,
    payload_start: usize,
    aligned_used: usize,
) {
    if !trace_enabled {
        return;
    }
    ALLOC_TRACE_STAGE.store(ALLOC_TRACE_STAGE_BLOCK, Ordering::Release);
    ALLOC_TRACE_BLOCK_PTR.store(block_start, Ordering::Release);
    ALLOC_TRACE_BLOCK_SIZE.store(block.size, Ordering::Release);
    ALLOC_TRACE_BLOCK_NEXT
        .store(block.next.map(|next| next.as_ptr() as usize).unwrap_or(0), Ordering::Release);
    ALLOC_TRACE_PAYLOAD.store(payload_start, Ordering::Release);
    ALLOC_TRACE_ALIGNED_USED.store(aligned_used, Ordering::Release);
}

#[inline]
fn trace_alloc_compare(trace_enabled: bool) {
    if !trace_enabled {
        return;
    }
    ALLOC_TRACE_STAGE.store(ALLOC_TRACE_STAGE_COMPARE, Ordering::Release);
}

#[inline]
fn trace_alloc_success(trace_enabled: bool) {
    if !trace_enabled {
        return;
    }
    ALLOC_TRACE_STAGE.store(ALLOC_TRACE_STAGE_SUCCESS, Ordering::Release);
}

#[inline]
fn publish_host_heap_range(start: usize, len: usize) {
    if start == 0 || len == 0 {
        return;
    }
    HOST_HEAP_VIRT_START.store(start, Ordering::Release);
    HOST_HEAP_VIRT_END.store(start.saturating_add(len), Ordering::Release);
}

#[inline]
pub fn host_heap_contains_addr(addr: usize) -> bool {
    let start = HOST_HEAP_VIRT_START.load(Ordering::Acquire);
    let end = HOST_HEAP_VIRT_END.load(Ordering::Acquire);
    start != 0 && end > start && addr >= start && addr < end
}

#[inline]
fn reject_hv_guest_host_heap_dealloc(ptr: *mut u8) -> bool {
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return false;
    }
    if !host_heap_contains_addr(ptr as usize) {
        return false;
    }
    if HV_GUEST_HOST_DEALLOC_LOGGED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!(
            "hv-guest-alloc: ignored host-heap dealloc ptr=0x{:X} risk=HVSR-0002\n",
            ptr as usize
        );
    }
    true
}

#[inline]
fn trace_alloc_invalid_ptr(trace_enabled: bool, block_ptr: usize) {
    if !trace_enabled {
        return;
    }
    ALLOC_TRACE_STAGE.store(ALLOC_TRACE_STAGE_INVALID_PTR, Ordering::Release);
    ALLOC_TRACE_BLOCK_PTR.store(block_ptr, Ordering::Release);
}

pub fn last_alloc_trace() -> AllocTrace {
    AllocTrace {
        seq: ALLOC_TRACE_SEQ.load(Ordering::Acquire),
        caller_rip: ALLOC_TRACE_CALLER.load(Ordering::Acquire),
        caller_rip_1: ALLOC_TRACE_RIP1.load(Ordering::Acquire),
        caller_rip_2: ALLOC_TRACE_RIP2.load(Ordering::Acquire),
        layout_size: ALLOC_TRACE_SIZE.load(Ordering::Acquire),
        layout_align: ALLOC_TRACE_ALIGN.load(Ordering::Acquire),
        stage: ALLOC_TRACE_STAGE.load(Ordering::Acquire),
        head_ptr: ALLOC_TRACE_HEAD.load(Ordering::Acquire),
        block_ptr: ALLOC_TRACE_BLOCK_PTR.load(Ordering::Acquire),
        block_size: ALLOC_TRACE_BLOCK_SIZE.load(Ordering::Acquire),
        block_next: ALLOC_TRACE_BLOCK_NEXT.load(Ordering::Acquire),
        payload_start: ALLOC_TRACE_PAYLOAD.load(Ordering::Acquire),
        aligned_used: ALLOC_TRACE_ALIGNED_USED.load(Ordering::Acquire),
    }
}

#[repr(C)]
struct FreeBlock {
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct AllocTag {
    block_start: usize,
    block_size: usize,
    domain: u8,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HeapSourceKind {
    Unconfigured,
    Arena,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AllocDomain {
    Host,
    HvGuest(u8),
}

struct FreeList {
    head: Option<NonNull<FreeBlock>>,
    initialized: bool,
    heap_virt_start: usize,
    heap_len: usize,
    heap_phys_start: usize,
    heap_source: HeapSourceKind,
}

unsafe impl Send for FreeList {}

impl FreeList {
    const fn new() -> Self {
        Self {
            head: None,
            initialized: false,
            heap_virt_start: 0,
            heap_len: 0,
            heap_phys_start: 0,
            heap_source: HeapSourceKind::Unconfigured,
        }
    }

    unsafe fn init_once(&mut self) {
        if self.initialized {
            return;
        }

        let (heap_start, heap_len) = self.ensure_heap_backing();
        let heap_end = heap_start + heap_len;

        let block_start = align_up(heap_start, align_of::<FreeBlock>());
        if block_start >= heap_end {
            return;
        }

        let size = heap_end - block_start;
        if size < minimum_block_size() {
            return;
        }

        let block = block_start as *mut FreeBlock;
        block.write(FreeBlock { size, next: None });
        self.head = Some(NonNull::new_unchecked(block));
        self.initialized = true;
    }

    unsafe fn alloc(&mut self, domain: AllocDomain, layout: Layout) -> *mut u8 {
        if !self.initialized {
            self.init_once();
        }

        let trace_enabled = matches!(domain, AllocDomain::HvGuest(_))
            || crate::hv::current_guest_execution_context_vm_id().is_none();
        let mut current = self.head;
        trace_alloc_entry(trace_enabled, layout, current);
        let mut prev: Option<NonNull<FreeBlock>> = None;

        while let Some(mut block_ptr) = current {
            if !self.is_plausible_free_block_ptr(block_ptr.as_ptr() as usize) {
                trace_alloc_invalid_ptr(trace_enabled, block_ptr.as_ptr() as usize);
                return null_mut();
            }
            let block = block_ptr.as_mut();

            let block_start = block as *mut FreeBlock as usize;

            let payload_start = match aligned_payload(block_start, layout) {
                Some(v) => v,
                None => {
                    prev = Some(block_ptr);
                    current = block.next;
                    continue;
                }
            };

            let total_used = match payload_start
                .checked_add(layout.size())
                .and_then(|end| end.checked_sub(block_start))
            {
                Some(v) => v,
                None => {
                    prev = Some(block_ptr);
                    current = block.next;
                    continue;
                }
            };

            // If we split, the next free-list node must be properly aligned for `FreeBlock`.
            // This padding is accounted to the allocated block size.
            let aligned_used = align_up(total_used, align_of::<FreeBlock>());
            trace_alloc_block(trace_enabled, block, block_start, payload_start, aligned_used);
            trace_alloc_compare(trace_enabled);

            if aligned_used > block.size {
                prev = Some(block_ptr);
                current = block.next;
                continue;
            }

            let mut remaining = block.size.saturating_sub(aligned_used);

            let next_block = if remaining >= minimum_block_size() {
                let next_start = block_start + aligned_used;
                let next_ptr = next_start as *mut FreeBlock;
                next_ptr.write(FreeBlock {
                    size: remaining,
                    next: block.next,
                });
                Some(NonNull::new_unchecked(next_ptr))
            } else {
                remaining = 0;
                block.next
            };
            let alloc_block_size = if remaining == 0 {
                block.size
            } else {
                aligned_used
            };
            block.size = alloc_block_size;

            match prev {
                Some(mut p) => p.as_mut().next = next_block,
                None => self.head = next_block,
            }

            let tag_ptr = payload_start - size_of::<AllocTag>();
            (tag_ptr as *mut AllocTag).write(AllocTag {
                block_start,
                block_size: alloc_block_size,
                domain: alloc_domain_tag(domain),
            });

            trace_alloc_success(trace_enabled);
            return payload_start as *mut u8;
        }

        null_mut()
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }

        let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
        let tag = *tag_ptr;
        let block_size = tag.block_size;
        let block_start = tag.block_start;
        if !self.is_plausible_alloc_block(block_start, block_size) {
            crate::log!(
                "alloc: ignored invalid dealloc ptr=0x{:016X} tag_block=0x{:016X} tag_size={} tag_domain={}\n",
                ptr as usize,
                block_start,
                block_size,
                tag.domain
            );
            return;
        }
        let block_ptr = block_start as *mut FreeBlock;
        block_ptr.write(FreeBlock {
            size: block_size,
            next: None,
        });

        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut current = self.head;

        while let Some(node) = current {
            if (node.as_ptr() as usize) > block_start {
                break;
            }
            prev = current;
            current = node.as_ref().next;
        }

        let mut new_node = NonNull::new_unchecked(block_ptr);
        {
            let new_block = new_node.as_mut();
            new_block.next = current;
        }

        if let Some(mut p) = prev {
            p.as_mut().next = Some(new_node);
        } else {
            self.head = Some(new_node);
        }

        self.try_merge_with_next(new_node);

        if let Some(p) = prev {
            self.try_merge_with_next(p);
        }
    }

    fn install_heap(&mut self, virt_start: usize, phys_start: usize, len: usize) {
        self.head = None;
        self.initialized = false;
        self.heap_virt_start = virt_start;
        self.heap_len = len;
        self.heap_phys_start = phys_start;
        self.heap_source = HeapSourceKind::Arena;
    }

    fn ensure_heap_backing(&mut self) -> (usize, usize) {
        (self.heap_virt_start, self.heap_len)
    }

    fn is_plausible_free_block_ptr(&mut self, ptr: usize) -> bool {
        let (heap_start, heap_len) = self.ensure_heap_backing();
        let heap_end = heap_start.saturating_add(heap_len);
        ptr >= heap_start
            && ptr.saturating_add(size_of::<FreeBlock>()) <= heap_end
            && ptr.is_multiple_of(align_of::<FreeBlock>())
    }

    fn is_plausible_alloc_block(&mut self, block_start: usize, block_size: usize) -> bool {
        let (heap_start, heap_len) = self.ensure_heap_backing();
        let heap_end = heap_start.saturating_add(heap_len);
        block_start >= heap_start
            && block_start.saturating_add(block_size) <= heap_end
            && block_size >= minimum_block_size()
            && block_start.is_multiple_of(align_of::<FreeBlock>())
    }

    unsafe fn try_merge_with_next(&mut self, mut node: NonNull<FreeBlock>) {
        let node_size = node.as_ref().size;
        let node_end = (node.as_ptr() as usize).saturating_add(node_size);

        if let Some(next_ptr) = node.as_ref().next {
            let next_start = next_ptr.as_ptr() as usize;
            if node_end == next_start {
                let next_size = next_ptr.as_ref().size;
                let next_next = next_ptr.as_ref().next;
                let new_size = node_size + next_size;
                let node_mut = node.as_mut();
                node_mut.size = new_size;
                node_mut.next = next_next;
            }
        }
    }
}

struct Allocator;

static ALLOCATOR: Mutex<FreeList> = Mutex::new(FreeList::new());
static HV_GUEST_ALLOCATORS: [Mutex<FreeList>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(FreeList::new()) }; crate::allcaps::hv::VM_ID_LIMIT];
static HV_GUEST_HEAP_READY_MASK: AtomicU64 = AtomicU64::new(0);

pub(crate) fn hv_guest_allocator_state_spans() -> [(u64, usize); 2] {
    [
        ((&HV_GUEST_ALLOCATORS as *const _) as u64, core::mem::size_of_val(&HV_GUEST_ALLOCATORS)),
        (
            (&HV_GUEST_HEAP_READY_MASK as *const _) as u64,
            core::mem::size_of_val(&HV_GUEST_HEAP_READY_MASK),
        ),
    ]
}

const HOST_ALLOC_TAG: u8 = u8::MAX;
static HOST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
static HV_GUEST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU: [AtomicU32; 64] =
    [const { AtomicU32::new(0) }; 64];
static HV_GUEST_ALLOC_DOMAIN_FORCE_VM_BY_CPU: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];

fn alloc_domain_from_tag(tag: &AllocTag) -> AllocDomain {
    if (tag.domain as usize) < crate::allcaps::hv::VM_ID_LIMIT {
        AllocDomain::HvGuest(tag.domain)
    } else {
        AllocDomain::Host
    }
}

fn alloc_domain_tag(domain: AllocDomain) -> u8 {
    match domain {
        AllocDomain::Host => HOST_ALLOC_TAG,
        AllocDomain::HvGuest(vm_id) => vm_id,
    }
}

fn alloc_domain_vm_id(domain: AllocDomain) -> Option<u8> {
    match domain {
        AllocDomain::Host => None,
        AllocDomain::HvGuest(vm_id) => Some(vm_id),
    }
}

fn cpuid_slot() -> Option<usize> {
    let slot = crate::percpu::current_slot_via_cpuid();
    if slot < 64 { Some(slot) } else { None }
}

fn current_alloc_domain() -> AllocDomain {
    if let Some(slot) = cpuid_slot()
        && HV_GUEST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU[slot].load(Ordering::Acquire) != 0
    {
        let vm_tag = HV_GUEST_ALLOC_DOMAIN_FORCE_VM_BY_CPU[slot].load(Ordering::Acquire);
        if vm_tag != 0 {
            return AllocDomain::HvGuest(vm_tag.saturating_sub(1) as u8);
        }
    }

    if let Some(slot) = cpuid_slot()
        && HOST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU[slot].load(Ordering::Acquire) != 0
    {
        return AllocDomain::Host;
    }

    // Guest-side allocator routing must prove that execution is actually on
    // the Hull guest stack. Host carriers may keep VM/vthread identity for
    // ownership and TLS, but their service allocations belong to the host heap.
    if let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() {
        return AllocDomain::HvGuest(vm_id);
    }

    if let Some(vm_id) = crate::t::kernel_task_domain::guest_owned_alloc_vm_id() {
        return AllocDomain::HvGuest(vm_id);
    }

    let slot = crate::percpu::current_slot();
    if slot >= 64 {
        return AllocDomain::Host;
    }
    AllocDomain::Host
}

pub fn with_host_alloc_domain<T>(f: impl FnOnce() -> T) -> T {
    let Some(slot) = cpuid_slot() else {
        return f();
    };
    let Some(depth) = HOST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU.get(slot) else {
        return f();
    };
    depth.fetch_add(1, Ordering::AcqRel);
    let out = f();
    depth.fetch_sub(1, Ordering::AcqRel);
    out
}

pub struct HostAllocDomainGuard {
    slot: Option<usize>,
}

impl Drop for HostAllocDomainGuard {
    fn drop(&mut self) {
        if let Some(slot) = self.slot
            && let Some(depth) = HOST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU.get(slot)
        {
            depth.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

pub fn enter_host_alloc_domain_current_cpu() -> HostAllocDomainGuard {
    let Some(slot) = cpuid_slot() else {
        return HostAllocDomainGuard { slot: None };
    };
    let Some(depth) = HOST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU.get(slot) else {
        return HostAllocDomainGuard { slot: None };
    };
    depth.fetch_add(1, Ordering::AcqRel);
    HostAllocDomainGuard { slot: Some(slot) }
}

fn allocator_for_domain(domain: AllocDomain) -> &'static Mutex<FreeList> {
    match domain {
        AllocDomain::Host => &ALLOCATOR,
        AllocDomain::HvGuest(vm_id) => HV_GUEST_ALLOCATORS
            .get(vm_id as usize)
            .unwrap_or(&HV_GUEST_ALLOCATORS[0]),
    }
}

pub fn with_hv_guest_alloc_domain<T>(vm_id: u8, f: impl FnOnce() -> T) -> Option<T> {
    if (vm_id as usize) >= crate::allcaps::hv::VM_ID_LIMIT || !ensure_hv_guest_heap_ready(vm_id) {
        return None;
    }
    let Some(slot) = cpuid_slot() else {
        return Some(crate::t::kernel_task_domain::with(
            crate::t::kernel_task_domain::KernelTaskDomain::VmGuestOwnedAlloc,
            Some(vm_id),
            f,
        ));
    };
    let depth = HV_GUEST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU.get(slot)?;
    let vm_force = HV_GUEST_ALLOC_DOMAIN_FORCE_VM_BY_CPU.get(slot)?;
    let previous_vm = vm_force.swap(vm_id as u32 + 1, Ordering::AcqRel);
    depth.fetch_add(1, Ordering::AcqRel);
    let out = crate::t::kernel_task_domain::with(
        crate::t::kernel_task_domain::KernelTaskDomain::VmGuestOwnedAlloc,
        Some(vm_id),
        f,
    );
    depth.fetch_sub(1, Ordering::AcqRel);
    vm_force.store(previous_vm, Ordering::Release);
    Some(out)
}

pub fn ensure_hv_guest_heap_ready(vm_id: u8) -> bool {
    if (vm_id as usize) >= crate::allcaps::hv::VM_ID_LIMIT {
        return false;
    }
    let ready_bit = 1u64 << vm_id;
    if (HV_GUEST_HEAP_READY_MASK.load(Ordering::Acquire) & ready_bit) != 0 {
        return true;
    }

    let mut guard = HV_GUEST_ALLOCATORS[vm_id as usize].lock();
    if guard.initialized || guard.heap_len != 0 {
        if guard.heap_source != HeapSourceKind::Arena {
            crate::log!(
                "heap: hv guest vm{} non-arena heap already live src={:?} size={} KiB; refusing readiness\n",
                vm_id,
                guard.heap_source,
                guard.heap_len / 1024
            );
            return false;
        }
        HV_GUEST_HEAP_READY_MASK.fetch_or(ready_bit, Ordering::AcqRel);
        return true;
    }

    for &size in HV_GUEST_HEAP_CANDIDATES.iter() {
        let Some(arena) = phys::reserve_heap_arena(size, HV_GUEST_HEAP_ALIGN) else {
            continue;
        };
        guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
        HV_GUEST_HEAP_READY_MASK.fetch_or(ready_bit, Ordering::AcqRel);
        crate::log!(
            "heap: hv guest vm{} arena virt=0x{:X} phys=0x{:X} size={} MiB\n",
            vm_id,
            arena.virt_start,
            arena.phys_start,
            arena.length / (1024 * 1024)
        );
        return true;
    }

    crate::log!("heap: hv guest vm{} arena unavailable; no guest fallback configured\n", vm_id);
    false
}

fn round_hv_guest_heap_request(size: usize) -> usize {
    let clamped = size
        .max(HV_GUEST_HEAP_MIN_ARENA_SIZE)
        .min(HV_GUEST_HEAP_MAX_ARENA_SIZE);
    clamped.next_multiple_of(HV_GUEST_HEAP_ALIGN)
}

pub fn prepare_hv_guest_heap_for_vm(vm_id: u8, requested_size: usize) -> bool {
    if (vm_id as usize) >= crate::allcaps::hv::VM_ID_LIMIT {
        return false;
    }

    let requested_size = round_hv_guest_heap_request(requested_size);
    let ready_bit = 1u64 << vm_id;
    let mut guard = HV_GUEST_ALLOCATORS[vm_id as usize].lock();
    if guard.initialized {
        if guard.heap_source == HeapSourceKind::Arena {
            return guard.heap_len >= requested_size;
        }
        crate::log!(
            "heap: hv guest vm{} non-arena heap already initialized src={:?} size={} KiB requested={} MiB; refusing launch\n",
            vm_id,
            guard.heap_source,
            guard.heap_len / 1024,
            requested_size / (1024 * 1024)
        );
        return false;
    }
    if guard.heap_len != 0 {
        if guard.heap_source == HeapSourceKind::Arena && guard.heap_len >= requested_size {
            HV_GUEST_HEAP_READY_MASK.fetch_or(ready_bit, Ordering::AcqRel);
            return true;
        }
        crate::log!(
            "heap: hv guest vm{} non-arena heap configured src={:?} size={} KiB requested={} MiB; refusing launch\n",
            vm_id,
            guard.heap_source,
            guard.heap_len / 1024,
            requested_size / (1024 * 1024)
        );
        return false;
    }

    let Some(arena) = phys::reserve_heap_arena(requested_size, HV_GUEST_HEAP_ALIGN) else {
        crate::log!(
            "heap: hv guest vm{} requested arena unavailable size={} MiB\n",
            vm_id,
            requested_size / (1024 * 1024)
        );
        return false;
    };
    guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    HV_GUEST_HEAP_READY_MASK.fetch_or(ready_bit, Ordering::AcqRel);
    crate::log!(
        "heap: hv guest vm{} arena virt=0x{:X} phys=0x{:X} size={} MiB requested={} MiB\n",
        vm_id,
        arena.virt_start,
        arena.phys_start,
        arena.length / (1024 * 1024),
        requested_size / (1024 * 1024)
    );
    true
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let domain = current_alloc_domain();
        let ptr = allocator_for_domain(domain).lock().alloc(domain, layout);
        if !ptr.is_null() {
            let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
            (*tag_ptr).domain = alloc_domain_tag(domain);
            if let Some(vm_id) = alloc_domain_vm_id(domain) {
                log_hv_guest_alloc_watermark(vm_id, layout, ptr, "global");
            }
        } else if let Some(vm_id) = alloc_domain_vm_id(domain) {
            log_hv_guest_alloc_failure(vm_id, layout, "global");
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }
        if reject_hv_guest_host_heap_dealloc(ptr) {
            return;
        }
        let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
        let tag = *tag_ptr;
        allocator_for_domain(alloc_domain_from_tag(&tag))
            .lock()
            .dealloc(ptr)
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: Allocator = Allocator;

pub unsafe fn alloc_raw(layout: Layout) -> *mut u8 {
    let domain = current_alloc_domain();
    let ptr = {
        let mut guard = allocator_for_domain(domain).lock();
        guard.alloc(domain, layout)
    };
    if !ptr.is_null() {
        let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
        (*tag_ptr).domain = alloc_domain_tag(domain);
        if let Some(vm_id) = alloc_domain_vm_id(domain) {
            log_hv_guest_alloc_watermark(vm_id, layout, ptr, "raw");
        }
    } else if let Some(vm_id) = alloc_domain_vm_id(domain) {
        log_hv_guest_alloc_failure(vm_id, layout, "raw");
    }
    ptr
}

fn log_hv_guest_alloc_watermark(vm_id: u8, layout: Layout, ptr: *mut u8, path: &str) {
    with_host_alloc_domain(|| {
        let Some(bucket_slot) = HV_GUEST_ALLOC_FREE_BUCKET_BY_VM.get(vm_id as usize) else {
            return;
        };
        let stats = hv_guest_heap_stats(vm_id);
        let bucket = (stats.free_bytes >> HV_GUEST_ALLOC_BUCKET_SHIFT) as u32;
        let previous = bucket_slot.swap(bucket, Ordering::AcqRel);
        let should_log = layout.size() >= 1024 * 1024
            || previous == HV_GUEST_ALLOC_BUCKET_INIT
            || bucket != previous;
        if !should_log {
            return;
        }
        let trace = last_alloc_trace();
        crate::globalog::log_with_purpose(Some("info"), format_args!(
            "hv-guest-alloc: vm{} {} ok size={} align={} ptr=0x{:016X} free_bytes={} largest_free={} free_blocks={} bucket={} prev={} trace_size={} trace_align={} caller=0x{:016X} caller1=0x{:016X} caller2=0x{:016X}\n",
            vm_id,
            path,
            layout.size(),
            layout.align(),
            ptr as usize,
            stats.free_bytes,
            stats.largest_free_block,
            stats.free_blocks,
            bucket,
            previous,
            trace.layout_size,
            trace.layout_align,
            trace.caller_rip,
            trace.caller_rip_1,
            trace.caller_rip_2,
        ));
    });
}

fn log_hv_guest_alloc_failure(vm_id: u8, layout: Layout, path: &str) {
    with_host_alloc_domain(|| {
        let stats = hv_guest_heap_stats(vm_id);
        let trace = last_alloc_trace();
        crate::globalog::log_with_purpose(Some("warn"), format_args!(
            "hv-guest-alloc: vm{} {} failed size={} align={} src={:?} usable_total={} free_bytes={} largest_free={} free_blocks={} init={}\n",
            vm_id,
            path,
            layout.size(),
            layout.align(),
            stats.source,
            stats.usable_total,
            stats.free_bytes,
            stats.largest_free_block,
            stats.free_blocks,
            stats.initialized,
        ));
        crate::globalog::log_with_purpose(Some("warn"), format_args!(
            "hv-guest-alloc: trace seq={} caller=0x{:016X} caller1=0x{:016X} caller2=0x{:016X} size={} align={} stage={} head=0x{:016X} block=0x{:016X} block_size={} next=0x{:016X} payload=0x{:016X} aligned_used={}\n",
            trace.seq,
            trace.caller_rip,
            trace.caller_rip_1,
            trace.caller_rip_2,
            trace.layout_size,
            trace.layout_align,
            trace.stage,
            trace.head_ptr,
            trace.block_ptr,
            trace.block_size,
            trace.block_next,
            trace.payload_start,
            trace.aligned_used,
        ));
    });
}

pub unsafe fn dealloc_raw(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    if reject_hv_guest_host_heap_dealloc(ptr) {
        return;
    }
    let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
    let tag = *tag_ptr;
    allocator_for_domain(alloc_domain_from_tag(&tag))
        .lock()
        .dealloc(ptr)
}

#[derive(Copy, Clone, Debug)]
pub struct HeapStats {
    pub heap_start: usize,
    pub heap_end: usize,
    pub phys_start: usize,
    pub usable_start: usize,
    pub usable_total: usize,
    pub free_bytes: usize,
    pub largest_free_block: usize,
    pub free_blocks: usize,
    pub initialized: bool,
    pub source: HeapSourceKind,
}

pub fn heap_stats() -> HeapStats {
    let mut guard = ALLOCATOR.lock();
    unsafe {
        if !guard.initialized {
            guard.init_once();
        }
    }

    let (heap_start, heap_len) = guard.ensure_heap_backing();
    let heap_end = heap_start.saturating_add(heap_len);
    let usable_start = align_up(heap_start, align_of::<FreeBlock>());
    let usable_total = heap_end.saturating_sub(usable_start);

    let mut free_bytes = 0usize;
    let mut largest_free_block = 0usize;
    let mut free_blocks = 0usize;
    let mut current = guard.head;
    while let Some(block_ptr) = current {
        // Safety: free list nodes are managed by the allocator.
        let block = unsafe { block_ptr.as_ref() };
        free_blocks += 1;
        free_bytes = free_bytes.saturating_add(block.size);
        if block.size > largest_free_block {
            largest_free_block = block.size;
        }
        current = block.next;
    }

    HeapStats {
        heap_start,
        heap_end,
        phys_start: guard.heap_phys_start,
        usable_start,
        usable_total,
        free_bytes,
        largest_free_block,
        free_blocks,
        initialized: guard.initialized,
        source: guard.heap_source,
    }
}

fn heap_stats_from_guard(guard: &mut FreeList) -> HeapStats {
    unsafe {
        if !guard.initialized {
            guard.init_once();
        }
    }

    let (heap_start, heap_len) = guard.ensure_heap_backing();
    let heap_end = heap_start.saturating_add(heap_len);
    let usable_start = align_up(heap_start, align_of::<FreeBlock>());
    let usable_total = heap_end.saturating_sub(usable_start);

    let mut free_bytes = 0usize;
    let mut largest_free_block = 0usize;
    let mut free_blocks = 0usize;
    let mut current = guard.head;
    while let Some(block_ptr) = current {
        let block = unsafe { block_ptr.as_ref() };
        free_blocks += 1;
        free_bytes = free_bytes.saturating_add(block.size);
        if block.size > largest_free_block {
            largest_free_block = block.size;
        }
        current = block.next;
    }

    HeapStats {
        heap_start,
        heap_end,
        phys_start: guard.heap_phys_start,
        usable_start,
        usable_total,
        free_bytes,
        largest_free_block,
        free_blocks,
        initialized: guard.initialized,
        source: guard.heap_source,
    }
}

pub fn hv_guest_heap_stats(vm_id: u8) -> HeapStats {
    let Some(allocator) = HV_GUEST_ALLOCATORS.get(vm_id as usize) else {
        return HeapStats {
            heap_start: 0,
            heap_end: 0,
            phys_start: 0,
            usable_start: 0,
            usable_total: 0,
            free_bytes: 0,
            largest_free_block: 0,
            free_blocks: 0,
            initialized: false,
            source: HeapSourceKind::Unconfigured,
        };
    };
    let mut guard = allocator.lock();
    heap_stats_from_guard(&mut guard)
}

pub fn hv_guest_heap_stats_if_configured(vm_id: u8) -> Option<HeapStats> {
    let allocator = HV_GUEST_ALLOCATORS.get(vm_id as usize)?;
    let mut guard = allocator.lock();
    if !guard.initialized && guard.heap_len == 0 {
        return None;
    }
    Some(heap_stats_from_guard(&mut guard))
}

pub fn hv_guest_heap_stats_total() -> HeapStats {
    let mut total = HeapStats {
        heap_start: 0,
        heap_end: 0,
        phys_start: 0,
        usable_start: 0,
        usable_total: 0,
        free_bytes: 0,
        largest_free_block: 0,
        free_blocks: 0,
        initialized: false,
        source: HeapSourceKind::Unconfigured,
    };

    for allocator in HV_GUEST_ALLOCATORS.iter() {
        let mut guard = allocator.lock();
        if !guard.initialized && guard.heap_len == 0 {
            continue;
        }
        let stats = heap_stats_from_guard(&mut guard);
        if stats.heap_start != 0 && (total.heap_start == 0 || stats.heap_start < total.heap_start) {
            total.heap_start = stats.heap_start;
        }
        total.heap_end = total.heap_end.max(stats.heap_end);
        if total.phys_start == 0 {
            total.phys_start = stats.phys_start;
        }
        if stats.usable_start != 0
            && (total.usable_start == 0 || stats.usable_start < total.usable_start)
        {
            total.usable_start = stats.usable_start;
        }
        total.usable_total = total.usable_total.saturating_add(stats.usable_total);
        total.free_bytes = total.free_bytes.saturating_add(stats.free_bytes);
        total.largest_free_block = total.largest_free_block.max(stats.largest_free_block);
        total.free_blocks = total.free_blocks.saturating_add(stats.free_blocks);
        total.initialized |= stats.initialized;
        if stats.source == HeapSourceKind::Arena {
            total.source = HeapSourceKind::Arena;
        }
    }
    total
}

pub fn install_heap_arena(arena: HeapArena) -> bool {
    if arena.length < minimum_block_size() {
        crate::log!(
            "heap: requested arena too small size={} bytes (need >= {})\n",
            arena.length,
            minimum_block_size()
        );
        return false;
    }

    let mut guard = ALLOCATOR.lock();
    if guard.initialized {
        crate::log!("heap: allocator already initialized; cannot swap backing\n");
        return false;
    }

    guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    publish_host_heap_range(arena.virt_start, arena.length);
    phys::register_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    if crate::logflag::BOOT_INFO_LOGS {
        crate::log!(
            "heap: arena virt=0x{:X} phys=0x{:X} size={} MiB\n",
            arena.virt_start,
            arena.phys_start,
            arena.length / (1024 * 1024)
        );
    }
    true
}

const fn minimum_block_size() -> usize {
    size_of::<FreeBlock>() + size_of::<AllocTag>()
}

fn align_up(addr: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (addr + mask) & !mask
}

fn aligned_payload(block_start: usize, layout: Layout) -> Option<usize> {
    let payload_align = core::cmp::max(layout.align(), align_of::<AllocTag>());
    let payload_start =
        align_up(block_start + size_of::<FreeBlock>() + size_of::<AllocTag>(), payload_align);
    if payload_start > usize::MAX - layout.size() {
        None
    } else {
        Some(payload_start)
    }
}
