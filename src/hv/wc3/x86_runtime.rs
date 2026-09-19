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
    carrier_cr3: Option<u64>,
    carrier_pages: Vec<HeapArena>,
    admitted: bool,
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
        carrier_cr3: None,
        carrier_pages: Vec::new(),
        admitted: false,
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

pub(super) fn context_execute(
    handle: u64,
    out: &mut TrueosX86ExitV1,
    resume: bool,
) -> Result<(), i32> {
    let owner = owner()?;
    let (registers, address_space, cancelled, admitted) = {
        let runtime = RUNTIME.lock();
        let context = runtime
            .contexts
            .iter()
            .find(|context| context.handle == handle && context.owner == owner)
            .ok_or(ERR_NOT_FOUND)?;
        (context.registers, context.address_space, context.cancelled, context.admitted)
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

    let mut runtime = RUNTIME.lock();
    let context_index = runtime
        .contexts
        .iter()
        .position(|context| context.handle == handle && context.owner == owner)
        .ok_or(ERR_NOT_FOUND)?;
    let mappings = runtime
        .spaces
        .iter()
        .find(|space| space.handle == address_space && space.owner == owner)
        .ok_or(ERR_DENIED)?
        .mappings
        .iter()
        .map(|mapping| (mapping.start, mapping.len, mapping.permissions, mapping.arena))
        .collect::<Vec<_>>();
    let context = &mut runtime.contexts[context_index];
    if context.carrier_cr3.is_none() {
        let (cr3, pages) = build_carrier_page_tables(&mappings)?;
        context.carrier_cr3 = Some(cr3);
        context.carrier_pages = pages;
    }
    let cr3 = context.carrier_cr3.ok_or(ERR_NOT_FOUND)?;
    drop(runtime);

    let exit = unsafe { run_on_existing_vmx_carrier(cr3, registers)? };
    let mut runtime = RUNTIME.lock();
    let context = context_mut(&mut runtime, handle, owner)?;
    context.registers = exit.registers;
    context.admitted = true;
    *out = exit;
    Ok(())
}

fn build_carrier_page_tables(
    mappings: &[(u32, u32, u32, HeapArena)],
) -> Result<(u64, Vec<HeapArena>), i32> {
    let mut pages = Vec::new();
    let alloc_page = |pages: &mut Vec<HeapArena>| -> Result<HeapArena, i32> {
        let page = phys::reserve_heap_arena(PAGE_SIZE_4K, PAGE_SIZE_4K).ok_or(ERR_BUSY)?;
        unsafe { core::ptr::write_bytes(page.virt_start as *mut u8, 0, PAGE_SIZE_4K) };
        pages.push(page);
        Ok(page)
    };
    let pdpt = alloc_page(&mut pages)?;
    let mut pds = [pdpt; 4];
    for (index, pd) in pds.iter_mut().enumerate() {
        *pd = alloc_page(&mut pages)?;
        write_entry(pdpt, index, physical_address(*pd) | 0x7)?;
    }

    let mut ptes: Vec<(u32, HeapArena)> = Vec::new();
    for &(start, len, permissions, arena) in mappings {
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
                write_entry(*pd_page, pd_slot, physical_address(page) | 0x7)?;
                page
            };
            let page_offset = (va - start) as u64;
            let guest_physical = arena.phys_start.checked_add(page_offset).ok_or(ERR_INVALID)?;
            let mut flags = 1u64;
            if permissions & PERMISSION_WRITE != 0 { flags |= 2; }
            write_entry(pt, ((va as usize >> 12) & 0x1ff), guest_physical | flags)?;
            va = va.checked_add(PAGE_SIZE_4K as u32).ok_or(ERR_INVALID)?;
        }
    }
    // The carrier runs 32-bit protected mode with PAE enabled, so CR3 points
    // directly at the four-entry PDPTE page (there is no long-mode PML4).
    Ok((physical_address(pdpt), pages))
}

fn physical_address(page: HeapArena) -> u64 {
    page.phys_start
}

fn write_entry(page: HeapArena, index: usize, value: u64) -> Result<(), i32> {
    if index >= 512 { return Err(ERR_INVALID); }
    unsafe { (page.virt_start as *mut u64).add(index).write(value) };
    Ok(())
}

unsafe fn run_on_existing_vmx_carrier(
    cr3: u64,
    registers: TrueosX86RegistersV1,
) -> Result<TrueosX86ExitV1, i32> {
    let vm_id = crate::hv::current_vm_id().ok_or(ERR_DENIED)?;
    let segment_fields = [
        crate::hv::vmx::VMCS_GUEST_CS_SELECTOR, crate::hv::vmx::VMCS_GUEST_SS_SELECTOR,
        crate::hv::vmx::VMCS_GUEST_DS_SELECTOR, crate::hv::vmx::VMCS_GUEST_ES_SELECTOR,
        crate::hv::vmx::VMCS_GUEST_FS_SELECTOR, crate::hv::vmx::VMCS_GUEST_CS_BASE,
        crate::hv::vmx::VMCS_GUEST_SS_BASE, crate::hv::vmx::VMCS_GUEST_DS_BASE,
        crate::hv::vmx::VMCS_GUEST_ES_BASE, crate::hv::vmx::VMCS_GUEST_FS_BASE,
        crate::hv::vmx::VMCS_GUEST_CS_LIMIT, crate::hv::vmx::VMCS_GUEST_SS_LIMIT,
        crate::hv::vmx::VMCS_GUEST_DS_LIMIT, crate::hv::vmx::VMCS_GUEST_ES_LIMIT,
        crate::hv::vmx::VMCS_GUEST_FS_LIMIT, crate::hv::vmx::VMCS_GUEST_CS_AR,
        crate::hv::vmx::VMCS_GUEST_SS_AR, crate::hv::vmx::VMCS_GUEST_DS_AR,
        crate::hv::vmx::VMCS_GUEST_ES_AR, crate::hv::vmx::VMCS_GUEST_FS_AR,
    ];
    let outer_segments = segment_fields
        .iter()
        .map(|field| crate::hv::vmx::vmread(*field).ok_or(ERR_DENIED))
        .collect::<Result<Vec<_>, _>>()?;
    let outer = [
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_CR0).ok_or(ERR_DENIED)?,
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_CR3).ok_or(ERR_DENIED)?,
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_CR4).ok_or(ERR_DENIED)?,
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RIP).ok_or(ERR_DENIED)?,
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP).ok_or(ERR_DENIED)?,
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RFLAGS).ok_or(ERR_DENIED)?,
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_FS_BASE).ok_or(ERR_DENIED)?,
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_CTRL_ENTRY).ok_or(ERR_DENIED)?,
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_IA32_EFER).ok_or(ERR_DENIED)?,
    ];
    let outer_regs = crate::hv::vmx::guest_registers();
    let mut restore = || {
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_CR0, outer[0]);
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_CR3, outer[1]);
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_CR4, outer[2]);
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_RIP, outer[3]);
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_RSP, outer[4]);
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_RFLAGS, outer[5]);
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_FS_BASE, outer[6]);
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_CTRL_ENTRY, outer[7]);
        let _ = crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_IA32_EFER, outer[8]);
        for (field, value) in segment_fields.iter().zip(outer_segments.iter()) {
            let _ = crate::hv::vmx::vmwrite(*field, *value);
        }
        crate::hv::vmx::set_guest_registers(outer_regs);
    };
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_CR3, cr3).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_RIP, registers.eip as u64).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_RSP, registers.esp as u64).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_RFLAGS, (registers.eflags as u64 | 2) & !0x200).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_FS_BASE, registers.fs_base as u64).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_CTRL_ENTRY, outer[7] & !crate::hv::vmx::ENTRY_CTL_IA32E_MODE_GUEST).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_CS_SELECTOR, 0x08).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_SS_SELECTOR, 0x10).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_DS_SELECTOR, 0x10).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_ES_SELECTOR, 0x10).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_FS_SELECTOR, 0x18).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_CS_BASE, 0).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_SS_BASE, 0).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_DS_BASE, 0).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_ES_BASE, 0).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_FS_BASE, registers.fs_base as u64).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_CS_AR, 0xC09B).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_SS_AR, 0xC093).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_DS_AR, 0xC093).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_ES_AR, 0xC093).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_FS_AR, 0xC093).map_err(|_| ERR_DENIED)?;
    for field in [
        crate::hv::vmx::VMCS_GUEST_CS_LIMIT, crate::hv::vmx::VMCS_GUEST_SS_LIMIT,
        crate::hv::vmx::VMCS_GUEST_DS_LIMIT, crate::hv::vmx::VMCS_GUEST_ES_LIMIT,
        crate::hv::vmx::VMCS_GUEST_FS_LIMIT,
    ] {
        crate::hv::vmx::vmwrite(field, 0xffff_ffff).map_err(|_| ERR_DENIED)?;
    }
    crate::hv::vmx::vmwrite(crate::hv::vmx::VMCS_GUEST_IA32_EFER, 0).map_err(|_| ERR_DENIED)?;
    crate::hv::vmx::set_guest_registers(crate::hv::vmx::GuestRegisters { rax: registers.eax as u64, rbx: registers.ebx as u64, rcx: registers.ecx as u64, rdx: registers.edx as u64, rsi: registers.esi as u64, rdi: registers.edi as u64, rbp: registers.ebp as u64, ..Default::default() });
    let mut lr = crate::hv::vmx::LaunchResult::default();
    crate::hv::vmx::vmresume_once_wrapper(vm_id, &mut lr);
    let regs = crate::hv::vmx::guest_registers();
    let rsp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP).unwrap_or(registers.esp as u64);
    let exit = TrueosX86ExitV1 { kind: match lr.exit_reason & 0xffff { crate::hv::vmx::VMEXIT_REASON_VMCALL => 1, 0 => 2, 48 => 3, 0x0c => 4, _ => 255 }, detail: lr.exit_reason as u32, qualification: lr.exit_qualification, registers: TrueosX86RegistersV1 { eax: regs.rax as u32, ebx: regs.rbx as u32, ecx: regs.rcx as u32, edx: regs.rdx as u32, esi: regs.rsi as u32, edi: regs.rdi as u32, ebp: regs.rbp as u32, esp: rsp as u32, eip: lr.guest_rip as u32, eflags: crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RFLAGS).unwrap_or(registers.eflags as u64) as u32, fs_base: crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_FS_BASE).unwrap_or(registers.fs_base as u64) as u32 } };
    restore();
    Ok(exit)
}
