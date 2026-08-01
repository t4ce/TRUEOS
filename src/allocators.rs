use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use core::mem::{align_of, size_of};
use core::ptr::{NonNull, null_mut};
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use crate::phys::{self, HeapArena};

const HV_GUEST_HEAP_ALIGN: usize = 2 * 1024 * 1024;
pub const HV_GUEST_HEAP_MIN_ARENA_SIZE: usize = 16 * 1024 * 1024;
pub const HV_GUEST_HEAP_MAX_ARENA_SIZE: usize = 4 * 1024 * 1024 * 1024;
const HV_GUEST_HEAP_LARGE_FALLBACK_ARENA_SIZE: usize = 3 * 1024 * 1024 * 1024;
const HV_GUEST_HEAP_CANDIDATES: [usize; 9] = [
    HV_GUEST_HEAP_MAX_ARENA_SIZE,
    HV_GUEST_HEAP_LARGE_FALLBACK_ARENA_SIZE,
    2 * 1024 * 1024 * 1024,
    1024 * 1024 * 1024,
    512 * 1024 * 1024,
    384 * 1024 * 1024,
    256 * 1024 * 1024,
    192 * 1024 * 1024,
    128 * 1024 * 1024,
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
static ALLOC_DOMAIN_MISMATCH_LOGGED: AtomicU32 = AtomicU32::new(0);
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

    {
        #[inline]
        fn plausible_frame_ptr(ptr: usize) -> bool {
            if ptr < 0x1000 || !ptr.is_multiple_of(core::mem::align_of::<usize>()) {
                return false;
            }
            let sign = (ptr >> 47) & 1;
            let high = ptr >> 48;
            if sign == 0 { high == 0 } else { high == 0xFFFF }
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

fn publish_hv_guest_heap_range(vm_id: u8, start: usize, len: usize) {
    let Some(page) = hv_guest_allocator_page(vm_id) else {
        return;
    };
    page.heap_virt_start.store(start, Ordering::Release);
    page.heap_virt_end
        .store(start.saturating_add(len), Ordering::Release);
}

/// Stable lock-free bounds for allocator-independent ABI hot paths.
pub fn hv_guest_heap_bounds(vm_id: u8) -> Option<(usize, usize)> {
    let page = hv_guest_allocator_page(vm_id)?;
    let start = page.heap_virt_start.load(Ordering::Acquire);
    let end = page.heap_virt_end.load(Ordering::Acquire);
    (start != 0 && end > start).then_some((start, end))
}

#[inline]
pub fn host_heap_contains_addr(addr: usize) -> bool {
    let start = HOST_HEAP_VIRT_START.load(Ordering::Acquire);
    let end = HOST_HEAP_VIRT_END.load(Ordering::Acquire);
    start != 0 && end > start && addr >= start && addr < end
}

fn alloc_domain_for_address(addr: usize) -> Option<(AllocDomain, usize)> {
    let host_start = HOST_HEAP_VIRT_START.load(Ordering::Acquire);
    if host_heap_contains_addr(addr) {
        return Some((AllocDomain::Host, host_start));
    }
    for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
        let vm_id = vm_id as u8;
        let Some((start, end)) = hv_guest_heap_bounds(vm_id) else {
            continue;
        };
        if addr >= start && addr < end {
            return Some((AllocDomain::HvGuest(vm_id), start));
        }
    }
    None
}

fn log_alloc_domain_mismatch(ptr: *mut u8, address_domain: Option<AllocDomain>, tag: Option<u8>) {
    if ALLOC_DOMAIN_MISMATCH_LOGGED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log_error!(
            target: "hv";
            "allocator: rejected dealloc ptr=0x{:X} address_domain={:?} tag={:?} risk=realm-domain-mismatch\n",
            ptr as usize,
            address_domain,
            tag
        );
    }
}

fn dealloc_domain_for_address(ptr: *mut u8) -> Option<AllocDomain> {
    let Some((address_domain, _)) = alloc_domain_for_address(ptr as usize) else {
        log_alloc_domain_mismatch(ptr, None, None);
        return None;
    };
    Some(address_domain)
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
    /// Exact allocator-returned pointer; rejects an interior page whose
    /// preceding payload bytes merely resemble an allocation tag.
    payload_start: usize,
    domain: u8,
    /// Host-mediated DMA/GPU mappings that still name this allocation.
    ///
    /// This occupies padding already present in the tag on x86_64.
    dma_pins: u32,
}

const _: () = assert!(size_of::<AllocTag>() == 32);

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

/// Linear ownership token for one allocator-level guest DMA pin.
///
/// The token deliberately has no `Drop` implementation. Once registration is
/// published, an accidental broker-record drop must leak/quarantine the pin,
/// never make pages reusable without a definitive physical unmap.
pub(crate) struct HvGuestDmaPin {
    vm_id: u8,
    ptr: usize,
    bytes: usize,
    block_start: usize,
    block_size: usize,
}

/// Rollback guard used before a vVideo mapping is published in the broker.
pub(crate) struct HvGuestDmaPinReservation {
    pin: Option<HvGuestDmaPin>,
}

impl HvGuestDmaPinReservation {
    /// Transfer the pin into a broker backing after registration succeeds.
    pub(crate) fn retain_for_mapping(mut self) -> HvGuestDmaPin {
        self.pin.take().expect("live guest DMA pin reservation")
    }

    /// Keep the allocator pin without an owner when mapping rollback itself is
    /// uncertain. The per-VM pin count remains a lifecycle reuse fence.
    pub(crate) fn quarantine(mut self) {
        let _ = self.pin.take();
    }
}

impl Drop for HvGuestDmaPinReservation {
    fn drop(&mut self) {
        if let Some(pin) = self.pin.take() {
            let _ = release_hv_guest_dma_pin(pin);
        }
    }
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
                payload_start,
                domain: alloc_domain_tag(domain),
                dma_pins: 0,
            });

            trace_alloc_success(trace_enabled);
            return payload_start as *mut u8;
        }

        null_mut()
    }

    unsafe fn dealloc(&mut self, domain: AllocDomain, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }

        // All pointer/tag/free-list validation occurs under this FreeList's
        // mutex. Whole-heap release can therefore either win first (leaving an
        // empty geometry that rejects this pointer without dereferencing it)
        // or wait until deallocation is completely retired.
        let ptr_addr = ptr as usize;
        let (heap_start, heap_len) = self.ensure_heap_backing();
        let Some(heap_end) = heap_start.checked_add(heap_len) else {
            return;
        };
        let Some(tag_addr) = ptr_addr.checked_sub(size_of::<AllocTag>()) else {
            return;
        };
        if heap_start == 0
            || ptr_addr < heap_start
            || ptr_addr >= heap_end
            || tag_addr < heap_start
            || tag_addr.saturating_add(size_of::<AllocTag>()) > heap_end
        {
            return;
        }
        let tag_ptr = tag_addr as *mut AllocTag;
        let tag = unsafe { tag_ptr.read() };
        if tag.payload_start != ptr_addr || alloc_domain_from_tag(&tag) != Some(domain) {
            log_alloc_domain_mismatch(ptr, Some(domain), Some(tag.domain));
            return;
        }
        let block_size = tag.block_size;
        let block_start = tag.block_start;
        let Some(block_end) = block_start.checked_add(block_size) else {
            return;
        };
        if !self.is_plausible_alloc_block(block_start, block_size)
            || tag_addr < block_start
            || ptr_addr >= block_end
            || !unsafe { self.allocation_block_is_live(block_start, block_end) }
        {
            crate::log!(
                "alloc: ignored invalid dealloc ptr=0x{:016X} tag_block=0x{:016X} tag_size={} tag_domain={}\n",
                ptr as usize,
                block_start,
                block_size,
                tag.domain
            );
            return;
        }
        if tag.dma_pins != 0 {
            // Raw guest deallocation cannot return GPU-owned pages to the free
            // list. The definitive PPGTT unmap path owns the matching token.
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

    unsafe fn pin_dma_range(
        &mut self,
        domain: AllocDomain,
        ptr: usize,
        bytes: usize,
    ) -> Option<(usize, usize)> {
        if bytes == 0 {
            return None;
        }
        let range_end = ptr.checked_add(bytes)?;
        let tag_addr = ptr.checked_sub(size_of::<AllocTag>())?;
        let (heap_start, heap_len) = self.ensure_heap_backing();
        let heap_end = heap_start.checked_add(heap_len)?;
        if heap_start == 0
            || ptr < heap_start
            || ptr >= heap_end
            || tag_addr < heap_start
            || tag_addr.checked_add(size_of::<AllocTag>())? > heap_end
        {
            return None;
        }
        let tag_ptr = tag_addr as *mut AllocTag;
        let tag = unsafe { tag_ptr.read() };
        if tag.payload_start != ptr
            || alloc_domain_from_tag(&tag) != Some(domain)
            || !self.is_plausible_alloc_block(tag.block_start, tag.block_size)
        {
            return None;
        }
        let metadata_end = tag
            .block_start
            .checked_add(size_of::<FreeBlock>())
            .and_then(|value| value.checked_add(size_of::<AllocTag>()))?;
        let block_end = tag.block_start.checked_add(tag.block_size)?;
        if ptr < metadata_end || tag_addr < tag.block_start || range_end > block_end {
            return None;
        }
        if !unsafe { self.allocation_block_is_live(tag.block_start, block_end) } {
            return None;
        }
        let pins = tag.dma_pins.checked_add(1)?;
        unsafe { (*tag_ptr).dma_pins = pins };
        Some((tag.block_start, tag.block_size))
    }

    unsafe fn unpin_dma_range(&mut self, pin: &HvGuestDmaPin) -> bool {
        let Some(tag_addr) = pin.ptr.checked_sub(size_of::<AllocTag>()) else {
            return false;
        };
        let (heap_start, heap_len) = self.ensure_heap_backing();
        let Some(heap_end) = heap_start.checked_add(heap_len) else {
            return false;
        };
        if heap_start == 0
            || pin.ptr < heap_start
            || pin.ptr >= heap_end
            || tag_addr < heap_start
            || tag_addr.saturating_add(size_of::<AllocTag>()) > heap_end
        {
            return false;
        }
        let tag_ptr = tag_addr as *mut AllocTag;
        let tag = unsafe { tag_ptr.read() };
        let Some(range_end) = pin.ptr.checked_add(pin.bytes) else {
            return false;
        };
        let Some(block_end) = tag.block_start.checked_add(tag.block_size) else {
            return false;
        };
        if tag.payload_start != pin.ptr
            || alloc_domain_from_tag(&tag) != Some(AllocDomain::HvGuest(pin.vm_id))
            || tag.block_start != pin.block_start
            || tag.block_size != pin.block_size
            || range_end > block_end
            || tag.dma_pins == 0
            || !self.is_plausible_alloc_block(tag.block_start, tag.block_size)
            || !unsafe { self.allocation_block_is_live(tag.block_start, block_end) }
        {
            return false;
        }
        unsafe { (*tag_ptr).dma_pins = tag.dma_pins - 1 };
        true
    }

    unsafe fn allocation_block_is_live(&mut self, block_start: usize, block_end: usize) -> bool {
        let mut current = self.head;
        let mut remaining = self.heap_len / minimum_block_size() + 1;
        while let Some(node) = current {
            if remaining == 0 || !self.is_plausible_free_block_ptr(node.as_ptr() as usize) {
                return false;
            }
            remaining -= 1;
            let free = unsafe { node.as_ref() };
            let free_start = node.as_ptr() as usize;
            let Some(free_end) = free_start.checked_add(free.size) else {
                return false;
            };
            if block_start < free_end && free_start < block_end {
                return false;
            }
            current = free.next;
        }
        true
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

// The Hull page tables share this state with host service carriers at 4 KiB
// granularity. Give it a complete, dedicated page-sized ELF object so the
// shared mapping can never expose unrelated host BSS placed next to it by the
// compiler or linker.
#[repr(C, align(4096))]
struct HvGuestAllocatorSharedPage {
    allocator: Mutex<FreeList>,
    ready: AtomicU64,
    heap_virt_start: AtomicUsize,
    heap_virt_end: AtomicUsize,
}

impl HvGuestAllocatorSharedPage {
    const fn new() -> Self {
        Self {
            allocator: Mutex::new(FreeList::new()),
            ready: AtomicU64::new(0),
            heap_virt_start: AtomicUsize::new(0),
            heap_virt_end: AtomicUsize::new(0),
        }
    }
}

static HV_GUEST_ALLOCATOR_SHARED_PAGES: [HvGuestAllocatorSharedPage;
    crate::allcaps::hv::VM_ID_LIMIT] =
    [const { HvGuestAllocatorSharedPage::new() }; crate::allcaps::hv::VM_ID_LIMIT];

// Host lifecycle authority for allocator pins. The per-allocation count lives
// in `AllocTag` so deallocation is serialized by the existing heap mutex; this
// aggregate lets VM-slot/whole-arena release fail closed without walking every
// occupied allocation.
static HV_GUEST_DMA_PIN_TOTALS: [AtomicU64; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicU64::new(0) }; crate::allcaps::hv::VM_ID_LIMIT];

const _: () = assert!(core::mem::size_of::<HvGuestAllocatorSharedPage>() == 4096);
const _: () = assert!(core::mem::align_of::<HvGuestAllocatorSharedPage>() == 4096);

#[inline]
fn hv_guest_allocator_page(vm_id: u8) -> Option<&'static HvGuestAllocatorSharedPage> {
    HV_GUEST_ALLOCATOR_SHARED_PAGES.get(vm_id as usize)
}

pub(crate) fn pin_hv_guest_dma_range(
    vm_id: u8,
    guest_va: u64,
    bytes: usize,
) -> Option<HvGuestDmaPinReservation> {
    let ptr = usize::try_from(guest_va).ok()?;
    let page = hv_guest_allocator_page(vm_id)?;
    let total = HV_GUEST_DMA_PIN_TOTALS.get(vm_id as usize)?;
    let mut guard = page.allocator.lock();
    if total.load(Ordering::Acquire) == u64::MAX {
        return None;
    }
    let (block_start, block_size) =
        unsafe { guard.pin_dma_range(AllocDomain::HvGuest(vm_id), ptr, bytes)? };
    total.fetch_add(1, Ordering::AcqRel);
    Some(HvGuestDmaPinReservation {
        pin: Some(HvGuestDmaPin {
            vm_id,
            ptr,
            bytes,
            block_start,
            block_size,
        }),
    })
}

pub(crate) fn release_hv_guest_dma_pin(pin: HvGuestDmaPin) -> bool {
    let Some(page) = hv_guest_allocator_page(pin.vm_id) else {
        return false;
    };
    let Some(total) = HV_GUEST_DMA_PIN_TOTALS.get(pin.vm_id as usize) else {
        return false;
    };
    let mut guard = page.allocator.lock();
    if total.load(Ordering::Acquire) == 0 || !unsafe { guard.unpin_dma_range(&pin) } {
        return false;
    }
    total.fetch_sub(1, Ordering::AcqRel);
    true
}

pub(crate) fn hv_guest_dma_ranges_pinned(vm_id: u8) -> bool {
    HV_GUEST_DMA_PIN_TOTALS
        .get(vm_id as usize)
        .is_some_and(|total| total.load(Ordering::Acquire) != 0)
}

pub(crate) fn hv_guest_allocator_state_span(vm_id: u8) -> Option<(u64, usize)> {
    let page = hv_guest_allocator_page(vm_id)?;
    Some(((page as *const _) as u64, core::mem::size_of_val(page)))
}

const HOST_ALLOC_TAG: u8 = u8::MAX;
static HOST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
static HOST_ALLOC_DOMAIN_STRONG_DEPTH_BY_CPU: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
static HV_GUEST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU: [AtomicU32; 64] =
    [const { AtomicU32::new(0) }; 64];
static HV_GUEST_ALLOC_DOMAIN_FORCE_VM_BY_CPU: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];

fn alloc_domain_from_tag(tag: &AllocTag) -> Option<AllocDomain> {
    if tag.domain == HOST_ALLOC_TAG {
        Some(AllocDomain::Host)
    } else if (tag.domain as usize) < crate::allcaps::hv::VM_ID_LIMIT {
        Some(AllocDomain::HvGuest(tag.domain))
    } else {
        None
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

fn should_log_hv_guest_alloc_success() -> bool {
    crate::hv::current_hull_guest_context_vm_id().is_none()
}

fn cpuid_slot() -> Option<usize> {
    let slot = crate::percpu::current_slot_via_cpuid();
    if slot < 64 { Some(slot) } else { None }
}

fn current_alloc_domain() -> AllocDomain {
    // Hull guest code shares this image, but not host virtual heap/percpu
    // discovery state. The comm page/VMX context is the authority here.
    if let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() {
        return AllocDomain::HvGuest(vm_id);
    }

    if let Some(slot) = cpuid_slot()
        && HOST_ALLOC_DOMAIN_STRONG_DEPTH_BY_CPU[slot].load(Ordering::Acquire) != 0
    {
        return AllocDomain::Host;
    }

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

    if let Some(vm_id) = crate::r::kernel_task_domain::guest_owned_alloc_vm_id() {
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

pub fn with_host_alloc_domain_strong<T>(f: impl FnOnce() -> T) -> T {
    let Some(slot) = cpuid_slot() else {
        return f();
    };
    let Some(depth) = HOST_ALLOC_DOMAIN_STRONG_DEPTH_BY_CPU.get(slot) else {
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
        AllocDomain::HvGuest(vm_id) => {
            &hv_guest_allocator_page(vm_id)
                .unwrap_or(&HV_GUEST_ALLOCATOR_SHARED_PAGES[0])
                .allocator
        }
    }
}

pub fn with_hv_guest_alloc_domain<T>(vm_id: u8, f: impl FnOnce() -> T) -> Option<T> {
    if (vm_id as usize) >= crate::allcaps::hv::VM_ID_LIMIT || !ensure_hv_guest_heap_ready(vm_id) {
        return None;
    }
    if crate::hv::current_hull_guest_context_vm_id() == Some(vm_id) {
        return Some(f());
    }
    let Some(slot) = cpuid_slot() else {
        return Some(crate::r::kernel_task_domain::with(
            crate::r::kernel_task_domain::KernelTaskDomain::VmGuestOwnedAlloc,
            Some(vm_id),
            f,
        ));
    };
    let depth = HV_GUEST_ALLOC_DOMAIN_FORCE_DEPTH_BY_CPU.get(slot)?;
    let vm_force = HV_GUEST_ALLOC_DOMAIN_FORCE_VM_BY_CPU.get(slot)?;
    let previous_vm = vm_force.swap(vm_id as u32 + 1, Ordering::AcqRel);
    depth.fetch_add(1, Ordering::AcqRel);
    let out = crate::r::kernel_task_domain::with(
        crate::r::kernel_task_domain::KernelTaskDomain::VmGuestOwnedAlloc,
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
    let page = &HV_GUEST_ALLOCATOR_SHARED_PAGES[vm_id as usize];
    if page.ready.load(Ordering::Acquire) != 0 && hv_guest_heap_bounds(vm_id).is_some() {
        return true;
    }

    let mut guard = page.allocator.lock();
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
        publish_hv_guest_heap_range(vm_id, guard.heap_virt_start, guard.heap_len);
        page.ready.store(1, Ordering::Release);
        return true;
    }

    for &size in HV_GUEST_HEAP_CANDIDATES.iter() {
        let Some(arena) = phys::reserve_heap_arena(size, HV_GUEST_HEAP_ALIGN) else {
            continue;
        };
        guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
        publish_hv_guest_heap_range(vm_id, arena.virt_start, arena.length);
        page.ready.store(1, Ordering::Release);
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

pub fn prepare_hv_guest_heap_for_vm(
    vm_id: u8,
    requested_size: usize,
    minimum_acceptable_size: usize,
) -> bool {
    if (vm_id as usize) >= crate::allcaps::hv::VM_ID_LIMIT {
        return false;
    }

    let requested_size = round_hv_guest_heap_request(requested_size);
    let minimum_acceptable_size =
        round_hv_guest_heap_request(minimum_acceptable_size).min(requested_size);
    let page = &HV_GUEST_ALLOCATOR_SHARED_PAGES[vm_id as usize];
    let mut guard = page.allocator.lock();
    if guard.initialized {
        if guard.heap_source == HeapSourceKind::Arena {
            publish_hv_guest_heap_range(vm_id, guard.heap_virt_start, guard.heap_len);
            page.ready.store(1, Ordering::Release);
            return guard.heap_len >= minimum_acceptable_size;
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
        if guard.heap_source == HeapSourceKind::Arena && guard.heap_len >= minimum_acceptable_size {
            publish_hv_guest_heap_range(vm_id, guard.heap_virt_start, guard.heap_len);
            page.ready.store(1, Ordering::Release);
            return true;
        }
        crate::log!(
            "heap: hv guest vm{} heap configured src={:?} size={} KiB requested={} MiB min_acceptable={} MiB; refusing launch\n",
            vm_id,
            guard.heap_source,
            guard.heap_len / 1024,
            requested_size / (1024 * 1024),
            minimum_acceptable_size / (1024 * 1024)
        );
        return false;
    }

    let mut selected = None;
    let mut last_candidate = 0;
    for &candidate in HV_GUEST_HEAP_CANDIDATES.iter() {
        let candidate = candidate.min(requested_size);
        if candidate < minimum_acceptable_size || candidate == last_candidate {
            continue;
        }
        last_candidate = candidate;
        let Some(arena) = phys::reserve_heap_arena(candidate, HV_GUEST_HEAP_ALIGN) else {
            continue;
        };
        selected = Some(arena);
        break;
    }

    let Some(arena) = selected else {
        crate::log!(
            "heap: hv guest vm{} requested arena unavailable size={} MiB min_acceptable={} MiB absolute_min={} MiB\n",
            vm_id,
            requested_size / (1024 * 1024),
            minimum_acceptable_size / (1024 * 1024),
            HV_GUEST_HEAP_MIN_ARENA_SIZE / (1024 * 1024)
        );
        return false;
    };
    guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    publish_hv_guest_heap_range(vm_id, arena.virt_start, arena.length);
    page.ready.store(1, Ordering::Release);
    crate::log!(
        "heap: hv guest vm{} arena virt=0x{:X} phys=0x{:X} size={} MiB requested={} MiB\n",
        vm_id,
        arena.virt_start,
        arena.phys_start,
        arena.length / (1024 * 1024),
        requested_size / (1024 * 1024)
    );
    if arena.length < requested_size {
        crate::log!(
            "heap: hv guest vm{} arena fallback accepted size={} MiB requested={} MiB min_acceptable={} MiB\n",
            vm_id,
            arena.length / (1024 * 1024),
            requested_size / (1024 * 1024),
            minimum_acceptable_size / (1024 * 1024)
        );
    }
    true
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let domain = current_alloc_domain();
        let ptr = allocator_for_domain(domain).lock().alloc(domain, layout);
        if !ptr.is_null() {
            let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
            (*tag_ptr).domain = alloc_domain_tag(domain);
            if let Some(vm_id) = alloc_domain_vm_id(domain)
                && should_log_hv_guest_alloc_success()
            {
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
        let Some(domain) = dealloc_domain_for_address(ptr) else {
            return;
        };
        unsafe { allocator_for_domain(domain).lock().dealloc(domain, ptr) }
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
        if let Some(vm_id) = alloc_domain_vm_id(domain)
            && should_log_hv_guest_alloc_success()
        {
            log_hv_guest_alloc_watermark(vm_id, layout, ptr, "raw");
        }
    } else if let Some(vm_id) = alloc_domain_vm_id(domain) {
        log_hv_guest_alloc_failure(vm_id, layout, "raw");
    }
    ptr
}

pub unsafe fn alloc_raw_hv_guest(vm_id: u8, layout: Layout) -> *mut u8 {
    if (vm_id as usize) >= crate::allcaps::hv::VM_ID_LIMIT || !ensure_hv_guest_heap_ready(vm_id) {
        return core::ptr::null_mut();
    }

    let domain = AllocDomain::HvGuest(vm_id);
    let ptr = {
        let mut guard = allocator_for_domain(domain).lock();
        guard.alloc(domain, layout)
    };
    if !ptr.is_null() {
        let tag_ptr = ptr.sub(size_of::<AllocTag>()) as *mut AllocTag;
        (*tag_ptr).domain = alloc_domain_tag(domain);
        if should_log_hv_guest_alloc_success() {
            log_hv_guest_alloc_watermark(vm_id, layout, ptr, "raw-explicit");
        }
    } else if should_log_hv_guest_alloc_success() {
        log_hv_guest_alloc_failure(vm_id, layout, "raw-explicit");
    }
    ptr
}

fn log_hv_guest_alloc_watermark(vm_id: u8, layout: Layout, ptr: *mut u8, path: &str) {
    with_host_alloc_domain_strong(|| {
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
        crate::log_info!(
            target: "hv";
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
        );
    });
}

fn log_hv_guest_alloc_failure(vm_id: u8, layout: Layout, path: &str) {
    with_host_alloc_domain_strong(|| {
        let stats = hv_guest_heap_stats(vm_id);
        let trace = last_alloc_trace();
        crate::log_warn!(
            target: "hv";
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
        );
        crate::log_warn!(
            target: "hv";
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
    });
}

pub unsafe fn dealloc_raw(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let Some(domain) = dealloc_domain_for_address(ptr) else {
        return;
    };
    unsafe { allocator_for_domain(domain).lock().dealloc(domain, ptr) }
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

/// Allocation-free, bounded validation of the host heap free list.
///
/// Unlike [`heap_stats`], this stops at the first non-increasing pointer,
/// overlap, invalid node, or traversal limit. It is therefore safe to use as a
/// diagnostic when a duplicate free may have made a node point to itself.
#[derive(Clone, Copy, Debug)]
pub struct HeapIntegrityReport {
    pub healthy: bool,
    pub reason: &'static str,
    pub nodes: usize,
    pub current: usize,
    pub next: usize,
}

pub fn host_heap_integrity_bounded() -> HeapIntegrityReport {
    const NODE_LIMIT: usize = 1_000_000;

    let mut guard = ALLOCATOR.lock();
    unsafe {
        if !guard.initialized {
            guard.init_once();
        }
    }

    let (heap_start, heap_len) = guard.ensure_heap_backing();
    let heap_end = heap_start.saturating_add(heap_len);
    let mut nodes = 0usize;
    let mut previous_end = align_up(heap_start, align_of::<FreeBlock>());
    let mut current = guard.head;

    while let Some(block_ptr) = current {
        let address = block_ptr.as_ptr() as usize;
        if nodes >= NODE_LIMIT {
            return HeapIntegrityReport {
                healthy: false,
                reason: "node-limit",
                nodes,
                current: address,
                next: address,
            };
        }
        if !guard.is_plausible_free_block_ptr(address) {
            return HeapIntegrityReport {
                healthy: false,
                reason: "invalid-node",
                nodes,
                current: address,
                next: 0,
            };
        }

        // Safety: the pointer has just been checked against the configured
        // heap bounds and FreeBlock alignment.
        let block = unsafe { block_ptr.as_ref() };
        let block_end = address.saturating_add(block.size);
        if block.size < minimum_block_size() || block_end > heap_end {
            return HeapIntegrityReport {
                healthy: false,
                reason: "invalid-size",
                nodes,
                current: address,
                next: block.next.map(|next| next.as_ptr() as usize).unwrap_or(0),
            };
        }
        if address < previous_end {
            return HeapIntegrityReport {
                healthy: false,
                reason: "unordered-or-overlap",
                nodes,
                current: address,
                next: block.next.map(|next| next.as_ptr() as usize).unwrap_or(0),
            };
        }

        nodes = nodes.saturating_add(1);
        let next_address = block.next.map(|next| next.as_ptr() as usize).unwrap_or(0);
        if next_address != 0 && next_address <= address {
            return HeapIntegrityReport {
                healthy: false,
                reason: "non-increasing-next",
                nodes,
                current: address,
                next: next_address,
            };
        }
        if next_address != 0 && block_end > next_address {
            return HeapIntegrityReport {
                healthy: false,
                reason: "overlapping-next",
                nodes,
                current: address,
                next: next_address,
            };
        }

        previous_end = block_end;
        current = block.next;
    }

    HeapIntegrityReport {
        healthy: true,
        reason: "ok",
        nodes,
        current: 0,
        next: 0,
    }
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
    let Some(allocator) = hv_guest_allocator_page(vm_id).map(|page| &page.allocator) else {
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
    let allocator = &hv_guest_allocator_page(vm_id)?.allocator;
    let mut guard = allocator.lock();
    if !guard.initialized && guard.heap_len == 0 {
        return None;
    }
    Some(heap_stats_from_guard(&mut guard))
}

const HV_GUEST_HEAP_IMAGE_MAGIC: u32 = u32::from_le_bytes(*b"HGS1");
// Version 2 records the exact-payload AllocTag layout used by DMA pinning.
const HV_GUEST_HEAP_IMAGE_VERSION: u32 = 2;

fn heap_image_push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn heap_image_push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Capture only allocated spans of a paused guest heap. Free spans are encoded
/// as ranges and rebuilt on import, keeping a 512 MiB arena with 30 MiB in use
/// close to 30 MiB on disk.
pub fn snapshot_hv_guest_heap(vm_id: u8) -> Result<Vec<u8>, &'static str> {
    let page = hv_guest_allocator_page(vm_id).ok_or("unsupported guest heap")?;
    let mut guard = page.allocator.lock();
    if hv_guest_dma_ranges_pinned(vm_id) {
        return Err("guest heap has GPU-pinned allocations");
    }
    if !guard.initialized || guard.heap_source != HeapSourceKind::Arena || guard.heap_len == 0 {
        return Err("guest heap is not an initialized arena");
    }
    let heap_start = guard.heap_virt_start;
    let heap_len = guard.heap_len;
    let heap_end = heap_start
        .checked_add(heap_len)
        .ok_or("guest heap bounds")?;
    let mut free = Vec::<(usize, usize)>::new();
    let mut current = guard.head;
    let mut previous_end = heap_start;
    while let Some(node) = current {
        let address = node.as_ptr() as usize;
        if !guard.is_plausible_free_block_ptr(address) || address < previous_end {
            return Err("guest heap free list is invalid");
        }
        let block = unsafe { node.as_ref() };
        let end = address
            .checked_add(block.size)
            .ok_or("guest heap free range")?;
        if block.size < minimum_block_size() || end > heap_end {
            return Err("guest heap free range is invalid");
        }
        free.push((address - heap_start, block.size));
        previous_end = end;
        current = block.next;
    }

    let mut occupied = Vec::<(usize, usize)>::new();
    let mut cursor = 0usize;
    for &(offset, len) in free.iter() {
        if cursor < offset {
            occupied.push((cursor, offset - cursor));
        }
        cursor = offset.checked_add(len).ok_or("guest heap free range")?;
    }
    if cursor < heap_len {
        occupied.push((cursor, heap_len - cursor));
    }
    let occupied_bytes = occupied
        .iter()
        .try_fold(0usize, |total, (_, len)| total.checked_add(*len))
        .ok_or("guest heap image size")?;
    let header_bytes = 40usize
        .checked_add(free.len().saturating_mul(16))
        .and_then(|v| v.checked_add(occupied.len().saturating_mul(16)))
        .and_then(|v| v.checked_add(occupied_bytes))
        .ok_or("guest heap image size")?;
    let mut out = Vec::with_capacity(header_bytes);
    heap_image_push_u32(&mut out, HV_GUEST_HEAP_IMAGE_MAGIC);
    heap_image_push_u32(&mut out, HV_GUEST_HEAP_IMAGE_VERSION);
    heap_image_push_u64(&mut out, guard.heap_phys_start as u64);
    heap_image_push_u64(&mut out, heap_start as u64);
    heap_image_push_u64(&mut out, heap_len as u64);
    heap_image_push_u32(&mut out, free.len() as u32);
    heap_image_push_u32(&mut out, occupied.len() as u32);
    for &(offset, len) in free.iter() {
        heap_image_push_u64(&mut out, offset as u64);
        heap_image_push_u64(&mut out, len as u64);
    }
    for &(offset, len) in occupied.iter() {
        heap_image_push_u64(&mut out, offset as u64);
        heap_image_push_u64(&mut out, len as u64);
        let bytes = unsafe { core::slice::from_raw_parts((heap_start + offset) as *const u8, len) };
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

fn heap_image_take_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let raw = bytes.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u32::from_le_bytes(raw))
}

fn heap_image_take_u64(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let raw = bytes.get(*offset..end)?.try_into().ok()?;
    *offset = end;
    Some(u64::from_le_bytes(raw))
}

/// Restore a pointer-bearing guest heap at its original HHDM address.
pub fn restore_hv_guest_heap(vm_id: u8, bytes: &[u8]) -> Result<(), &'static str> {
    let mut offset = 0usize;
    if heap_image_take_u32(bytes, &mut offset) != Some(HV_GUEST_HEAP_IMAGE_MAGIC)
        || heap_image_take_u32(bytes, &mut offset) != Some(HV_GUEST_HEAP_IMAGE_VERSION)
    {
        return Err("bad guest heap image");
    }
    let phys_start = heap_image_take_u64(bytes, &mut offset).ok_or("guest heap header")?;
    let virt_start =
        usize::try_from(heap_image_take_u64(bytes, &mut offset).ok_or("guest heap header")?)
            .map_err(|_| "guest heap address")?;
    let heap_len =
        usize::try_from(heap_image_take_u64(bytes, &mut offset).ok_or("guest heap header")?)
            .map_err(|_| "guest heap length")?;
    let free_count = heap_image_take_u32(bytes, &mut offset).ok_or("guest heap header")? as usize;
    let occupied_count =
        heap_image_take_u32(bytes, &mut offset).ok_or("guest heap header")? as usize;
    if heap_len < minimum_block_size()
        || phys_start as usize % HV_GUEST_HEAP_ALIGN != 0
        || heap_len % HV_GUEST_HEAP_ALIGN != 0
    {
        return Err("guest heap geometry");
    }
    let mut free = Vec::<(usize, usize)>::with_capacity(free_count);
    let mut previous_end = 0usize;
    for _ in 0..free_count {
        let start = usize::try_from(heap_image_take_u64(bytes, &mut offset).ok_or("free range")?)
            .map_err(|_| "free range")?;
        let len = usize::try_from(heap_image_take_u64(bytes, &mut offset).ok_or("free range")?)
            .map_err(|_| "free range")?;
        let end = start.checked_add(len).ok_or("free range")?;
        if start < previous_end || end > heap_len || len < minimum_block_size() {
            return Err("free range geometry");
        }
        free.push((start, len));
        previous_end = end;
    }
    let mut occupied = Vec::<(usize, usize, usize)>::with_capacity(occupied_count);
    previous_end = 0;
    for _ in 0..occupied_count {
        let start =
            usize::try_from(heap_image_take_u64(bytes, &mut offset).ok_or("occupied range")?)
                .map_err(|_| "occupied range")?;
        let len = usize::try_from(heap_image_take_u64(bytes, &mut offset).ok_or("occupied range")?)
            .map_err(|_| "occupied range")?;
        let end = start.checked_add(len).ok_or("occupied range")?;
        let data_end = offset.checked_add(len).ok_or("occupied data")?;
        if start < previous_end || end > heap_len || data_end > bytes.len() {
            return Err("occupied range geometry");
        }
        occupied.push((start, len, offset));
        previous_end = end;
        offset = data_end;
    }
    if offset != bytes.len() {
        return Err("trailing guest heap image bytes");
    }
    let mut cover = Vec::<(usize, usize)>::with_capacity(free.len() + occupied.len());
    cover.extend(free.iter().copied());
    cover.extend(occupied.iter().map(|&(start, len, _)| (start, len)));
    cover.sort_by_key(|range| range.0);
    let mut covered = 0usize;
    for (start, len) in cover {
        if start != covered {
            return Err("guest heap image has a gap or overlap");
        }
        covered = covered.checked_add(len).ok_or("guest heap coverage")?;
    }
    if covered != heap_len {
        return Err("guest heap image is incomplete");
    }

    let page = hv_guest_allocator_page(vm_id).ok_or("unsupported guest heap")?;
    {
        let guard = page.allocator.lock();
        if guard.initialized || guard.heap_len != 0 || hv_guest_dma_ranges_pinned(vm_id) {
            return Err("guest heap slot is already configured");
        }
    }
    let arena = phys::reserve_heap_arena_at(phys_start, heap_len)
        .ok_or("original guest heap physical range is unavailable")?;
    if arena.virt_start != virt_start {
        let _ = phys::free_phys_range(arena.phys_start, arena.length);
        return Err("guest heap HHDM address changed");
    }
    for &(start, len, data_offset) in occupied.iter() {
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes[data_offset..data_offset + len].as_ptr(),
                (virt_start + start) as *mut u8,
                len,
            );
        }
    }
    let mut guard = page.allocator.lock();
    if guard.initialized || guard.heap_len != 0 || hv_guest_dma_ranges_pinned(vm_id) {
        // Close the check/reserve/install window. Another lifecycle operation
        // may have populated this slot while the exact arena was reserved.
        drop(guard);
        let _ = phys::free_phys_range(arena.phys_start, arena.length);
        return Err("guest heap slot changed during restore");
    }
    guard.install_heap(arena.virt_start, arena.phys_start as usize, arena.length);
    guard.initialized = true;
    let mut next = None;
    for &(start, len) in free.iter().rev() {
        let ptr = (virt_start + start) as *mut FreeBlock;
        unsafe {
            ptr.write(FreeBlock { size: len, next });
            next = NonNull::new(ptr);
        }
    }
    guard.head = next;
    drop(guard);
    publish_hv_guest_heap_range(vm_id, arena.virt_start, arena.length);
    page.ready.store(1, Ordering::Release);
    Ok(())
}

/// Release an offline guest heap. Callers must first drop all host-owned
/// Blueprint values allocated from this domain.
pub fn release_hv_guest_heap_for_vm(vm_id: u8) -> bool {
    let Some(page) = hv_guest_allocator_page(vm_id) else {
        return false;
    };
    let arena = {
        let mut guard = page.allocator.lock();
        if guard.heap_source != HeapSourceKind::Arena
            || guard.heap_len == 0
            || hv_guest_dma_ranges_pinned(vm_id)
        {
            return false;
        }
        let arena = HeapArena {
            phys_start: guard.heap_phys_start as u64,
            virt_start: guard.heap_virt_start,
            length: guard.heap_len,
        };
        *guard = FreeList::new();
        arena
    };
    page.ready.store(0, Ordering::Release);
    publish_hv_guest_heap_range(vm_id, 0, 0);
    phys::free_phys_range(arena.phys_start, arena.length)
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

    for page in HV_GUEST_ALLOCATOR_SHARED_PAGES.iter() {
        let mut guard = page.allocator.lock();
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
    if crate::log_os::flags::BOOT_INFO_LOGS {
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
