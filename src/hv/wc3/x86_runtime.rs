//! WC3-private, generic 32-bit address-space bookkeeping.
//!
//! This module intentionally contains no PE, Windows, or Warcraft policy.
//! Handles are capabilities only while presented by the Blueprint VM that
//! created them; a numeric handle from another VM is rejected.

use alloc::vec::Vec;
use spin::Mutex;

use crate::hv::memory::PAGE_SIZE_4K;
use crate::phys::{self, HeapArena};
use v::bp_abi::{TrueosX86ExitV1, TrueosX86RegistersV1};

pub(super) const ERR_INVALID: i32 = -22;
pub(super) const ERR_DENIED: i32 = -13;
pub(super) const ERR_NOT_FOUND: i32 = -2;
pub(super) const ERR_BUSY: i32 = -16;
pub(super) const ERR_UNSUPPORTED: i32 = -95;
pub(super) const PERMISSION_READ: u32 = 1;
pub(super) const PERMISSION_WRITE: u32 = 2;
pub(super) const PERMISSION_EXECUTE: u32 = 4;
const PERMISSION_MASK: u32 = PERMISSION_READ | PERMISSION_WRITE | PERMISSION_EXECUTE;

struct Mapping {
    start: u32,
    len: u32,
    permissions: u32,
    arena: HeapArena,
}

struct AddressSpace {
    handle: u64,
    owner: u8,
    mappings: Vec<Mapping>,
}

struct Context {
    handle: u64,
    owner: u8,
    address_space: u64,
    registers: TrueosX86RegistersV1,
    parked: bool,
    cancelled: bool,
}

struct Runtime {
    next_handle: u64,
    spaces: Vec<AddressSpace>,
    contexts: Vec<Context>,
}

impl Runtime {
    const fn new() -> Self {
        Self {
            next_handle: 1,
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

static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime::new());

fn owner() -> Result<u8, i32> {
    crate::hv::current_vm_id().ok_or(ERR_DENIED)
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
    let mut runtime = RUNTIME.lock();
    let handle = runtime.fresh_handle();
    runtime.spaces.push(AddressSpace {
        handle,
        owner,
        mappings: Vec::new(),
    });
    *out = handle;
    Ok(())
}

pub(super) fn address_space_destroy(handle: u64) -> Result<(), i32> {
    let owner = owner()?;
    let mut runtime = RUNTIME.lock();
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
    let mut runtime = RUNTIME.lock();
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
    let arena = phys::reserve_heap_arena(len as usize, PAGE_SIZE_4K).ok_or(ERR_BUSY)?;
    unsafe {
        core::ptr::write_bytes(arena.virt_start as *mut u8, 0, arena.length);
    }
    space.mappings.push(Mapping {
        start,
        len,
        permissions,
        arena,
    });
    Ok(())
}

pub(super) fn address_space_unmap(handle: u64, start: u32, len: u32) -> Result<(), i32> {
    let owner = owner()?;
    checked_range(start, len)?;
    let mut runtime = RUNTIME.lock();
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
    Ok(())
}

pub(super) fn address_space_read(handle: u64, address: u32, out: &mut [u8]) -> Result<usize, i32> {
    let owner = owner()?;
    let runtime = RUNTIME.lock();
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
            (mapping.arena.virt_start as *const u8).add(offset),
            out.as_mut_ptr(),
            out.len(),
        );
    }
    Ok(out.len())
}

pub(super) fn address_space_write(handle: u64, address: u32, data: &[u8]) -> Result<usize, i32> {
    let owner = owner()?;
    let runtime = RUNTIME.lock();
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
            (mapping.arena.virt_start as *mut u8).add(offset),
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
    let mut runtime = RUNTIME.lock();
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
        parked: false,
        cancelled: false,
    });
    *out = handle;
    Ok(())
}

pub(super) fn context_destroy(handle: u64) -> Result<(), i32> {
    let owner = owner()?;
    let mut runtime = RUNTIME.lock();
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
    *out = context_mut(&mut RUNTIME.lock(), handle, owner)?.registers;
    Ok(())
}

pub(super) fn context_registers_set(
    handle: u64,
    registers: TrueosX86RegistersV1,
) -> Result<(), i32> {
    let owner = owner()?;
    context_mut(&mut RUNTIME.lock(), handle, owner)?.registers = registers;
    Ok(())
}

pub(super) fn context_park(handle: u64) -> Result<(), i32> {
    let owner = owner()?;
    context_mut(&mut RUNTIME.lock(), handle, owner)?.parked = true;
    Ok(())
}

pub(super) fn context_cancel(handle: u64) -> Result<(), i32> {
    let owner = owner()?;
    context_mut(&mut RUNTIME.lock(), handle, owner)?.cancelled = true;
    Ok(())
}

pub(super) fn context_execute(handle: u64, out: &mut TrueosX86ExitV1) -> Result<(), i32> {
    let owner = owner()?;
    let mut runtime = RUNTIME.lock();
    let context = context_mut(&mut runtime, handle, owner)?;
    *out = TrueosX86ExitV1 {
        kind: if context.cancelled { 5 } else { 255 },
        detail: 0,
        qualification: 0,
        registers: context.registers,
    };
    // The carrier hand-off is deliberately not emulated here.  Returning a
    // hard error prevents a package from mistaking host-memory bookkeeping for
    // authentic x86 execution until the VMX context switch is wired.
    Err(ERR_UNSUPPORTED)
}
