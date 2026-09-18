use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use sha2::{Digest, Sha256};
use spin::Mutex;

use crate::{
    hv::{memory::PAGE_SIZE_4K, vmcall::DispatchOutcome},
    phys::{self, HeapArena},
};

use super::{imports, pe32, thunk32};

pub(crate) const ENTRY_VA: u32 = pe32::IMAGE_BASE + pe32::ENTRY_RVA;
pub(crate) const TEB_VA: u32 = 0x0020_1000;
const EXPECTED_SHA256: [u8; 32] = [
    0x5a, 0x8c, 0xca, 0x72, 0x7c, 0x71, 0x9a, 0xe0, 0x54, 0xad, 0xf8, 0xd1, 0x55, 0x23, 0xa8, 0xe3,
    0x09, 0x97, 0x45, 0x22, 0x5e, 0x2f, 0x4f, 0x98, 0x85, 0xf8, 0x87, 0x74, 0xaa, 0x6f, 0x36, 0xd9,
];
const WINDOWS_XP_GET_VERSION: u32 = 0x0A28_0105;
const HEAP_CREATE_RETURN: u32 = 0x0040_2D9B;
const HEAP_CREATE_INITIAL: u32 = 0x1000;
const GET_VERSION_EX_A_RETURN: u32 = 0x0040_2C61;
const OS_VERSION_INFO_A_BYTES: usize = 0x94;

#[derive(Copy, Clone)]
pub(crate) struct GuestMapping {
    pub(crate) phys_start: u64,
    pub(crate) bytes: usize,
}
struct LauncherState {
    arena: HeapArena,
    imports: Vec<super::imports::LauncherImport>,
    heap: Option<HeapHandle>,
}

#[derive(Copy, Clone)]
struct HeapHandle {
    value: u32,
    flags: u32,
    initial: u32,
    maximum: u32,
}
static LAUNCHERS: [Mutex<Option<LauncherState>>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(None) }; crate::allcaps::hv::VM_ID_LIMIT];
static CALLS: [AtomicU32; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicU32::new(0) }; crate::allcaps::hv::VM_ID_LIMIT];

pub(crate) fn prepare(vm_id: u8, bytes: &[u8]) -> Result<(), &'static str> {
    if Sha256::digest(bytes).as_slice() != EXPECTED_SHA256 {
        return Err("wc3 launcher artifact hash");
    }
    let mut materialized = pe32::materialize(bytes)?;
    let thunk_bytes = materialized
        .imports
        .len()
        .checked_mul(thunk32::THUNK_BYTES)
        .ok_or("wc3 thunk size")?;
    if thunk_bytes > PAGE_SIZE_4K {
        return Err("wc3 too many imports for thunk page");
    }
    let arena = phys::reserve_heap_arena(pe32::IMAGE_BYTES + PAGE_SIZE_4K * 2, PAGE_SIZE_4K)
        .ok_or("wc3 launcher backing allocation")?;
    let mut thunks = alloc::vec![0; PAGE_SIZE_4K];
    imports::patch(&mut materialized.image, &materialized.imports, &mut thunks)?;
    unsafe {
        core::ptr::write_bytes(arena.virt_start as *mut u8, 0, arena.length);
        core::ptr::copy_nonoverlapping(
            materialized.image.as_ptr(),
            arena.virt_start as *mut u8,
            pe32::IMAGE_BYTES,
        );
        core::ptr::copy_nonoverlapping(
            thunks.as_ptr(),
            (arena.virt_start + pe32::IMAGE_BYTES) as *mut u8,
            PAGE_SIZE_4K,
        );
        core::ptr::write_unaligned(
            (arena.virt_start + pe32::IMAGE_BYTES + PAGE_SIZE_4K) as *mut u32,
            u32::MAX,
        );
    }
    let slot = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?;
    *slot.lock() = Some(LauncherState {
        arena,
        imports: materialized.imports,
        heap: None,
    });
    CALLS[usize::from(vm_id)].store(0, Ordering::Release);
    super::trace::info(format_args!(
        "gate-1a artifact vm={} sha256=5a8cca727c719ae054adf8d15523a8e3099745225e2f4f9885f88774aa6f36d9 bytes={} accepted=1",
        vm_id,
        bytes.len()
    ));
    super::trace::info(format_args!(
        "gate-1a pe vm={} base=0x00400000 image=0x00044000 entry=0x00402144 imports={}",
        vm_id,
        LAUNCHERS[usize::from(vm_id)]
            .lock()
            .as_ref()
            .map(|state| state.imports.len())
            .unwrap_or(0)
    ));
    Ok(())
}

pub(crate) fn guest_mapping(vm_id: u8) -> Option<GuestMapping> {
    let state = LAUNCHERS.get(usize::from(vm_id))?.lock();
    state.as_ref().map(|state| GuestMapping {
        phys_start: state.arena.phys_start,
        bytes: state.arena.length,
    })
}

pub(crate) fn release(vm_id: u8) {
    let Some(slot) = LAUNCHERS.get(usize::from(vm_id)) else {
        return;
    };
    let state = slot.lock().take();
    if let Some(state) = state {
        let _ = phys::free_phys_range(state.arena.phys_start, state.arena.length);
    }
    CALLS[usize::from(vm_id)].store(0, Ordering::Release);
}

fn stack_words(vm_id: u8) -> Result<[u32; 4], &'static str> {
    let esp =
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP).ok_or("guest ESP unavailable")?;
    let offset = esp
        .checked_sub(crate::hv::memory::GUEST_STACK_VA_BASE)
        .ok_or("guest ESP below stack")?;
    let offset = usize::try_from(offset).map_err(|_| "guest ESP range")?;
    let bytes =
        crate::hv::memory::guest_stack_slice_for_vm(vm_id).ok_or("guest stack unavailable")?;
    let frame = bytes
        .get(offset..offset.checked_add(16).ok_or("guest stack frame overflow")?)
        .ok_or("guest stack frame unavailable")?;
    Ok([
        u32::from_le_bytes(frame[0..4].try_into().map_err(|_| "guest return address")?),
        u32::from_le_bytes(frame[4..8].try_into().map_err(|_| "guest heap flags")?),
        u32::from_le_bytes(frame[8..12].try_into().map_err(|_| "guest heap initial")?),
        u32::from_le_bytes(frame[12..16].try_into().map_err(|_| "guest heap maximum")?),
    ])
}

fn heap_create(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let [return_address, flags, initial, maximum] = stack_words(vm_id)?;
    if return_address != HEAP_CREATE_RETURN
        || flags != 0
        || initial != HEAP_CREATE_INITIAL
        || maximum != 0
    {
        return Err("unexpected HeapCreate frame");
    }
    let state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?;
    let mut state = state.lock();
    let state = state.as_mut().ok_or("wc3 launcher state unavailable")?;
    let heap = state.heap.get_or_insert(HeapHandle {
        value: 0x5743_0001u32
            .checked_add(u32::from(vm_id))
            .ok_or("heap handle overflow")?,
        flags,
        initial,
        maximum,
    });
    if heap.value == 0 || heap.flags != flags || heap.initial != initial || heap.maximum != maximum
    {
        return Err("wc3 heap handle state");
    }
    Ok((heap.value, return_address))
}

fn guest_stack_range_mut(vm_id: u8, guest_address: u32, bytes: usize) -> Result<&'static mut [u8], &'static str> {
    let offset = u64::from(guest_address)
        .checked_sub(crate::hv::memory::GUEST_STACK_VA_BASE)
        .ok_or("guest address below stack")?;
    let offset = usize::try_from(offset).map_err(|_| "guest address range")?;
    let stack = crate::hv::memory::guest_stack_mut_ptr_for_vm(vm_id).ok_or("guest stack unavailable")?;
    let stack_bytes = crate::hv::memory::active_guest_stack_bytes_for_vm(vm_id);
    let end = offset.checked_add(bytes).ok_or("guest structure overflow")?;
    if end > stack_bytes {
        return Err("guest structure outside writable stack");
    }
    Ok(unsafe { core::slice::from_raw_parts_mut(stack.add(offset), bytes) })
}

fn get_version_ex_a(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP).ok_or("guest ESP unavailable")?;
    let frame = guest_stack_range_mut(vm_id, esp as u32, 8)?;
    let return_address = u32::from_le_bytes(frame[0..4].try_into().map_err(|_| "GetVersionExA return")?);
    let version_info = u32::from_le_bytes(frame[4..8].try_into().map_err(|_| "GetVersionExA argument")?);
    if return_address != GET_VERSION_EX_A_RETURN {
        return Err("unexpected GetVersionExA return address");
    }
    let output = guest_stack_range_mut(vm_id, version_info, OS_VERSION_INFO_A_BYTES)?;
    if u32::from_le_bytes(output[0..4].try_into().map_err(|_| "GetVersionExA size")?) != OS_VERSION_INFO_A_BYTES as u32 {
        return Err("unexpected GetVersionExA structure size");
    }
    output.fill(0);
    output[0..4].copy_from_slice(&(OS_VERSION_INFO_A_BYTES as u32).to_le_bytes());
    output[4..8].copy_from_slice(&5u32.to_le_bytes());
    output[8..12].copy_from_slice(&1u32.to_le_bytes());
    output[12..16].copy_from_slice(&2600u32.to_le_bytes());
    output[16..20].copy_from_slice(&2u32.to_le_bytes());
    Ok((version_info, return_address))
}

pub(crate) fn handle_vmcall(vm_id: u8) -> DispatchOutcome {
    let id = crate::hv::vmx::guest_registers().rax as u32;
    let imports = match LAUNCHERS
        .get(usize::from(vm_id))
        .and_then(|slot| slot.lock().as_ref().map(|state| state.imports.clone()))
    {
        Some(imports) => imports,
        None => return DispatchOutcome::Stop,
    };
    let Some(import) = imports.get(id as usize) else {
        super::trace::fail(format_args!(
            "gate-1b failed vm={} phase=invalid-import-id id={}",
            vm_id, id
        ));
        return DispatchOutcome::Stop;
    };
    let call = CALLS[usize::from(vm_id)].fetch_add(1, Ordering::AcqRel) + 1;
    super::trace::info(format_args!("call #{} {}!{}", call, import.module, import.symbol));
    if call == 1 && imports::is_get_version(import) {
        let mut registers = crate::hv::vmx::guest_registers();
        registers.rax = u64::from(WINDOWS_XP_GET_VERSION);
        crate::hv::vmx::set_guest_registers(registers);
        super::trace::info(format_args!("return #1 KERNEL32.dll!GetVersion eax=0x0A280105"));
        DispatchOutcome::Resume
    } else if call == 2 && imports::is_heap_create(import) {
        match heap_create(vm_id) {
            Ok((handle, return_address)) => {
                super::trace::info(format_args!(
                    "HeapCreate args flags=0x00000000 initial=0x00001000 maximum=0x00000000"
                ));
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(handle);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #2 KERNEL32.dll!HeapCreate handle=0x{:08X} ret=0x{:08X}",
                    handle, return_address
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1c failed vm={} phase=HeapCreate reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 3 && imports::is_get_version_ex_a(import) {
        match get_version_ex_a(vm_id) {
            Ok((version_info, return_address)) => {
                super::trace::info(format_args!(
                    "GetVersionExA arg=0x{:08X} size=0x00000094",
                    version_info
                ));
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = 1;
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #3 KERNEL32.dll!GetVersionExA eax=1 major=5 minor=1 build=2600 platform=2 ret=0x{:08X}",
                    return_address
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1d failed vm={} phase=GetVersionExA reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 4 {
        super::trace::info(format_args!(
            "gate-1d complete vm={} next-import={}!{}",
            vm_id, import.module, import.symbol
        ));
        DispatchOutcome::Stop
    } else {
        super::trace::fail(format_args!(
            "gate-1d failed vm={} phase=unexpected-{}-import {}!{}",
            vm_id, call, import.module, import.symbol
        ));
        DispatchOutcome::Stop
    }
}
