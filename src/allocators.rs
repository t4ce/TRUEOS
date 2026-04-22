use core::alloc::{GlobalAlloc, Layout};
#[cfg(target_arch = "x86_64")]
use core::arch::asm;
use core::mem::{align_of, size_of};
use core::ptr::{NonNull, addr_of_mut, null_mut};
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use crate::phys::{self, HeapArena};

pub const FALLBACK_HEAP_SIZE: usize = 256 * 1024;
pub const HV_GUEST_HEAP_FALLBACK_SIZE: usize = 256 * 1024;
const HV_GUEST_HEAP_ALIGN: usize = 2 * 1024 * 1024;
const HV_GUEST_HEAP_CANDIDATES: [usize; 4] = [
    128 * 1024 * 1024,
    64 * 1024 * 1024,
    32 * 1024 * 1024,
    16 * 1024 * 1024,
];

static mut FALLBACK_HEAP: [u8; FALLBACK_HEAP_SIZE] = [0; FALLBACK_HEAP_SIZE];
static mut HV_GUEST_FALLBACK_HEAP: [u8; HV_GUEST_HEAP_FALLBACK_SIZE] =
    [0; HV_GUEST_HEAP_FALLBACK_SIZE];

const ALLOC_TRACE_STAGE_ENTRY: u32 = 1;
const ALLOC_TRACE_STAGE_BLOCK: u32 = 2;
const ALLOC_TRACE_STAGE_COMPARE: u32 = 3;
const ALLOC_TRACE_STAGE_SUCCESS: u32 = 4;
const ALLOC_TRACE_STAGE_INVALID_PTR: u32 = 5;

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
    #[cfg(target_arch = "x86_64")]
    {
    let rbp: usize;
        asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack, preserves_flags));
        let mut frame = rbp as *const usize;
        let mut remaining = depth;
        while remaining != 0 {
            if frame.is_null() {
                return 0;
            }
            frame = (*frame) as *const usize;
            remaining -= 1;
        }
        return if frame.is_null() { 0 } else { *frame.add(1) };
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
    Fallback,
    Arena,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AllocDomain {
    Host = 0,
    HvGuest = 1,
}

struct FreeList {
    head: Option<NonNull<FreeBlock>>,
    initialized: bool,
    heap_virt_start: usize,
    heap_len: usize,
    heap_phys_start: usize,
    heap_source: HeapSourceKind,
    fallback_virt_start: usize,
    fallback_len: usize,
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
            heap_source: HeapSourceKind::Fallback,
            fallback_virt_start: 0,
            fallback_len: 0,
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

        let trace_enabled = true;
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
                domain: domain as u8,
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
        self.heap_virt_start = virt_start;
        self.heap_len = len;
        self.heap_phys_start = phys_start;
        self.heap_source = HeapSourceKind::Arena;
    }

    fn ensure_heap_backing(&mut self) -> (usize, usize) {
        if self.heap_len == 0 {
            let start = self.fallback_virt_start;
            let len = self.fallback_len;
            if start == 0 || len == 0 {
                return (0, 0);
            }
            self.heap_virt_start = start;
            self.heap_len = len;
            self.heap_phys_start = 0;
            self.heap_source = HeapSourceKind::Fallback;
        }
        (self.heap_virt_start, self.heap_len)
    }

    fn is_plausible_free_block_ptr(&mut self, ptr: usize) -> bool {
        let (heap_start, heap_len) = self.ensure_heap_backing();
        let heap_end = heap_start.saturating_add(heap_len);
        ptr >= heap_start
            && ptr.saturating_add(size_of::<FreeBlock>()) <= heap_end
            && ptr.is_multiple_of(align_of::<FreeBlock>())
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
static HV_GUEST_ALLOCATOR: Mutex<FreeList> = Mutex::new(FreeList::new());
static HV_GUEST_ACTIVE_CPU_MASK: AtomicU64 = AtomicU64::new(0);
static HV_GUEST_HEAP_READY: AtomicU32 = AtomicU32::new(0);

fn alloc_domain_from_tag(tag: &AllocTag) -> AllocDomain {
    match tag.domain {
        x if x == AllocDomain::HvGuest as u8 => AllocDomain::HvGuest,
        _ => AllocDomain::Host,
    }
}

fn current_alloc_domain() -> AllocDomain {
    // During the first Hull guest entry, avoid CPUID/slot discovery entirely.
    // The guest shares the same image as the host, so the host-side boot-armed
    // flag is visible here and is enough to route early allocations to the
    // guest allocator without touching percpu discovery.
    if crate::hv::guest_boot_active() {
        return AllocDomain::HvGuest;
    }
    let slot = crate::percpu::current_slot_via_cpuid();
    if slot >= 64 {
        return AllocDomain::Host;
    }
    if (HV_GUEST_ACTIVE_CPU_MASK.load(Ordering::Acquire) & (1u64 << slot)) != 0 {
        AllocDomain::HvGuest
    } else {
        AllocDomain::Host
    }
}

fn allocator_for_domain(domain: AllocDomain) -> &'static Mutex<FreeList> {
    match domain {
        AllocDomain::Host => &ALLOCATOR,
        AllocDomain::HvGuest => &HV_GUEST_ALLOCATOR,
    }
}

fn init_fallback_regions() {
    let host_fallback = addr_of_mut!(FALLBACK_HEAP) as *mut u8 as usize;
    let hv_fallback = addr_of_mut!(HV_GUEST_FALLBACK_HEAP) as *mut u8 as usize;
    {
        let mut guard = ALLOCATOR.lock();
        if guard.fallback_virt_start == 0 {
            guard.fallback_virt_start = host_fallback;
            guard.fallback_len = FALLBACK_HEAP_SIZE;
        }
    }
    {
        let mut guard = HV_GUEST_ALLOCATOR.lock();
        if guard.fallback_virt_start == 0 {
            guard.fallback_virt_start = hv_fallback;
            guard.fallback_len = HV_GUEST_HEAP_FALLBACK_SIZE;
        }
    }
}

pub fn enter_hv_guest_domain_current_cpu() -> bool {
    init_fallback_regions();
    let slot = crate::percpu::current_slot_via_cpuid();
    if slot >= 64 {
        return false;
    }
    let _ = ensure_hv_guest_heap_ready();
    HV_GUEST_ACTIVE_CPU_MASK.fetch_or(1u64 << slot, Ordering::AcqRel);
    true
}

pub fn leave_hv_guest_domain_current_cpu() {
    let slot = crate::percpu::current_slot_via_cpuid();
    if slot >= 64 {
        return;
    }
    HV_GUEST_ACTIVE_CPU_MASK.fetch_and(!(1u64 << slot), Ordering::AcqRel);
}

pub fn ensure_hv_guest_heap_ready() -> bool {
    init_fallback_regions();
    if HV_GUEST_HEAP_READY.load(Ordering::Acquire) != 0 {
        return true;
    }

    let mut guard = HV_GUEST_ALLOCATOR.lock();
    if guard.initialized || guard.heap_len != 0 {
        HV_GUEST_HEAP_READY.store(1, Ordering::Release);
        return true;
    }

    for &size in HV_GUEST_HEAP_CANDIDATES.iter() {
        let Some(arena) = phys::reserve_heap_arena(size, HV_GUEST_HEAP_ALIGN) else {
            continue;
        };
        guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
        HV_GUEST_HEAP_READY.store(1, Ordering::Release);
        crate::log!(
            "heap: hv guest arena virt=0x{:X} phys=0x{:X} size={} MiB\n",
            arena.virt_start,
            arena.phys_start,
            arena.length / (1024 * 1024)
        );
        return true;
    }

    crate::log!(
        "heap: hv guest arena unavailable, falling back to private {} KiB heap\n",
        HV_GUEST_HEAP_FALLBACK_SIZE / 1024
    );
    HV_GUEST_HEAP_READY.store(1, Ordering::Release);
    true
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        init_fallback_regions();
        let domain = current_alloc_domain();
        let ptr = allocator_for_domain(domain).lock().alloc(domain, layout);
        if !ptr.is_null() {
            let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
            (*tag_ptr).domain = domain as u8;
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
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
    init_fallback_regions();
    let domain = current_alloc_domain();
    let ptr = allocator_for_domain(domain).lock().alloc(domain, layout);
    if !ptr.is_null() {
        let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
        (*tag_ptr).domain = domain as u8;
    } else if domain == AllocDomain::HvGuest {
        let stats = hv_guest_heap_stats();
        let trace = last_alloc_trace();
        crate::log!(
            "hv-guest-alloc: alloc_raw failed size={} align={} src={:?} usable_total={} free_bytes={} largest_free={} free_blocks={} init={}\n",
            layout.size(),
            layout.align(),
            stats.source,
            stats.usable_total,
            stats.free_bytes,
            stats.largest_free_block,
            stats.free_blocks,
            stats.initialized,
        );
        crate::log!(
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
        );
    }
    ptr
}

pub unsafe fn dealloc_raw(ptr: *mut u8) {
    if ptr.is_null() {
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
    init_fallback_regions();
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

pub fn hv_guest_heap_stats() -> HeapStats {
    init_fallback_regions();
    let mut guard = HV_GUEST_ALLOCATOR.lock();
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

pub fn install_heap_arena(arena: HeapArena) -> bool {
    init_fallback_regions();
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
    let payload_start =
        align_up(block_start + size_of::<FreeBlock>() + size_of::<AllocTag>(), layout.align());
    if payload_start > usize::MAX - layout.size() {
        None
    } else {
        Some(payload_start)
    }
}

fn alloc_error(layout: Layout) -> ! {
    let stats = heap_stats();
    crate::log!("OOM: alloc request size={} align={}\n", layout.size(), layout.align());
    crate::log!(
        "OOM: heap virt=0x{:X}..0x{:X} phys=0x{:X} src={:?} usable_start=0x{:X} usable_total={} free_bytes={} largest_free={} free_blocks={} init={}\n",
        stats.heap_start,
        stats.heap_end,
        stats.phys_start,
        stats.source,
        stats.usable_start,
        stats.usable_total,
        stats.free_bytes,
        stats.largest_free_block,
        stats.free_blocks,
        stats.initialized
    );

    unsafe {
        #[cfg(target_arch = "x86_64")]
        {
        asm!("cli", options(nomem, nostack));
            loop {
                asm!("hlt", options(nomem, nostack));
            }
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            // ARMTODO: OOM stop handling is still x86-specific (`cli`/`hlt`).
            // Non-x86 bring-up needs the right platform halt/panic stop path.
            loop {
                core::hint::spin_loop();
            }
        }
    }
}
