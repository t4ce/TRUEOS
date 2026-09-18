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

#[derive(Copy, Clone)]
pub(crate) struct GuestMapping {
    pub(crate) phys_start: u64,
    pub(crate) bytes: usize,
}
struct LauncherState {
    arena: HeapArena,
    imports: Vec<super::imports::LauncherImport>,
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
        return DispatchOutcome::Stop;
    };
    let call = CALLS[usize::from(vm_id)].fetch_add(1, Ordering::AcqRel) + 1;
    super::trace::info(format_args!("call #{} {}!{}", call, import.module, import.symbol));
    if call == 1
        && import.module.eq_ignore_ascii_case("KERNEL32.dll")
        && import.symbol == "GetVersion"
    {
        super::trace::info(format_args!(
            "gate-1a complete vm={} first-import=KERNEL32.dll!GetVersion",
            vm_id
        ));
    } else {
        super::trace::fail(format_args!("vm={} phase=unexpected-first-import", vm_id));
    }
    DispatchOutcome::Stop
}
