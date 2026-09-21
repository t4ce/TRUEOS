//! WC3-private, generic 32-bit address-space bookkeeping.
//!
//! This module intentionally contains no PE, Windows, or Warcraft policy.
//! Handles are capabilities only while presented by the Blueprint VM that
//! created them; a numeric handle from another VM is rejected.

use alloc::vec::Vec;
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};
use spin::Mutex;

use crate::hv::memory::PAGE_SIZE_4K;
use crate::phys::HeapArena;
use crate::r::static_slots::StaticSlots;
use v::bp_abi::{
    TrueosX86DebugRegistersV1,
    TrueosX86ExitV1,
    TrueosX86RegistersV1,
};

pub(super) const ERR_INVALID: i32 = -22;
pub(super) const ERR_NO_MEMORY: i32 = -12;
pub(super) const ERR_DENIED: i32 = -13;
pub(super) const ERR_NOT_FOUND: i32 = -2;
pub(super) const ERR_BUSY: i32 = -16;
pub(super) const PERMISSION_READ: u32 = 1;
pub(super) const PERMISSION_WRITE: u32 = 2;
pub(super) const PERMISSION_EXECUTE: u32 = 4;
const PERMISSION_MASK: u32 = PERMISSION_READ | PERMISSION_WRITE | PERMISSION_EXECUTE;
const PAE_PRESENT: u64 = 1 << 0;
const PAE_WRITABLE: u64 = 1 << 1;
const PAE_PDPT_BYTES: usize = 32;
const PAE_PDPT_SLOTS: usize = PAGE_SIZE_4K / PAE_PDPT_BYTES;
const WAR3_DWORD_SCAN_STEP_START: u64 = 0x0045_b0a2;
const WAR3_DWORD_SCAN_STEP_END: u64 = 0x0045_b0df;

fn quiet_war3_dword_scan_debug_exit(exit: &crate::hv::TransientProtected32Exit) -> bool {
    exit.launch.exit_reason & 0xffff == 0
        && exit.interruption_info & (1 << 31) != 0
        && exit.interruption_info & 0xff == 1
        && (WAR3_DWORD_SCAN_STEP_START..=WAR3_DWORD_SCAN_STEP_END)
            .contains(&exit.launch.guest_rip)
}

struct Mapping {
    start: u32,
    len: u32,
    permissions: u32,
    backing: MappingBacking,
}

/// A mapping is application-owned memory, not a new global-PMM arena.  The
/// Blueprint VM has already reserved this heap before it enters the carrier.
struct MappingBacking {
    // An integer keeps the global runtime Send; this pointer is owned by the
    // VM guest heap and only dereferenced while its runtime lock is held.
    ptr: usize,
    phys_start: u64,
}

impl MappingBacking {
    fn new(owner: u8, len: usize) -> Result<Self, i32> {
        let layout = Layout::from_size_align(len, PAGE_SIZE_4K).map_err(|_| ERR_INVALID)?;
        let ptr = unsafe { crate::allocators::alloc_raw_hv_guest(owner, layout) };
        if ptr.is_null() {
            let stats = crate::allocators::hv_guest_heap_stats(owner);
            crate::log_warn!(
                target: "hv";
                "x86 backing OOM vm={} requested={} guest_heap_total={} free={} largest_free={} free_blocks={}\n",
                owner,
                len,
                stats.usable_total,
                stats.free_bytes,
                stats.largest_free_block,
                stats.free_blocks,
            );
            return Err(ERR_NO_MEMORY);
        }
        let stats = crate::allocators::hv_guest_heap_stats(owner);
        let heap_translation = || {
            let start = stats.heap_start;
            let end = stats.heap_end;
            let address = ptr as usize;
            let allocation_end = address.checked_add(len)?;
            if !stats.initialized || address < start || allocation_end > end {
                return None;
            }
            (stats.phys_start as u64).checked_add((address - start) as u64)
        };
        // The normal HHDM translation is preferred.  A Blueprint carrier may
        // run with a narrowed host mapping, so retain the VM heap's published
        // virtual/physical bounds as the authoritative fallback.
        let Some(phys_start) = crate::phys::virt_to_phys_checked(ptr).or_else(heap_translation) else {
            unsafe { crate::allocators::dealloc_raw(ptr) };
            return Err(ERR_INVALID);
        };
        unsafe { core::ptr::write_bytes(ptr, 0, len) };
        Ok(Self { ptr: ptr as usize, phys_start })
    }
}

impl Drop for MappingBacking {
    fn drop(&mut self) {
        unsafe { crate::allocators::dealloc_raw(self.ptr as *mut u8) };
    }
}

/// The carrier page tables live in their owner's already EPT-admitted guest
/// heap, alongside the x86 mapping backing. Retain ownership here while the
/// Copy reference below is used to wire the PAE tables.
struct CarrierPage {
    ptr: usize,
    phys_start: u64,
}

#[derive(Copy, Clone)]
struct CarrierPageRef {
    ptr: usize,
    phys_start: u64,
}

impl Drop for CarrierPage {
    fn drop(&mut self) {
        unsafe { crate::allocators::dealloc_raw(self.ptr as *mut u8) };
    }
}

impl CarrierPage {
    fn reference(&self) -> CarrierPageRef {
        CarrierPageRef { ptr: self.ptr, phys_start: self.phys_start }
    }
}

struct AddressSpace {
    handle: u64,
    owner: u8,
    mappings: Vec<Mapping>,
    generation: u64,
}

struct Context {
    handle: u64,
    owner: u8,
    address_space: u64,
    registers: TrueosX86RegistersV1,
    debug_registers: TrueosX86DebugRegistersV1,
    extended_state: crate::hv::vmx::VmxExtendedState,
    parked: bool,
    cancelled: bool,
    carrier_cr3: Option<u64>,
    carrier_pages: Vec<CarrierPage>,
    carrier_pdpt_slot: Option<u8>,
    carrier_generation: u64,
    last_carrier_slot: Option<usize>,
    carrier_migration_observed: bool,
    admitted: bool,
}

impl Drop for Context {
    fn drop(&mut self) {
        if let Some(slot) = self.carrier_pdpt_slot.take() {
            release_carrier_pdpt(self.owner, slot);
        }
    }
}

struct Runtime {
    next_handle: u64,
    spaces: Vec<AddressSpace>,
    contexts: Vec<Context>,
}

impl Runtime {
    const fn new() -> Self {
        Self {
            // Keep the carrier-visible slot entirely NOBITS. `fresh_handle`
            // canonicalizes this first zero to the public handle value 1.
            next_handle: 0,
            spaces: Vec::new(),
            contexts: Vec::new(),
        }
    }

    fn fresh_handle(&mut self) -> u64 {
        let handle = self.next_handle.max(1);
        self.next_handle = self.next_handle.wrapping_add(1).max(1);
        handle
    }
}

// The hull has private RW/BSS, while a Tokio carrier executes against the
// host kernel mapping.  Keep each Blueprint's x86 capabilities in a page that
// the hull deliberately retains as kernel-backed shared state.  The mapping
// payloads themselves are already allocated from that Blueprint's guest heap.
#[repr(C, align(4096))]
struct RuntimeSlot {
    init: AtomicU8,
    runtime: UnsafeCell<MaybeUninit<Mutex<Runtime>>>,
}

unsafe impl Sync for RuntimeSlot {}

const RUNTIME_UNINITIALIZED: u8 = 0;
const RUNTIME_INITIALIZING: u8 = 1;
const RUNTIME_READY: u8 = 2;

impl RuntimeSlot {
    const fn new() -> Self {
        Self {
            init: AtomicU8::new(RUNTIME_UNINITIALIZED),
            runtime: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    fn runtime(&self) -> &'static Mutex<Runtime> {
        loop {
            match self.init.load(Ordering::Acquire) {
                RUNTIME_READY => return unsafe { &*self.runtime.get().cast::<Mutex<Runtime>>() },
                RUNTIME_UNINITIALIZED => {
                    if self
                        .init
                        .compare_exchange(
                            RUNTIME_UNINITIALIZED,
                            RUNTIME_INITIALIZING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        unsafe { self.runtime.get().write(MaybeUninit::new(Mutex::new(Runtime::new()))) };
                        self.init.store(RUNTIME_READY, Ordering::Release);
                    }
                }
                RUNTIME_INITIALIZING => core::hint::spin_loop(),
                _ => unreachable!("x86 runtime slot state"),
            }
        }
    }
}

#[unsafe(link_section = ".bss.blueprint_x86_runtimes")]
static RUNTIMES: [RuntimeSlot; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { RuntimeSlot::new() }; crate::allcaps::hv::VM_ID_LIMIT];

static CARRIER_PDPT_ARENAS: StaticSlots<
    Option<HeapArena>,
    { crate::allcaps::hv::VM_ID_LIMIT },
> = StaticSlots::from_slots(
    [const { Mutex::new(None) }; crate::allcaps::hv::VM_ID_LIMIT],
);
static CARRIER_PDPT_IN_USE: StaticSlots<
    [u64; 2],
    { crate::allcaps::hv::VM_ID_LIMIT },
> = StaticSlots::from_slots(
    [const { Mutex::new([0; 2]) }; crate::allcaps::hv::VM_ID_LIMIT],
);

pub(super) fn carrier_pdpt_span(vm_id: u8) -> Result<(u64, usize), &'static str> {
    let arena_lock = CARRIER_PDPT_ARENAS
        .get_u8(vm_id)
        .ok_or("wc3 carrier pdpt vm id")?;
    let mut arena = arena_lock.lock();
    if arena.is_none() {
        let allocated = crate::phys::reserve_legacy_pae_root_arena(PAGE_SIZE_4K, PAGE_SIZE_4K)
            .ok_or("wc3 carrier pdpt low arena")?;
        if allocated.phys_start.saturating_add(allocated.length as u64) > (1u64 << 32) {
            return Err("wc3 carrier pdpt above 4g");
        }
        unsafe { core::ptr::write_bytes(allocated.virt_start as *mut u8, 0, allocated.length) };
        *arena = Some(allocated);
    }
    let arena = arena.ok_or("wc3 carrier pdpt arena")?;
    Ok((arena.phys_start, arena.length))
}

fn reserve_carrier_pdpt(vm_id: u8) -> Result<(u8, CarrierPageRef), i32> {
    let (phys_start, _) = carrier_pdpt_span(vm_id).map_err(|_| ERR_NO_MEMORY)?;
    let arena = CARRIER_PDPT_ARENAS
        .get_u8(vm_id)
        .and_then(|slot| *slot.lock())
        .ok_or(ERR_NO_MEMORY)?;
    let in_use = CARRIER_PDPT_IN_USE.get_u8(vm_id).ok_or(ERR_DENIED)?;
    let mut words = in_use.lock();
    for slot in 0..PAE_PDPT_SLOTS {
        let word = slot / 64;
        let bit = 1u64 << (slot % 64);
        if words[word] & bit != 0 {
            continue;
        }
        words[word] |= bit;
        let offset = slot * PAE_PDPT_BYTES;
        let ptr = unsafe { (arena.virt_start as *mut u8).add(offset) };
        unsafe { core::ptr::write_bytes(ptr, 0, PAE_PDPT_BYTES) };
        return Ok((
            u8::try_from(slot).map_err(|_| ERR_NO_MEMORY)?,
            CarrierPageRef {
                ptr: ptr as usize,
                phys_start: phys_start + offset as u64,
            },
        ));
    }
    Err(ERR_NO_MEMORY)
}

fn release_carrier_pdpt(vm_id: u8, slot: u8) {
    let Some(in_use) = CARRIER_PDPT_IN_USE.get_u8(vm_id) else {
        return;
    };
    let slot = usize::from(slot);
    if slot >= PAE_PDPT_SLOTS {
        return;
    }
    let mut words = in_use.lock();
    words[slot / 64] &= !(1u64 << (slot % 64));
}

fn runtime_for(owner: u8) -> Result<&'static Mutex<Runtime>, i32> {
    RUNTIMES.get(owner as usize).map(RuntimeSlot::runtime).ok_or(ERR_DENIED)
}

pub(crate) fn purge_one_shot_state(vm_id: u8) {
    let Some(slot) = RUNTIMES.get(vm_id as usize) else {
        return;
    };

    let (spaces, contexts) = if slot.init.load(Ordering::Acquire) == RUNTIME_READY {
        let retired = {
            let runtime = unsafe { &*slot.runtime.get().cast::<Mutex<Runtime>>() };
            let mut runtime = runtime.lock();
            core::mem::replace(&mut *runtime, Runtime::new())
        };
        let counts = (retired.spaces.len(), retired.contexts.len());
        // Mapping and carrier-page drops must run while this VM's guest heap
        // is still installed. Their allocations belong to that heap.
        drop(retired);
        counts
    } else {
        (0, 0)
    };

    if let Some(in_use) = CARRIER_PDPT_IN_USE.get_u8(vm_id) {
        *in_use.lock() = [0; 2];
    }
    let carrier_pdpt_released = CARRIER_PDPT_ARENAS
        .get_u8(vm_id)
        .and_then(|arena| arena.lock().take())
        .is_some_and(|arena| crate::phys::free_phys_range(arena.phys_start, arena.length));

    if spaces != 0 || contexts != 0 || carrier_pdpt_released {
        crate::log_info!(target: "hv";
            "wc3: one-shot purge vm={} address_spaces={} contexts={} carrier_pdpt_released={}\n",
            vm_id,
            spaces,
            contexts,
            carrier_pdpt_released as u8,
        );
    }
}

/// Kernel-backed metadata span for one Blueprint's carrier-visible x86
/// runtime.  `memory` retains this page when it creates the Hull's private
/// RW/BSS image, so a Tokio carrier and the Hull see the same capabilities.
pub(crate) fn shared_runtime_state_span(vm_id: u8) -> Option<(u64, usize)> {
    RUNTIMES
        .get(vm_id as usize)
        .map(|slot| ((slot as *const RuntimeSlot) as u64, core::mem::size_of::<RuntimeSlot>()))
}

fn owner() -> Result<u8, i32> {
    // A Blueprint may enter this ABI through its hull or through a Tokio
    // carrier.  The execution owner is invariant across that handoff; the
    // LAPIC-local VM marker is not.
    crate::hv::current_guest_execution_context_vm_id().ok_or(ERR_DENIED)
}

fn checked_range(start: u32, len: u32) -> Result<u32, i32> {
    if len == 0 || start & (PAGE_SIZE_4K as u32 - 1) != 0 || len & (PAGE_SIZE_4K as u32 - 1) != 0 {
        return Err(ERR_INVALID);
    }
    start.checked_add(len).ok_or(ERR_INVALID)
}

fn mapping_index(
    space: &AddressSpace,
    address: u32,
    len: usize,
    required: u32,
) -> Result<usize, i32> {
    let end = address
        .checked_add(u32::try_from(len).map_err(|_| ERR_INVALID)?)
        .ok_or(ERR_INVALID)?;
    space
        .mappings
        .iter()
        .position(|mapping| {
            mapping.permissions & required == required
                && address >= mapping.start
                && end <= mapping.start + mapping.len
        })
        .ok_or(ERR_NOT_FOUND)
}

pub(super) fn address_space_create(out: &mut u64) -> Result<(), i32> {
    let owner = owner()?;
    let mut runtime = runtime_for(owner)?.lock();
    let handle = runtime.fresh_handle();
    runtime.spaces.push(AddressSpace {
        handle,
        owner,
        mappings: Vec::new(),
        generation: 1,
    });
    *out = handle;
    Ok(())
}

pub(super) fn address_space_destroy(handle: u64) -> Result<(), i32> {
    let owner = owner()?;
    let mut runtime = runtime_for(owner)?.lock();
    let index = runtime
        .spaces
        .iter()
        .position(|space| space.handle == handle)
        .ok_or(ERR_NOT_FOUND)?;
    if runtime.spaces[index].owner != owner {
        return Err(ERR_DENIED);
    }
    if runtime
        .contexts
        .iter()
        .any(|context| context.address_space == handle)
    {
        return Err(ERR_BUSY);
    }
    runtime.spaces.swap_remove(index);
    Ok(())
}

pub(super) fn address_space_map(
    handle: u64,
    start: u32,
    len: u32,
    permissions: u32,
) -> Result<(), i32> {
    let owner = owner()?;
    let end = checked_range(start, len)?;
    if permissions == 0 || permissions & !PERMISSION_MASK != 0 {
        return Err(ERR_INVALID);
    }
    let mut runtime = runtime_for(owner)?.lock();
    let space = runtime
        .spaces
        .iter_mut()
        .find(|space| space.handle == handle)
        .ok_or(ERR_NOT_FOUND)?;
    if space.owner != owner {
        return Err(ERR_DENIED);
    }
    if space
        .mappings
        .iter()
        .any(|mapping| start < mapping.start + mapping.len && mapping.start < end)
    {
        return Err(ERR_BUSY);
    }
    let backing = MappingBacking::new(owner, len as usize)?;
    space.mappings.push(Mapping {
        start,
        len,
        permissions,
        backing,
    });
    space.generation = space.generation.wrapping_add(1).max(1);
    Ok(())
}

pub(super) fn address_space_unmap(handle: u64, start: u32, len: u32) -> Result<(), i32> {
    let owner = owner()?;
    checked_range(start, len)?;
    let mut runtime = runtime_for(owner)?.lock();
    let space = runtime
        .spaces
        .iter_mut()
        .find(|space| space.handle == handle)
        .ok_or(ERR_NOT_FOUND)?;
    if space.owner != owner {
        return Err(ERR_DENIED);
    }
    let index = space
        .mappings
        .iter()
        .position(|mapping| mapping.start == start && mapping.len == len)
        .ok_or(ERR_NOT_FOUND)?;
    space.mappings.swap_remove(index);
    space.generation = space.generation.wrapping_add(1).max(1);
    Ok(())
}

pub(super) fn address_space_read(handle: u64, address: u32, out: &mut [u8]) -> Result<usize, i32> {
    let owner = owner()?;
    let runtime = runtime_for(owner)?.lock();
    let space = runtime
        .spaces
        .iter()
        .find(|space| space.handle == handle)
        .ok_or(ERR_NOT_FOUND)?;
    if space.owner != owner {
        return Err(ERR_DENIED);
    }
    let index = mapping_index(space, address, out.len(), PERMISSION_READ)?;
    let mapping = &space.mappings[index];
    let offset = (address - mapping.start) as usize;
    unsafe {
        core::ptr::copy_nonoverlapping(
            (mapping.backing.ptr as *const u8).add(offset),
            out.as_mut_ptr(),
            out.len(),
        );
    }
    Ok(out.len())
}

pub(super) fn address_space_write(handle: u64, address: u32, data: &[u8]) -> Result<usize, i32> {
    let owner = owner()?;
    let runtime = runtime_for(owner)?.lock();
    let space = runtime
        .spaces
        .iter()
        .find(|space| space.handle == handle)
        .ok_or(ERR_NOT_FOUND)?;
    if space.owner != owner {
        return Err(ERR_DENIED);
    }
    let index = mapping_index(space, address, data.len(), PERMISSION_WRITE)?;
    let mapping = &space.mappings[index];
    let offset = (address - mapping.start) as usize;
    unsafe {
        core::ptr::copy_nonoverlapping(
            data.as_ptr(),
            (mapping.backing.ptr as *mut u8).add(offset),
            data.len(),
        );
    }
    Ok(data.len())
}

pub(super) fn context_create(
    address_space: u64,
    registers: TrueosX86RegistersV1,
    out: &mut u64,
) -> Result<(), i32> {
    let owner = owner()?;
    let mut runtime = runtime_for(owner)?.lock();
    if !runtime
        .spaces
        .iter()
        .any(|space| space.handle == address_space && space.owner == owner)
    {
        return Err(ERR_DENIED);
    }
    let handle = runtime.fresh_handle();
    runtime.contexts.push(Context {
        handle,
        owner,
        address_space,
        registers,
        debug_registers: TrueosX86DebugRegistersV1::default(),
        extended_state: crate::hv::vmx::clean_guest_extended_state(),
        parked: false,
        cancelled: false,
        carrier_cr3: None,
        carrier_pages: Vec::new(),
        carrier_pdpt_slot: None,
        carrier_generation: 0,
        last_carrier_slot: None,
        carrier_migration_observed: false,
        admitted: false,
    });
    *out = handle;
    Ok(())
}

pub(super) fn context_destroy(handle: u64) -> Result<(), i32> {
    let owner = owner()?;
    let mut runtime = runtime_for(owner)?.lock();
    let index = runtime
        .contexts
        .iter()
        .position(|context| context.handle == handle)
        .ok_or(ERR_NOT_FOUND)?;
    if runtime.contexts[index].owner != owner {
        return Err(ERR_DENIED);
    }
    runtime.contexts.swap_remove(index);
    Ok(())
}

fn context_mut(runtime: &mut Runtime, handle: u64, owner: u8) -> Result<&mut Context, i32> {
    let context = runtime
        .contexts
        .iter_mut()
        .find(|context| context.handle == handle)
        .ok_or(ERR_NOT_FOUND)?;
    if context.owner != owner {
        return Err(ERR_DENIED);
    }
    Ok(context)
}

pub(super) fn context_registers_get(
    handle: u64,
    out: &mut TrueosX86RegistersV1,
) -> Result<(), i32> {
    let owner = owner()?;
    *out = context_mut(&mut runtime_for(owner)?.lock(), handle, owner)?.registers;
    Ok(())
}

pub(super) fn context_registers_set(
    handle: u64,
    registers: TrueosX86RegistersV1,
) -> Result<(), i32> {
    let owner = owner()?;
    context_mut(&mut runtime_for(owner)?.lock(), handle, owner)?.registers = registers;
    Ok(())
}

pub(super) fn context_debug_registers_get(
    handle: u64,
    out: &mut TrueosX86DebugRegistersV1,
) -> Result<(), i32> {
    let owner = owner()?;
    *out = context_mut(&mut runtime_for(owner)?.lock(), handle, owner)?.debug_registers;
    Ok(())
}

pub(super) fn context_debug_registers_set(
    handle: u64,
    registers: TrueosX86DebugRegistersV1,
) -> Result<(), i32> {
    let owner = owner()?;
    context_mut(&mut runtime_for(owner)?.lock(), handle, owner)?.debug_registers = registers;
    Ok(())
}

pub(super) fn context_park(handle: u64) -> Result<(), i32> {
    let owner = owner()?;
    context_mut(&mut runtime_for(owner)?.lock(), handle, owner)?.parked = true;
    Ok(())
}

pub(super) fn context_cancel(handle: u64) -> Result<(), i32> {
    let owner = owner()?;
    context_mut(&mut runtime_for(owner)?.lock(), handle, owner)?.cancelled = true;
    Ok(())
}

pub(super) fn context_execute(
    handle: u64,
    out: &mut TrueosX86ExitV1,
    resume: bool,
) -> Result<(), i32> {
    let owner = owner()?;
    let (registers, debug_registers, address_space, cancelled, admitted, mut extended_state) = {
        let runtime = runtime_for(owner)?.lock();
        let context = runtime
            .contexts
            .iter()
            .find(|context| context.handle == handle && context.owner == owner)
            .ok_or(ERR_NOT_FOUND)?;
        (
            context.registers,
            context.debug_registers,
            context.address_space,
            context.cancelled,
            context.admitted,
            context.extended_state,
        )
    };
    if resume != admitted {
        return Err(if resume { ERR_BUSY } else { ERR_INVALID });
    }
    if cancelled {
        *out = TrueosX86ExitV1 {
            kind: 5,
            registers,
            ..TrueosX86ExitV1::default()
        };
        return Ok(());
    }

    let mut runtime = runtime_for(owner)?.lock();
    let context_index = runtime
        .contexts
        .iter()
        .position(|context| context.handle == handle && context.owner == owner)
        .ok_or(ERR_NOT_FOUND)?;
    let space = runtime
        .spaces
        .iter()
        .find(|space| space.handle == address_space && space.owner == owner)
        .ok_or(ERR_DENIED)?;
    let generation = space.generation;
    let mappings = space
        .mappings
        .iter()
        .map(|mapping| (mapping.start, mapping.len, mapping.permissions, mapping.backing.phys_start))
        .collect::<Vec<_>>();
    let context = &mut runtime.contexts[context_index];
    if context.carrier_generation != generation {
        context.carrier_cr3 = None;
        context.carrier_pages = Vec::new();
    }
    if context.carrier_cr3.is_none() {
        let pdpt = if let Some(slot) = context.carrier_pdpt_slot {
            let (_, span_bytes) = carrier_pdpt_span(owner).map_err(|_| ERR_NO_MEMORY)?;
            let arena = CARRIER_PDPT_ARENAS
                .get_u8(owner)
                .and_then(|arena| *arena.lock())
                .ok_or(ERR_NO_MEMORY)?;
            let offset = usize::from(slot) * PAE_PDPT_BYTES;
            if offset + PAE_PDPT_BYTES > span_bytes {
                return Err(ERR_INVALID);
            }
            let ptr = unsafe { (arena.virt_start as *mut u8).add(offset) };
            unsafe { core::ptr::write_bytes(ptr, 0, PAE_PDPT_BYTES) };
            CarrierPageRef {
                ptr: ptr as usize,
                phys_start: arena.phys_start + offset as u64,
            }
        } else {
            let (slot, pdpt) = reserve_carrier_pdpt(owner)?;
            context.carrier_pdpt_slot = Some(slot);
            pdpt
        };
        let (cr3, pages) = build_carrier_page_tables(owner, pdpt, &mappings)?;
        context.carrier_cr3 = Some(cr3);
        context.carrier_pages = pages;
        context.carrier_generation = generation;
    }
    let cr3 = context.carrier_cr3.ok_or(ERR_NOT_FOUND)?;
    drop(runtime);

    let (exit, returned_debug_registers, entered, carrier_slot) =
        run_x86_context_on_carrier(owner, cr3, registers, debug_registers, &mut extended_state)?;
    let mut runtime = runtime_for(owner)?.lock();
    let context = context_mut(&mut runtime, handle, owner)?;
    context.registers = exit.registers;
    if entered {
        context.debug_registers = returned_debug_registers;
        context.extended_state = extended_state;
        if context.last_carrier_slot.is_some_and(|previous| previous != carrier_slot)
            && !context.carrier_migration_observed
        {
            context.carrier_migration_observed = true;
            crate::log_important!(
                target: "hv";
                "x86 context carrier migration vm={} context={} from_slot={} to_slot={} xstate=context-owned\n",
                owner,
                handle,
                context.last_carrier_slot.unwrap_or(carrier_slot),
                carrier_slot,
            );
        }
        context.last_carrier_slot = Some(carrier_slot);
    }
    context.admitted = true;
    *out = exit;
    Ok(())
}

fn build_carrier_page_tables(
    owner: u8,
    pdpt: CarrierPageRef,
    mappings: &[(u32, u32, u32, u64)],
) -> Result<(u64, Vec<CarrierPage>), i32> {
    let mut pages = Vec::new();
    let alloc_page = |pages: &mut Vec<CarrierPage>| -> Result<CarrierPageRef, i32> {
        let layout = Layout::from_size_align(PAGE_SIZE_4K, PAGE_SIZE_4K).map_err(|_| ERR_INVALID)?;
        let ptr = unsafe { crate::allocators::alloc_raw_hv_guest(owner, layout) };
        if ptr.is_null() {
            return Err(ERR_NO_MEMORY);
        }
        let stats = crate::allocators::hv_guest_heap_stats(owner);
        let heap_translation = || {
            let start = stats.heap_start;
            let address = ptr as usize;
            let end = address.checked_add(PAGE_SIZE_4K)?;
            if !stats.initialized || address < start || end > stats.heap_end {
                return None;
            }
            (stats.phys_start as u64).checked_add((address - start) as u64)
        };
        let Some(phys_start) = crate::phys::virt_to_phys_checked(ptr).or_else(heap_translation) else {
            unsafe { crate::allocators::dealloc_raw(ptr) };
            return Err(ERR_INVALID);
        };
        unsafe { core::ptr::write_bytes(ptr, 0, PAGE_SIZE_4K) };
        pages.push(CarrierPage { ptr: ptr as usize, phys_start });
        Ok(pages.last().ok_or(ERR_NO_MEMORY)?.reference())
    };
    let mut pds = [pdpt; 4];
    for (index, pd) in pds.iter_mut().enumerate() {
        *pd = alloc_page(&mut pages)?;
        // In legacy 32-bit PAE paging, PDPTE bits 1 and 2 are reserved.  A
        // value of 0x7 therefore raises #PF(RSVD) on the first page walk.
        write_entry(pdpt, index, physical_address(*pd) | PAE_PRESENT)?;
    }

    let mut ptes: Vec<(u32, CarrierPageRef)> = Vec::new();
    for &(start, len, permissions, phys_start) in mappings {
        let end = start.checked_add(len).ok_or(ERR_INVALID)?;
        let mut va = start;
        while va < end {
            let pd_index = (va as usize >> 21) & 0x7ff;
            let pd_slot = pd_index & 0x1ff;
            let pd_page = &pds[pd_index >> 9];
            let pt = if let Some((_, page)) = ptes.iter().find(|(index, _)| *index == pd_index as u32) {
                *page
            } else {
                let page = alloc_page(&mut pages)?;
                ptes.push((pd_index as u32, page));
                // The carrier executes at CPL0, so keep its mappings
                // supervisor-only.  In particular, do not inherit a U/S bit
                // into a guest whose CR4 may retain host SMEP/SMAP policy.
                write_entry(
                    *pd_page,
                    pd_slot,
                    physical_address(page) | PAE_PRESENT | PAE_WRITABLE,
                )?;
                page
            };
            let page_offset = (va - start) as u64;
            let guest_physical = phys_start.checked_add(page_offset).ok_or(ERR_INVALID)?;
            let mut flags = PAE_PRESENT;
            if permissions & PERMISSION_WRITE != 0 { flags |= PAE_WRITABLE; }
            write_entry(pt, (va as usize >> 12) & 0x1ff, guest_physical | flags)?;
            va = va.checked_add(PAGE_SIZE_4K as u32).ok_or(ERR_INVALID)?;
        }
    }
    // The carrier runs 32-bit protected mode with PAE enabled, so CR3 points
    // directly at the four-entry PDPTE page (there is no long-mode PML4).
    Ok((physical_address(pdpt), pages))
}

fn physical_address(page: CarrierPageRef) -> u64 {
    page.phys_start
}

fn write_entry(page: CarrierPageRef, index: usize, value: u64) -> Result<(), i32> {
    if index >= 512 { return Err(ERR_INVALID); }
    unsafe { (page.ptr as *mut u64).add(index).write(value) };
    Ok(())
}

fn run_x86_context_on_carrier(
    owner: u8,
    cr3: u64,
    registers: TrueosX86RegistersV1,
    debug_registers: TrueosX86DebugRegistersV1,
    extended_state: &mut crate::hv::vmx::VmxExtendedState,
) -> Result<(TrueosX86ExitV1, TrueosX86DebugRegistersV1, bool, usize), i32> {
    let carrier_slot = crate::percpu::current_slot();
    let exit = crate::hv::run_transient_protected32(
        owner, cr3, registers.eip, registers.esp, registers.eflags, registers.fs_base,
        debug_registers,
        crate::hv::vmx::GuestRegisters {
            rax: registers.eax as u64, rbx: registers.ebx as u64,
            rcx: registers.ecx as u64, rdx: registers.edx as u64,
            rsi: registers.esi as u64, rdi: registers.edi as u64,
            rbp: registers.ebp as u64, ..Default::default()
        },
        extended_state,
    ).map_err(|_| ERR_DENIED)?;
    let entered = exit.launch.entered != 0;
    let hot_war3_divide = exit.launch.guest_rip == 0x0045_ae47
        && exit.launch.exit_reason & 0xffff == 0
        && exit.interruption_info & 0xff == 0;
    let quiet_war3_dword_scan = quiet_war3_dword_scan_debug_exit(&exit);
    // Keep raw hardware evidence even when interruption-info is invalid. The
    // public exception payload otherwise deliberately discards invalid fields.
    if !hot_war3_divide
        && !quiet_war3_dword_scan
        && !matches!(exit.launch.exit_reason & 0xffff, crate::hv::vmx::VMEXIT_REASON_VMCALL | 52)
    {
        crate::log_important!(
            target: "hv";
            "x86 raw exit vm={} entered={} launch_failed={} reason=0x{:08X} instr_err=0x{:X} qualification=0x{:016X} intr_info=0x{:08X} intr_error=0x{:08X} cr2=0x{:016X} cr3=0x{:016X} rip=0x{:08X} rsp=0x{:08X} instruction_len={}\n",
            owner, exit.launch.entered, exit.launch.launch_failed,
            exit.launch.exit_reason, exit.launch.instr_err,
            exit.launch.exit_qualification, exit.interruption_info,
            exit.interruption_error_code, exit.guest_cr2, exit.cr3,
            exit.launch.guest_rip, exit.rsp, exit.instruction_len,
        );
    }
    if exit.launch.exit_reason & 0xffff == 48 {
        crate::log_important!(
            target: "hv";
            "x86 ept violation vm={} rip=0x{:08X} cr3=0x{:016X} gpa=0x{:016X} gla=0x{:016X} qualification=0x{:X}\n",
            owner, exit.launch.guest_rip, exit.cr3, exit.guest_physical,
            exit.guest_linear, exit.launch.exit_qualification,
        );
    }
    if !hot_war3_divide && !quiet_war3_dword_scan && exit.launch.exit_reason & 0xffff == 0 {
        let interruption_info = exit.interruption_info;
        let vector = (interruption_info & 0xff) as u8;
        let valid = interruption_info & (1 << 31) != 0;
        let error_code_valid = valid && interruption_info & (1 << 11) != 0;
        crate::log_important!(
            target: "hv";
            "x86 exception vm={} rip=0x{:08X} vector={} name={} error_valid={} error=0x{:X} intr_info=0x{:08X}\n",
            owner,
            exit.launch.guest_rip,
            vector,
            if valid { crate::hv::vmx::decode_exception_vector(vector) } else { "invalid-interruption-info" },
            error_code_valid as u8,
            exit.interruption_error_code,
            interruption_info,
        );
    }
    let exit_reason = exit.launch.exit_reason & 0xffff;
    let resumed_eip = if exit_reason == crate::hv::vmx::VMEXIT_REASON_VMCALL {
        exit.launch
            .guest_rip
            .checked_add(exit.instruction_len)
            .ok_or(ERR_INVALID)?
    } else {
        exit.launch.guest_rip
    };
    let (detail, qualification) = pack_exit_metadata(
        exit.launch.exit_reason,
        exit.launch.exit_qualification,
        exit.interruption_info,
        exit.interruption_error_code,
        exit.launch.exit_qualification,
    );
    Ok((TrueosX86ExitV1 {
        kind: match exit_reason {
            crate::hv::vmx::VMEXIT_REASON_VMCALL => 1, 0 => 2, 48 => 3, 0x0c => 4, _ => 255,
        },
        detail,
        qualification,
        registers: TrueosX86RegistersV1 {
            eax: exit.registers.rax as u32, ebx: exit.registers.rbx as u32,
            ecx: exit.registers.rcx as u32, edx: exit.registers.rdx as u32,
            esi: exit.registers.rsi as u32, edi: exit.registers.rdi as u32,
            ebp: exit.registers.rbp as u32, esp: exit.rsp as u32,
            eip: u32::try_from(resumed_eip).map_err(|_| ERR_INVALID)?, eflags: exit.rflags as u32,
            fs_base: exit.fs_base as u32,
        },
    }, exit.debug_registers, entered, carrier_slot))
}

/// Pack generic VM-exit metadata without changing the C ABI layout.
///
/// For `kind=Exception`, `detail` is VM-exit interruption information: low
/// eight bits are the vector, bits 8..10 the interruption type, bit 11 the
/// error-code-valid flag, and bit 31 the valid flag. `qualification` is the
/// exception error code only when bit 11 is set. Its high 32 bits are the
/// fault linear address for a valid page fault. For memory violations it
/// remains the EPT qualification; every other exit preserves its prior fields.
fn pack_exit_metadata(
    raw_exit_reason: u64,
    exit_qualification: u64,
    interruption_info: u64,
    interruption_error_code: u64,
    page_fault_linear: u64,
) -> (u32, u64) {
    if raw_exit_reason & 0xffff == 0 {
        let valid = interruption_info & (1 << 31) != 0;
        let vector = interruption_info & 0xff;
        let error_valid = valid && interruption_info & (1 << 11) != 0;

        if valid && vector == 1 {
            // #DB qualification carries the architectural debug-condition bits.
            return (interruption_info as u32, exit_qualification & 0xffff_ffff);
        }

        let page_fault = valid && vector == 14;
        let error = if error_valid { interruption_error_code as u32 } else { 0 };
        let linear = if page_fault { page_fault_linear as u32 } else { 0 };
        (interruption_info as u32, u64::from(error) | (u64::from(linear) << 32))
    } else {
        (raw_exit_reason as u32, exit_qualification)
    }
}

#[cfg(test)]
mod tests {
    use super::pack_exit_metadata;

    #[test]
    fn page_fault_uses_exit_qualification_not_captured_cr2() {
        let info = (1u64 << 31) | (3 << 8) | (1 << 11) | 14;
        let (detail, qualification) = pack_exit_metadata(0, 1, info, 2, 0);
        assert_eq!(detail & 0xff, 14);
        assert_ne!(detail & (1 << 11), 0);
        assert_eq!(qualification as u32, 2);
        assert_eq!((qualification >> 32) as u32, 1);
    }

    #[test]
    fn exception_without_error_code_has_zero_qualification() {
        let (detail, qualification) = pack_exit_metadata(0, 0xdead, (1u64 << 31) | 6, 0xbeef, 1);
        assert_eq!(detail & 0xff, 6);
        assert_eq!(qualification, 0);
    }

    #[test]
    fn debug_exception_preserves_qualification_as_debug_status() {
        let debug_status = 0x0000_4000;
        let (detail, qualification) =
            pack_exit_metadata(0, debug_status.into(), (1u64 << 31) | (3 << 8) | 1, 0, 0);
        assert_eq!(detail & 0xff, 1);
        assert_eq!(qualification, u64::from(debug_status));
    }

    #[test]
    fn non_exception_exit_keeps_existing_metadata() {
        assert_eq!(pack_exit_metadata(48, 0x1234, 0xffff, 0xbeef, 1), (48, 0x1234));
    }

    #[test]
    fn invalid_interruption_information_stays_invalid() {
        let (detail, qualification) = pack_exit_metadata(0, 0xdead, 0, 5, 1);
        assert_eq!(detail & (1 << 31), 0);
        assert_eq!(qualification, 0);
    }
}
