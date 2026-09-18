use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use sha2::{Digest, Sha256};
use spin::Mutex;
use trueos_executor::Spawner;
use trueos_time::{Duration, Timer};

use crate::{
    hv::{memory::PAGE_SIZE_4K, vmcall::DispatchOutcome},
    phys::{self, HeapArena},
};

use super::{imports, pe32, thunk32};

pub(crate) const ENTRY_VA: u32 = pe32::IMAGE_BASE + pe32::ENTRY_RVA;
pub(crate) const TEB_VA: u32 = 0x0020_1000;
pub(crate) const HEAP_VA: u32 = 0x0021_0000;
pub(crate) const PROCESS_DATA_VA: u32 = 0x0021_1000;
pub(crate) const HEAP_BYTES: usize = PAGE_SIZE_4K;
const HEAP_BACKING_OFFSET: usize = pe32::IMAGE_BYTES + PAGE_SIZE_4K * 2;
const PROCESS_DATA_BACKING_OFFSET: usize = pe32::IMAGE_BYTES + PAGE_SIZE_4K * 3;
const EXPECTED_SHA256: [u8; 32] = [
    0x5a, 0x8c, 0xca, 0x72, 0x7c, 0x71, 0x9a, 0xe0, 0x54, 0xad, 0xf8, 0xd1, 0x55, 0x23, 0xa8, 0xe3,
    0x09, 0x97, 0x45, 0x22, 0x5e, 0x2f, 0x4f, 0x98, 0x85, 0xf8, 0x87, 0x74, 0xaa, 0x6f, 0x36, 0xd9,
];
const WINDOWS_XP_GET_VERSION: u32 = 0x0A28_0105;
const HEAP_CREATE_RETURN: u32 = 0x0040_2D9B;
const HEAP_CREATE_INITIAL: u32 = 0x1000;
const GET_VERSION_EX_A_RETURN: u32 = 0x0040_2C61;
const OS_VERSION_INFO_A_BYTES: usize = 0x94;
const CRITICAL_SECTION_BYTES: usize = 0x18;
const CRITICAL_SECTION_CALLS: [(u32, u32); 4] = [
    (0x0040_22A4, 0x0040_AB08),
    (0x0040_22AC, 0x0040_AB38),
    (0x0040_22B4, 0x0040_AB20),
    (0x0040_22BC, 0x0040_AAF0),
];
const TLS_ALLOC_RETURN: u32 = 0x0040_3ED4;
const TLS_SLOT_COUNT: usize = 64;
const HEAP_ALLOC_RETURN: u32 = 0x0040_4132;
const HEAP_ALLOC_FLAGS: u32 = 0x0000_0008;
const HEAP_ALLOC_BYTES: u32 = 0x0000_0080;
const HEAP_ALLOC_SECOND_RETURN: u32 = 0x0040_1FEC;
const HEAP_ALLOC_SECOND_FLAGS: u32 = 0;
const HEAP_ALLOC_SECOND_BYTES: u32 = 0x0000_0480;
const HEAP_ALLOC_THIRD_RETURN: u32 = 0x0040_1FEC;
const HEAP_ALLOC_THIRD_FLAGS: u32 = 0;
const HEAP_ALLOC_THIRD_BYTES: u32 = 0x0000_0010;
const HEAP_ALLOC_LOCK_RETURN: u32 = 0x0040_1FEC;
const HEAP_ALLOC_LOCK_FLAGS: u32 = 0;
const HEAP_ALLOC_LOCK_BYTES: u32 = 0x0000_0020;
const TLS_SET_VALUE_RETURN: u32 = 0x0040_3EFC;
const TLS_SET_VALUE_INDEX: u32 = 0;
const TLS_SET_VALUE_VALUE: u32 = HEAP_VA;
const GET_CURRENT_THREAD_ID_RETURN: u32 = 0x0040_3F0D;
const GET_CURRENT_THREAD_ID_ESI: u32 = HEAP_VA;
const GET_STARTUP_INFO_A_RETURN: u32 = 0x0040_48FF;
const GET_STARTUP_INFO_A_SECOND_RETURN: u32 = 0x0040_21FB;
const STARTUP_INFO_A_BYTES: usize = 0x44;
const GET_MODULE_FILE_NAME_A_RETURN: u32 = 0x0040_4545;
const GET_MODULE_FILE_NAME_A_BUFFER: u32 = 0x0040_ABA8;
const GET_MODULE_FILE_NAME_A_SIZE: u32 = 0x104;
const GET_MODULE_HANDLE_A_RETURN: u32 = 0x0040_221E;
const MODULE_IMAGE_BASE: u32 = pe32::IMAGE_BASE;
const MODULE_FILENAME_A: &[u8] = b"C:\\Warcraft III\\Warcraft III.exe\0";
const GET_STD_HANDLE_RETURN: u32 = 0x0040_4A0D;
const GET_FILE_TYPE_RETURN: u32 = 0x0040_4A1B;
const SET_HANDLE_COUNT_RETURN: u32 = 0x0040_4A52;
const STD_HANDLES: [u32; 3] = [0x5743_1001, 0x5743_1002, 0x5743_1003];
const GET_COMMAND_LINE_A_RETURN: u32 = 0x0040_21D0;
const GET_ENVIRONMENT_STRINGS_W_RETURN: u32 = 0x0040_4786;
const GET_ENVIRONMENT_STRINGS_A_RETURN: u32 = 0x0040_479E;
const FREE_ENVIRONMENT_STRINGS_A_RETURN: u32 = 0x0040_488E;
const ENVIRONMENT_BLOCK_VA: u32 = PROCESS_DATA_VA + 0x100;
const COMMAND_LINE: &[u8] = b"\"Warcraft III.exe\"\0";
const LAUNCHER_PATH: &str = "apps/common/Warcraft III/Warcraft III.exe";
const ENTER_CRITICAL_SECTION_RETURN: u32 = 0x0040_231C;
const ENTER_CRITICAL_SECTION_POINTER: u32 = 0x0021_0510;
const LEAVE_CRITICAL_SECTION_RETURN: u32 = 0x0040_2332;
const STATIC_LOCK_17: u32 = 0x0040_AB08;
const DYNAMIC_LOCK_25: u32 = 0x0021_0510;
const GET_ACP_RETURN: u32 = 0x0040_234C;
const GET_ACP_CODE_PAGE: u32 = 1252;
const GET_CP_INFO_FIRST_RETURN: u32 = 0x0040_238B;
const GET_CP_INFO_SECOND_RETURN: u32 = 0x0040_25A1;
const CP_INFO_BYTES: usize = 0x14;
const GET_STRING_TYPE_W_RETURN: u32 = 0x0040_4C28;
const GET_STRING_TYPE_W_SOURCE: u32 = 0x0040_71E0;
const GET_STRING_TYPE_W_SECOND_RETURN: u32 = 0x0040_4D16;
const MULTI_BYTE_TO_WIDE_CHAR_SIZING_RETURN: u32 = 0x0040_4CAE;
const MULTI_BYTE_TO_WIDE_CHAR_CONVERT_RETURN: u32 = 0x0040_4D04;
const MULTI_BYTE_TO_WIDE_CHAR_ANSI_WRAPPER_SIZING_RETURN: u32 = 0x0040_2AA5;
const MULTI_BYTE_TO_WIDE_CHAR_ANSI_WRAPPER_CONVERT_RETURN: u32 = 0x0040_2AFD;
const WIDE_CHAR_TO_MULTI_BYTE_RETURN: u32 = 0x0040_2BD3;
const WIDE_CHAR_TO_MULTI_BYTE_FLAGS: u32 = 0x0000_0220;
const MULTI_BYTE_TO_WIDE_CHAR_CODE_PAGE: u32 = GET_ACP_CODE_PAGE;
const LC_MAP_STRING_W_LOWERCASE: u32 = 0x0000_0100;
const LC_MAP_STRING_W_UPPERCASE: u32 = 0x0000_0200;
const LC_MAP_STRING_W_PROBE_RETURN: u32 = 0x0040_2A08;
const LC_MAP_STRING_W_ANSI_SIZING_RETURN: u32 = 0x0040_2B13;
const LC_MAP_STRING_W_ANSI_OUTPUT_RETURN: u32 = 0x0040_2B46;
const LC_MAP_STRING_W_WIDE_OUTPUT_RETURN: u32 = 0x0040_2BAE;

#[derive(Copy, Clone)]
enum MultiByteToWideCharSite {
    LocaleSizing,
    LocaleConversion,
    AnsiMapSizing,
    AnsiMapConversion,
}

fn multi_byte_to_wide_char_site(return_address: u32) -> Option<MultiByteToWideCharSite> {
    Some(match return_address {
        MULTI_BYTE_TO_WIDE_CHAR_SIZING_RETURN => MultiByteToWideCharSite::LocaleSizing,
        MULTI_BYTE_TO_WIDE_CHAR_CONVERT_RETURN => MultiByteToWideCharSite::LocaleConversion,
        MULTI_BYTE_TO_WIDE_CHAR_ANSI_WRAPPER_SIZING_RETURN => {
            MultiByteToWideCharSite::AnsiMapSizing
        }
        MULTI_BYTE_TO_WIDE_CHAR_ANSI_WRAPPER_CONVERT_RETURN => {
            MultiByteToWideCharSite::AnsiMapConversion
        }
        _ => return None,
    })
}

#[derive(Copy, Clone)]
pub(crate) struct GuestMapping {
    pub(crate) phys_start: u64,
    pub(crate) bytes: usize,
}
struct LauncherState {
    arena: HeapArena,
    imports: Vec<super::imports::LauncherImport>,
    heap: Option<HeapHandle>,
    initialized_critical_sections: [u32; 4],
    initialized_critical_section_count: usize,
    dynamic_critical_sections: Vec<u32>,
    tls_allocated: [bool; TLS_SLOT_COUNT],
    tls_values: [u32; TLS_SLOT_COUNT],
    tls_value_set: [bool; TLS_SLOT_COUNT],
    primary_thread_id: u32,
    std_handles: [u32; 3],
    heap_next: usize,
    heap_allocations: Vec<HeapAllocation>,
}

struct HeapAllocation {
    guest_ptr: u32,
    bytes: u32,
    flags: u32,
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
static AUTOSTART_SCHEDULED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

pub(crate) fn schedule_autostart(spawner: &Spawner) {
    if AUTOSTART_SCHEDULED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        if let Ok(token) = delayed_autostart(*spawner) {
            spawner.spawn(token);
        }
    }
}

#[trueos_executor::task]
async fn delayed_autostart(spawner: Spawner) {
    Timer::after(Duration::from_secs(5)).await;
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!("wc3 autostart: no TRUEOSFS root\n");
        return;
    };
    let Some(bytes) = crate::r::fs::trueosfs::file_out_async(disk, LAUNCHER_PATH)
        .await
        .ok()
        .flatten()
    else {
        crate::log!("wc3 autostart: artifact not found\n");
        return;
    };
    match crate::hv::start_wc3_launcher(0, &spawner, &bytes) {
        Ok(()) => crate::log!("wc3 autostart: queued Gate-1A on vm0\n"),
        Err(error) => crate::log!("wc3 autostart: start failed vm0 error={error:?}\n"),
    }
}

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
    let arena = phys::reserve_heap_arena(pe32::IMAGE_BYTES + PAGE_SIZE_4K * 4, PAGE_SIZE_4K)
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
        core::ptr::copy_nonoverlapping(
            COMMAND_LINE.as_ptr(),
            (arena.virt_start + PROCESS_DATA_BACKING_OFFSET) as *mut u8,
            COMMAND_LINE.len(),
        );
        core::ptr::write_bytes(
            (arena.virt_start + PROCESS_DATA_BACKING_OFFSET + 0x100) as *mut u8,
            0,
            2,
        );
    }
    let slot = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?;
    *slot.lock() = Some(LauncherState {
        arena,
        imports: materialized.imports,
        heap: None,
        initialized_critical_sections: [0; 4],
        initialized_critical_section_count: 0,
        dynamic_critical_sections: Vec::new(),
        tls_allocated: [false; TLS_SLOT_COUNT],
        tls_values: [0; TLS_SLOT_COUNT],
        tls_value_set: [false; TLS_SLOT_COUNT],
        primary_thread_id: 1,
        std_handles: STD_HANDLES,
        heap_next: 0,
        heap_allocations: Vec::new(),
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

fn guest_stack_range_mut(
    vm_id: u8,
    guest_address: u32,
    bytes: usize,
) -> Result<&'static mut [u8], &'static str> {
    let offset = u64::from(guest_address)
        .checked_sub(crate::hv::memory::GUEST_STACK_VA_BASE)
        .ok_or("guest address below stack")?;
    let offset = usize::try_from(offset).map_err(|_| "guest address range")?;
    let stack =
        crate::hv::memory::guest_stack_mut_ptr_for_vm(vm_id).ok_or("guest stack unavailable")?;
    let stack_bytes = crate::hv::memory::active_guest_stack_bytes_for_vm(vm_id);
    let end = offset
        .checked_add(bytes)
        .ok_or("guest structure overflow")?;
    if end > stack_bytes {
        return Err("guest structure outside writable stack");
    }
    Ok(unsafe { core::slice::from_raw_parts_mut(stack.add(offset), bytes) })
}

fn get_version_ex_a(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp =
        crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP).ok_or("guest ESP unavailable")?;
    let frame = guest_stack_range_mut(vm_id, esp as u32, 8)?;
    let return_address =
        u32::from_le_bytes(frame[0..4].try_into().map_err(|_| "GetVersionExA return")?);
    let version_info = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "GetVersionExA argument")?,
    );
    if return_address != GET_VERSION_EX_A_RETURN {
        return Err("unexpected GetVersionExA return address");
    }
    let output = guest_stack_range_mut(vm_id, version_info, OS_VERSION_INFO_A_BYTES)?;
    if u32::from_le_bytes(output[0..4].try_into().map_err(|_| "GetVersionExA size")?)
        != OS_VERSION_INFO_A_BYTES as u32
    {
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

fn launcher_image_range_mut(
    vm_id: u8,
    guest_address: u32,
    bytes: usize,
) -> Result<&'static mut [u8], &'static str> {
    let state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock();
    let state = state.as_ref().ok_or("wc3 launcher state unavailable")?;
    let offset = u64::from(guest_address)
        .checked_sub(u64::from(pe32::IMAGE_BASE))
        .ok_or("guest image address below image")?;
    let offset = usize::try_from(offset).map_err(|_| "guest image address range")?;
    let end = offset
        .checked_add(bytes)
        .ok_or("guest image range overflow")?;
    if end > pe32::IMAGE_BYTES || end > state.arena.length {
        return Err("guest image range outside writable image");
    }
    Ok(unsafe {
        core::slice::from_raw_parts_mut((state.arena.virt_start + offset) as *mut u8, bytes)
    })
}

fn initialize_critical_section(vm_id: u8, call: u32) -> Result<(u32, u32), &'static str> {
    let expected = CRITICAL_SECTION_CALLS
        .get(usize::try_from(call - 4).map_err(|_| "critical-section sequence")?)
        .copied()
        .ok_or("unexpected critical-section sequence")?;
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "critical-section return address")?,
    );
    let critical_section = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "critical-section argument")?,
    );
    if (return_address, critical_section) != expected || critical_section == 0 {
        return Err("unexpected critical-section frame");
    }
    let output = launcher_image_range_mut(vm_id, critical_section, CRITICAL_SECTION_BYTES)?;
    output.fill(0);
    output[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    let state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?;
    let mut state = state.lock();
    let state = state.as_mut().ok_or("wc3 launcher state unavailable")?;
    let slot = usize::try_from(call - 4).map_err(|_| "critical-section sequence")?;
    state.initialized_critical_sections[slot] = critical_section;
    state.initialized_critical_section_count = slot + 1;
    Ok((critical_section, return_address))
}

fn dynamic_critical_section_range_mut(
    vm_id: u8,
    guest_address: u32,
) -> Result<&'static mut [u8], &'static str> {
    {
        let guard = LAUNCHERS
            .get(usize::from(vm_id))
            .ok_or("unsupported wc3 launcher VM id")?
            .lock();
        let state = guard.as_ref().ok_or("wc3 launcher state unavailable")?;
        if !state.dynamic_critical_sections.contains(&guest_address) {
            return Err("critical section is not registered");
        }
    }
    launcher_heap_range_mut(vm_id, guest_address, CRITICAL_SECTION_BYTES)
}

fn registered_critical_section_range_mut(
    vm_id: u8,
    guest_address: u32,
) -> Result<&'static mut [u8], &'static str> {
    let known = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock();
    let state = known.as_ref().ok_or("wc3 launcher state unavailable")?;
    let static_known = state.initialized_critical_sections.contains(&guest_address);
    let dynamic_known = state.dynamic_critical_sections.contains(&guest_address);
    drop(known);
    if static_known {
        launcher_image_range_mut(vm_id, guest_address, CRITICAL_SECTION_BYTES)
    } else if dynamic_known {
        dynamic_critical_section_range_mut(vm_id, guest_address)
    } else {
        Err("critical section is not registered")
    }
}

fn initialize_dynamic_critical_section(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "InitializeCriticalSection return")?,
    );
    let critical_section = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "InitializeCriticalSection argument")?,
    );
    if return_address != 0x0040_2301 || critical_section != DYNAMIC_LOCK_25 {
        return Err("unexpected dynamic InitializeCriticalSection frame");
    }
    let allocation = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock();
    let state = allocation
        .as_ref()
        .ok_or("wc3 launcher state unavailable")?;
    if !state
        .heap_allocations
        .iter()
        .any(|allocation| allocation.guest_ptr == critical_section && allocation.bytes >= 0x18)
    {
        return Err("dynamic critical section is not a recorded allocation");
    }
    drop(allocation);
    let output = launcher_heap_range_mut(vm_id, critical_section, CRITICAL_SECTION_BYTES)?;
    output.fill(0);
    output[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock();
    let state = state.as_mut().ok_or("wc3 launcher state unavailable")?;
    state.dynamic_critical_sections.push(critical_section);
    Ok((critical_section, return_address))
}

fn critical_section_frame(vm_id: u8, expected_return: u32) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "critical-section return")?,
    );
    let critical_section = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "critical-section argument")?,
    );
    if return_address != expected_return || critical_section == 0 {
        return Err("unexpected critical-section frame");
    }
    Ok((critical_section, return_address))
}

fn enter_critical_section(
    vm_id: u8,
    expected_pointer: u32,
) -> Result<(u32, u32, u32), &'static str> {
    let (critical_section, return_address) =
        critical_section_frame(vm_id, ENTER_CRITICAL_SECTION_RETURN)?;
    if critical_section != expected_pointer {
        return Err("unexpected EnterCriticalSection pointer");
    }
    let tid = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock()
        .as_ref()
        .ok_or("wc3 launcher state unavailable")?
        .primary_thread_id;
    let output = registered_critical_section_range_mut(vm_id, critical_section)?;
    let lock_count = i32::from_le_bytes(output[4..8].try_into().map_err(|_| "LockCount")?);
    let recursion = u32::from_le_bytes(output[8..12].try_into().map_err(|_| "RecursionCount")?);
    let owner = u32::from_le_bytes(output[12..16].try_into().map_err(|_| "OwningThread")?);
    if owner == 0 && recursion == 0 && lock_count == -1 {
        output[4..8].copy_from_slice(&0i32.to_le_bytes());
        output[8..12].copy_from_slice(&1u32.to_le_bytes());
        output[12..16].copy_from_slice(&tid.to_le_bytes());
    } else if owner == tid && recursion > 0 {
        output[4..8].copy_from_slice(
            &lock_count
                .checked_add(1)
                .ok_or("LockCount overflow")?
                .to_le_bytes(),
        );
        output[8..12].copy_from_slice(
            &recursion
                .checked_add(1)
                .ok_or("RecursionCount overflow")?
                .to_le_bytes(),
        );
    } else {
        return Err("critical-section contention");
    }
    Ok((critical_section, tid, return_address))
}

fn leave_critical_section(vm_id: u8) -> Result<(u32, u32, u32), &'static str> {
    let (critical_section, return_address) =
        critical_section_frame(vm_id, LEAVE_CRITICAL_SECTION_RETURN)?;
    let tid = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock()
        .as_ref()
        .ok_or("wc3 launcher state unavailable")?
        .primary_thread_id;
    let output = registered_critical_section_range_mut(vm_id, critical_section)?;
    let lock_count = i32::from_le_bytes(output[4..8].try_into().map_err(|_| "LockCount")?);
    let recursion = u32::from_le_bytes(output[8..12].try_into().map_err(|_| "RecursionCount")?);
    let owner = u32::from_le_bytes(output[12..16].try_into().map_err(|_| "OwningThread")?);
    super::trace::info(format_args!(
        "LeaveCriticalSection frame ret=0x{:08X} ptr=0x{:08X} owner={} recursion={} lock_count={}",
        return_address, critical_section, owner, recursion, lock_count
    ));
    if owner != tid || recursion == 0 || lock_count < 0 {
        return Err("invalid LeaveCriticalSection ownership");
    }
    let expected_lock_count = recursion
        .checked_sub(1)
        .and_then(|count| i32::try_from(count).ok())
        .ok_or("invalid LeaveCriticalSection recursion")?;
    if lock_count != expected_lock_count {
        return Err("invalid LeaveCriticalSection lock count");
    }
    output[4..8].copy_from_slice(
        &lock_count
            .checked_sub(1)
            .ok_or("LockCount underflow")?
            .to_le_bytes(),
    );
    let recursion = recursion - 1;
    output[8..12].copy_from_slice(&recursion.to_le_bytes());
    if recursion == 0 {
        output[12..16].copy_from_slice(&0u32.to_le_bytes());
    }
    Ok((critical_section, tid, return_address))
}

fn tls_alloc(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 4)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "TlsAlloc return address")?,
    );
    if return_address != TLS_ALLOC_RETURN {
        return Err("unexpected TlsAlloc return address");
    }
    let state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?;
    let mut state = state.lock();
    let state = state.as_mut().ok_or("wc3 launcher state unavailable")?;
    let index = state
        .tls_allocated
        .iter()
        .position(|allocated| !allocated)
        .ok_or("TlsAlloc exhausted")?;
    state.tls_allocated[index] = true;
    Ok((u32::try_from(index).map_err(|_| "TlsAlloc index overflow")?, return_address))
}

fn heap_alloc_words(vm_id: u8) -> Result<[u32; 4], &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 16)?;
    Ok([
        u32::from_le_bytes(
            frame[0..4]
                .try_into()
                .map_err(|_| "HeapAlloc return address")?,
        ),
        u32::from_le_bytes(
            frame[4..8]
                .try_into()
                .map_err(|_| "HeapAlloc heap handle")?,
        ),
        u32::from_le_bytes(frame[8..12].try_into().map_err(|_| "HeapAlloc flags")?),
        u32::from_le_bytes(frame[12..16].try_into().map_err(|_| "HeapAlloc bytes")?),
    ])
}

fn launcher_heap_range_mut(
    vm_id: u8,
    guest_address: u32,
    bytes: usize,
) -> Result<&'static mut [u8], &'static str> {
    let offset = usize::try_from(
        u64::from(guest_address)
            .checked_sub(u64::from(HEAP_VA))
            .ok_or("guest heap address below heap")?,
    )
    .map_err(|_| "guest heap address range")?;
    let end = offset
        .checked_add(bytes)
        .ok_or("guest heap range overflow")?;
    if end > HEAP_BYTES {
        return Err("guest heap range outside private heap page");
    }
    let state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock();
    let state = state.as_ref().ok_or("wc3 launcher state unavailable")?;
    Ok(unsafe {
        core::slice::from_raw_parts_mut(
            (state.arena.virt_start + HEAP_BACKING_OFFSET + offset) as *mut u8,
            bytes,
        )
    })
}

fn launcher_process_data_range_mut(
    vm_id: u8,
    guest_address: u32,
    bytes: usize,
) -> Result<&'static mut [u8], &'static str> {
    let offset = usize::try_from(
        u64::from(guest_address)
            .checked_sub(u64::from(PROCESS_DATA_VA))
            .ok_or("guest process-data address below page")?,
    )
    .map_err(|_| "guest process-data address range")?;
    let end = offset
        .checked_add(bytes)
        .ok_or("guest process-data range overflow")?;
    if end > PAGE_SIZE_4K {
        return Err("guest process-data range outside private page");
    }
    let state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock();
    let state = state.as_ref().ok_or("wc3 launcher state unavailable")?;
    Ok(unsafe {
        core::slice::from_raw_parts_mut(
            (state.arena.virt_start + PROCESS_DATA_BACKING_OFFSET + offset) as *mut u8,
            bytes,
        )
    })
}

fn no_argument_return_frame(vm_id: u8, error: &'static str) -> Result<u32, &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 4)?;
    Ok(u32::from_le_bytes(frame[0..4].try_into().map_err(|_| error)?))
}

fn get_command_line_a(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let return_address = no_argument_return_frame(vm_id, "GetCommandLineA return")?;
    if return_address != GET_COMMAND_LINE_A_RETURN {
        return Err("unexpected GetCommandLineA return address");
    }
    let output = launcher_process_data_range_mut(vm_id, PROCESS_DATA_VA, COMMAND_LINE.len())?;
    if output != COMMAND_LINE {
        return Err("GetCommandLineA process data mismatch");
    }
    Ok((PROCESS_DATA_VA, return_address))
}

fn get_environment_strings_w(vm_id: u8) -> Result<u32, &'static str> {
    let return_address = no_argument_return_frame(vm_id, "GetEnvironmentStringsW return")?;
    if return_address != GET_ENVIRONMENT_STRINGS_W_RETURN {
        return Err("unexpected GetEnvironmentStringsW return address");
    }
    Ok(return_address)
}

fn get_environment_strings_a(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let return_address = no_argument_return_frame(vm_id, "GetEnvironmentStrings return")?;
    if return_address != GET_ENVIRONMENT_STRINGS_A_RETURN {
        return Err("unexpected GetEnvironmentStrings return address");
    }
    let output = launcher_process_data_range_mut(vm_id, ENVIRONMENT_BLOCK_VA, 2)?;
    if output != [0, 0] {
        return Err("GetEnvironmentStrings process data mismatch");
    }
    Ok((ENVIRONMENT_BLOCK_VA, return_address))
}

fn free_environment_strings_a(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "FreeEnvironmentStringsA return")?,
    );
    let environment = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "FreeEnvironmentStringsA argument")?,
    );
    if return_address != FREE_ENVIRONMENT_STRINGS_A_RETURN || environment != ENVIRONMENT_BLOCK_VA {
        return Err("unexpected FreeEnvironmentStringsA frame");
    }
    Ok((environment, return_address))
}

fn heap_alloc(
    vm_id: u8,
    expected_return: u32,
    expected_flags: u32,
    expected_bytes: u32,
) -> Result<(u32, u32, u32, u32, u32), &'static str> {
    let [return_address, heap_handle, flags, bytes] = heap_alloc_words(vm_id)?;
    if return_address != expected_return {
        return Err("unexpected HeapAlloc return address");
    }
    if flags != expected_flags || bytes != expected_bytes {
        return Err("unexpected HeapAlloc arguments");
    }
    let launcher = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?;
    let expected_heap = launcher
        .lock()
        .as_ref()
        .and_then(|state| state.heap)
        .ok_or("wc3 heap handle unavailable")?
        .value;
    if heap_handle != expected_heap {
        return Err("unexpected HeapAlloc heap handle");
    }
    let state = launcher.lock();
    let next = state
        .as_ref()
        .ok_or("wc3 launcher state unavailable")?
        .heap_next;
    let aligned = next.checked_add(15).ok_or("HeapAlloc alignment overflow")? & !15;
    let end = aligned
        .checked_add(usize::try_from(bytes).map_err(|_| "HeapAlloc size")?)
        .ok_or("HeapAlloc size overflow")?;
    if end > HEAP_BYTES {
        return Err("HeapAlloc exceeds private heap page");
    }
    let guest_ptr = HEAP_VA
        .checked_add(u32::try_from(aligned).map_err(|_| "HeapAlloc pointer overflow")?)
        .ok_or("HeapAlloc pointer overflow")?;
    drop(state);
    let output = launcher_heap_range_mut(
        vm_id,
        guest_ptr,
        usize::try_from(bytes).map_err(|_| "HeapAlloc size")?,
    )?;
    if flags & HEAP_ALLOC_FLAGS != 0 {
        output.fill(0);
    }
    let mut state = launcher.lock();
    let state = state.as_mut().ok_or("wc3 launcher state unavailable")?;
    state.heap_next = end;
    state.heap_allocations.push(HeapAllocation {
        guest_ptr,
        bytes,
        flags,
    });
    Ok((guest_ptr, return_address, heap_handle, flags, bytes))
}

fn tls_set_value(vm_id: u8) -> Result<(u32, u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 12)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "TlsSetValue return address")?,
    );
    let index = u32::from_le_bytes(frame[4..8].try_into().map_err(|_| "TlsSetValue index")?);
    let value = u32::from_le_bytes(frame[8..12].try_into().map_err(|_| "TlsSetValue value")?);
    if return_address != TLS_SET_VALUE_RETURN {
        return Err("unexpected TlsSetValue return address");
    }
    if index != TLS_SET_VALUE_INDEX || value != TLS_SET_VALUE_VALUE {
        return Err("unexpected TlsSetValue arguments");
    }
    let slot = usize::try_from(index).map_err(|_| "TlsSetValue index range")?;
    let launcher = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?;
    let mut state = launcher.lock();
    let state = state.as_mut().ok_or("wc3 launcher state unavailable")?;
    if !state.tls_allocated[slot] {
        return Err("TlsSetValue index was not allocated");
    }
    state.tls_values[slot] = value;
    state.tls_value_set[slot] = true;
    Ok((index, value, return_address))
}

fn get_current_thread_id(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 4)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetCurrentThreadId return address")?,
    );
    if return_address != GET_CURRENT_THREAD_ID_RETURN {
        return Err("unexpected GetCurrentThreadId return address");
    }
    let registers = crate::hv::vmx::guest_registers();
    if registers.rsi as u32 != GET_CURRENT_THREAD_ID_ESI {
        return Err("unexpected GetCurrentThreadId ESI");
    }
    let state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock();
    let thread_id = state
        .as_ref()
        .ok_or("wc3 launcher state unavailable")?
        .primary_thread_id;
    if thread_id == 0 {
        return Err("primary guest thread id is zero");
    }
    Ok((thread_id, return_address))
}

fn get_acp(vm_id: u8) -> Result<u32, &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 4)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetACP return address")?,
    );
    if return_address != GET_ACP_RETURN {
        return Err("unexpected GetACP return address");
    }
    Ok(return_address)
}

fn get_cp_info(vm_id: u8, expected_return: u32) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 12)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetCPInfo return address")?,
    );
    let code_page = u32::from_le_bytes(frame[4..8].try_into().map_err(|_| "GetCPInfo code page")?);
    let cp_info = u32::from_le_bytes(
        frame[8..12]
            .try_into()
            .map_err(|_| "GetCPInfo output pointer")?,
    );
    if return_address != expected_return || code_page != GET_ACP_CODE_PAGE {
        return Err("unexpected GetCPInfo frame");
    }
    let output = guest_stack_range_mut(vm_id, cp_info, CP_INFO_BYTES)?;
    output.fill(0);
    output[0..4].copy_from_slice(&1u32.to_le_bytes());
    output[4] = 0x3F;
    Ok((cp_info, return_address))
}

fn launcher_read_range(
    vm_id: u8,
    guest_address: u32,
    bytes: usize,
) -> Result<Vec<u8>, &'static str> {
    if u64::from(guest_address) >= crate::hv::memory::GUEST_STACK_VA_BASE {
        return Ok(guest_stack_range_mut(vm_id, guest_address, bytes)?.to_vec());
    }
    if u64::from(guest_address) >= u64::from(pe32::IMAGE_BASE)
        && u64::from(guest_address) < u64::from(pe32::IMAGE_BASE) + pe32::IMAGE_BYTES as u64
    {
        return Ok(launcher_image_range_mut(vm_id, guest_address, bytes)?.to_vec());
    }
    if u64::from(guest_address) >= u64::from(HEAP_VA)
        && u64::from(guest_address) < u64::from(HEAP_VA) + HEAP_BYTES as u64
    {
        return Ok(launcher_heap_range_mut(vm_id, guest_address, bytes)?.to_vec());
    }
    if u64::from(guest_address) >= u64::from(PROCESS_DATA_VA)
        && u64::from(guest_address) < u64::from(PROCESS_DATA_VA) + PAGE_SIZE_4K as u64
    {
        return Ok(launcher_process_data_range_mut(vm_id, guest_address, bytes)?.to_vec());
    }
    Err("guest range is outside launcher mappings")
}

fn launcher_writable_range_mut(
    vm_id: u8,
    guest_address: u32,
    bytes: usize,
) -> Result<&'static mut [u8], &'static str> {
    if u64::from(guest_address) >= crate::hv::memory::GUEST_STACK_VA_BASE {
        return guest_stack_range_mut(vm_id, guest_address, bytes);
    }
    if u64::from(guest_address) >= u64::from(pe32::IMAGE_BASE)
        && u64::from(guest_address) < u64::from(pe32::IMAGE_BASE) + pe32::IMAGE_BYTES as u64
    {
        return launcher_image_range_mut(vm_id, guest_address, bytes);
    }
    if u64::from(guest_address) >= u64::from(HEAP_VA)
        && u64::from(guest_address) < u64::from(HEAP_VA) + HEAP_BYTES as u64
    {
        return launcher_heap_range_mut(vm_id, guest_address, bytes);
    }
    if u64::from(guest_address) >= u64::from(PROCESS_DATA_VA)
        && u64::from(guest_address) < u64::from(PROCESS_DATA_VA) + PAGE_SIZE_4K as u64
    {
        return launcher_process_data_range_mut(vm_id, guest_address, bytes);
    }
    Err("guest destination is outside writable launcher mappings")
}

fn get_string_type_w(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 20)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetStringTypeW return")?,
    );
    let info_type = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "GetStringTypeW info type")?,
    );
    let source = u32::from_le_bytes(
        frame[8..12]
            .try_into()
            .map_err(|_| "GetStringTypeW source")?,
    );
    let count = u32::from_le_bytes(
        frame[12..16]
            .try_into()
            .map_err(|_| "GetStringTypeW count")?,
    );
    let output_pointer = u32::from_le_bytes(
        frame[16..20]
            .try_into()
            .map_err(|_| "GetStringTypeW output")?,
    );
    if !matches!(return_address, GET_STRING_TYPE_W_RETURN | GET_STRING_TYPE_W_SECOND_RETURN)
        || info_type != 1
        || count == 0
        || (return_address == GET_STRING_TYPE_W_RETURN && source != GET_STRING_TYPE_W_SOURCE)
    {
        return Err("unexpected GetStringTypeW frame");
    }
    let wide_bytes = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(2))
        .ok_or("GetStringTypeW size overflow")?;
    let source_bytes = launcher_read_range(vm_id, source, wide_bytes)?;
    let output = launcher_writable_range_mut(vm_id, output_pointer, wide_bytes)?;
    for (source_word, output_word) in source_bytes.chunks_exact(2).zip(output.chunks_exact_mut(2)) {
        let value = u16::from_le_bytes([source_word[0], source_word[1]]);
        output_word.copy_from_slice(&classify_cp1252(value).to_le_bytes());
    }
    Ok((output_pointer, return_address))
}

fn multi_byte_to_wide_char(vm_id: u8) -> Result<(u32, u32, u32, u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 28)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "MultiByteToWideChar return")?,
    );
    let code_page = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "MultiByteToWideChar code page")?,
    );
    let flags = u32::from_le_bytes(
        frame[8..12]
            .try_into()
            .map_err(|_| "MultiByteToWideChar flags")?,
    );
    let source = u32::from_le_bytes(
        frame[12..16]
            .try_into()
            .map_err(|_| "MultiByteToWideChar source")?,
    );
    let source_count = i32::from_le_bytes(
        frame[16..20]
            .try_into()
            .map_err(|_| "MultiByteToWideChar source count")?,
    );
    let destination = u32::from_le_bytes(
        frame[20..24]
            .try_into()
            .map_err(|_| "MultiByteToWideChar destination")?,
    );
    let destination_count = i32::from_le_bytes(
        frame[24..28]
            .try_into()
            .map_err(|_| "MultiByteToWideChar destination count")?,
    );
    super::trace::info(format_args!(
        "MultiByteToWideChar frame ret=0x{:08X} cp={} flags=0x{:08X} src=0x{:08X} src_count={} dst=0x{:08X} dst_count={}",
        return_address, code_page, flags, source, source_count, destination, destination_count
    ));
    let site = multi_byte_to_wide_char_site(return_address)
        .ok_or("unexpected MultiByteToWideChar return site")?;
    if code_page != MULTI_BYTE_TO_WIDE_CHAR_CODE_PAGE
        || flags != 1
        || source_count == 0
        || destination_count < 0
    {
        return Err("unexpected MultiByteToWideChar frame");
    }
    let source_limit = if source_count < 0 {
        0x10000usize
    } else {
        usize::try_from(source_count).map_err(|_| "MultiByteToWideChar source count")?
    };
    let source_bytes = launcher_read_range(vm_id, source, source_limit)?;
    let source_len = if source_count < 0 {
        source_bytes
            .iter()
            .position(|byte| *byte == 0)
            .map(|index| index + 1)
            .ok_or("MultiByteToWideChar unterminated source")?
    } else {
        source_bytes.len()
    };
    let required = source_len;
    if destination == 0 || destination_count == 0 {
        if !matches!(
            site,
            MultiByteToWideCharSite::LocaleSizing | MultiByteToWideCharSite::AnsiMapSizing
        ) || destination != 0
            || destination_count != 0
        {
            return Err("unexpected MultiByteToWideChar conversion destination");
        }
        return Ok((required as u32, return_address, source, destination, required as u32));
    }
    if !matches!(
        site,
        MultiByteToWideCharSite::LocaleConversion | MultiByteToWideCharSite::AnsiMapConversion
    ) || destination == 0
        || destination_count == 0
        || usize::try_from(destination_count)
            .map_err(|_| "MultiByteToWideChar destination count")?
            < required
    {
        return Err("unexpected MultiByteToWideChar conversion size");
    }
    let output_bytes = required
        .checked_mul(2)
        .ok_or("MultiByteToWideChar output overflow")?;
    let output = launcher_writable_range_mut(vm_id, destination, output_bytes)?;
    for (index, byte) in source_bytes[..source_len].iter().enumerate() {
        output[index * 2..index * 2 + 2].copy_from_slice(&decode_cp1252(*byte).to_le_bytes());
    }
    Ok((required as u32, return_address, source, destination, required as u32))
}

fn lc_map_string_w(vm_id: u8) -> Result<(u32, u32, u32, u32, u32, u32, u16), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 28)?;
    let return_address =
        u32::from_le_bytes(frame[0..4].try_into().map_err(|_| "LCMapStringW return")?);
    let locale = u32::from_le_bytes(frame[4..8].try_into().map_err(|_| "LCMapStringW locale")?);
    let flags = u32::from_le_bytes(frame[8..12].try_into().map_err(|_| "LCMapStringW flags")?);
    let source = u32::from_le_bytes(
        frame[12..16]
            .try_into()
            .map_err(|_| "LCMapStringW source")?,
    );
    let source_count = i32::from_le_bytes(
        frame[16..20]
            .try_into()
            .map_err(|_| "LCMapStringW source count")?,
    );
    let destination = u32::from_le_bytes(
        frame[20..24]
            .try_into()
            .map_err(|_| "LCMapStringW destination")?,
    );
    let destination_count = i32::from_le_bytes(
        frame[24..28]
            .try_into()
            .map_err(|_| "LCMapStringW destination count")?,
    );
    super::trace::info(format_args!(
        "LCMapStringW frame ret=0x{:08X} locale=0x{:08X} flags=0x{:08X} source=0x{:08X} source_count={} destination=0x{:08X} destination_count={}",
        return_address, locale, flags, source, source_count, destination, destination_count
    ));
    if !matches!(
        return_address,
        LC_MAP_STRING_W_PROBE_RETURN
            | LC_MAP_STRING_W_ANSI_SIZING_RETURN
            | LC_MAP_STRING_W_ANSI_OUTPUT_RETURN
            | LC_MAP_STRING_W_WIDE_OUTPUT_RETURN
    ) || locale != 0
        || !matches!(flags, LC_MAP_STRING_W_LOWERCASE | LC_MAP_STRING_W_UPPERCASE)
        || source == 0
        || source_count == 0
        || destination_count < 0
        || (destination == 0 && destination_count != 0)
        || (destination != 0 && destination_count == 0)
        || (return_address == LC_MAP_STRING_W_PROBE_RETURN
            && (source != GET_STRING_TYPE_W_SOURCE
                || source_count != 1
                || destination != 0
                || destination_count != 0))
        || (return_address == LC_MAP_STRING_W_ANSI_SIZING_RETURN && destination != 0)
        || (matches!(
            return_address,
            LC_MAP_STRING_W_ANSI_OUTPUT_RETURN | LC_MAP_STRING_W_WIDE_OUTPUT_RETURN
        ) && destination == 0)
    {
        return Err("unexpected LCMapStringW frame");
    }
    let source_bytes = if source_count < 0 {
        let bytes = launcher_read_range(vm_id, source, PAGE_SIZE_4K)?;
        let end = bytes
            .chunks_exact(2)
            .position(|word| word == [0, 0])
            .map(|index| (index + 1) * 2)
            .ok_or("LCMapStringW unterminated source")?;
        bytes[..end].to_vec()
    } else {
        let bytes = usize::try_from(source_count)
            .ok()
            .and_then(|count| count.checked_mul(2))
            .ok_or("LCMapStringW source size overflow")?;
        launcher_read_range(vm_id, source, bytes)?
    };
    let required = source_bytes.len() / 2;
    let first_source = u16::from_le_bytes([source_bytes[0], source_bytes[1]]);
    if destination == 0 {
        return Ok((
            required as u32,
            return_address,
            flags,
            source,
            destination,
            required as u32,
            first_source,
        ));
    }
    let capacity = usize::try_from(destination_count).map_err(|_| "LCMapStringW capacity")?;
    if capacity < required {
        return Err("LCMapStringW destination too small");
    }
    let output_bytes = required
        .checked_mul(2)
        .ok_or("LCMapStringW output overflow")?;
    let output = launcher_writable_range_mut(vm_id, destination, output_bytes)?;
    for (index, word) in source_bytes.chunks_exact(2).enumerate() {
        let value = u16::from_le_bytes([word[0], word[1]]);
        let mapped = if flags == LC_MAP_STRING_W_LOWERCASE {
            map_cp1252_lowercase(value)
        } else {
            map_cp1252_uppercase(value)
        };
        output[index * 2..index * 2 + 2].copy_from_slice(&mapped.to_le_bytes());
    }
    Ok((required as u32, return_address, flags, source, destination, required as u32, first_source))
}

fn map_cp1252_lowercase(value: u16) -> u16 {
    match value {
        0x0041..=0x005A => value + 0x20,
        0x00C0..=0x00D6 | 0x00D8..=0x00DE => value + 0x20,
        0x0160 => 0x0161,
        0x0152 => 0x0153,
        0x017D => 0x017E,
        0x0178 => 0x00FF,
        _ => value,
    }
}

fn map_cp1252_uppercase(value: u16) -> u16 {
    match value {
        0x0061..=0x007A => value - 0x20,
        0x00E0..=0x00F6 | 0x00F8..=0x00FE => value - 0x20,
        0x0161 => 0x0160,
        0x0153 => 0x0152,
        0x017E => 0x017D,
        0x00FF => 0x0178,
        _ => value,
    }
}

fn encode_cp1252(value: u16) -> Option<u8> {
    Some(match value {
        0x0000..=0x007F | 0x00A0..=0x00FF => value as u8,
        0x0081 => 0x81,
        0x008D => 0x8D,
        0x008F => 0x8F,
        0x0090 => 0x90,
        0x009D => 0x9D,
        0x20AC => 0x80,
        0x201A => 0x82,
        0x0192 => 0x83,
        0x201E => 0x84,
        0x2026 => 0x85,
        0x2020 => 0x86,
        0x2021 => 0x87,
        0x02C6 => 0x88,
        0x2030 => 0x89,
        0x0160 => 0x8A,
        0x2039 => 0x8B,
        0x0152 => 0x8C,
        0x017D => 0x8E,
        0x2018 => 0x91,
        0x2019 => 0x92,
        0x201C => 0x93,
        0x201D => 0x94,
        0x2022 => 0x95,
        0x2013 => 0x96,
        0x2014 => 0x97,
        0x02DC => 0x98,
        0x2122 => 0x99,
        0x0161 => 0x9A,
        0x203A => 0x9B,
        0x0153 => 0x9C,
        0x017E => 0x9E,
        0x0178 => 0x9F,
        _ => return None,
    })
}

fn wide_char_to_multi_byte(vm_id: u8) -> Result<(u32, u32, u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 36)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "WideCharToMultiByte return")?,
    );
    let code_page = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "WideCharToMultiByte code page")?,
    );
    let flags = u32::from_le_bytes(
        frame[8..12]
            .try_into()
            .map_err(|_| "WideCharToMultiByte flags")?,
    );
    let source = u32::from_le_bytes(
        frame[12..16]
            .try_into()
            .map_err(|_| "WideCharToMultiByte source")?,
    );
    let source_count = i32::from_le_bytes(
        frame[16..20]
            .try_into()
            .map_err(|_| "WideCharToMultiByte source count")?,
    );
    let destination = u32::from_le_bytes(
        frame[20..24]
            .try_into()
            .map_err(|_| "WideCharToMultiByte destination")?,
    );
    let destination_count = i32::from_le_bytes(
        frame[24..28]
            .try_into()
            .map_err(|_| "WideCharToMultiByte destination count")?,
    );
    let default_char = u32::from_le_bytes(
        frame[28..32]
            .try_into()
            .map_err(|_| "WideCharToMultiByte default char")?,
    );
    let used_default = u32::from_le_bytes(
        frame[32..36]
            .try_into()
            .map_err(|_| "WideCharToMultiByte used default")?,
    );
    super::trace::info(format_args!(
        "WideCharToMultiByte frame ret=0x{:08X} cp={} flags=0x{:08X} src=0x{:08X} src_count={} dst=0x{:08X} dst_count={} default=0x{:08X} used_default=0x{:08X}",
        return_address,
        code_page,
        flags,
        source,
        source_count,
        destination,
        destination_count,
        default_char,
        used_default
    ));
    if return_address != WIDE_CHAR_TO_MULTI_BYTE_RETURN
        || code_page != MULTI_BYTE_TO_WIDE_CHAR_CODE_PAGE
        || flags != WIDE_CHAR_TO_MULTI_BYTE_FLAGS
        || source == 0
        || source_count <= 0
        || destination_count < 0
        || default_char != 0
        || used_default != 0
    {
        return Err("unexpected WideCharToMultiByte frame");
    }
    let source_words = usize::try_from(source_count)
        .map_err(|_| "WideCharToMultiByte source count")?
        .checked_mul(2)
        .ok_or("WideCharToMultiByte source size overflow")?;
    let source_bytes = launcher_read_range(vm_id, source, source_words)?;
    let mut encoded = Vec::with_capacity(source_words / 2);
    for word in source_bytes.chunks_exact(2) {
        encoded.push(encode_cp1252(u16::from_le_bytes([word[0], word[1]])).unwrap_or(b'?'));
    }
    let required = encoded.len();
    if destination == 0 || destination_count == 0 {
        if destination != 0 || destination_count != 0 {
            return Err("unexpected WideCharToMultiByte sizing destination");
        }
        return Ok((required as u32, return_address, source, destination));
    }
    let capacity =
        usize::try_from(destination_count).map_err(|_| "WideCharToMultiByte destination count")?;
    if capacity < required {
        return Err("WideCharToMultiByte destination too small");
    }
    let output = launcher_writable_range_mut(vm_id, destination, required)?;
    output.copy_from_slice(&encoded);
    Ok((required as u32, return_address, source, destination))
}

fn decode_cp1252(byte: u8) -> u16 {
    match byte {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        _ => u16::from(byte),
    }
}

const C1_UPPER: u16 = 0x0001;
const C1_LOWER: u16 = 0x0002;
const C1_DIGIT: u16 = 0x0004;
const C1_SPACE: u16 = 0x0008;
const C1_PUNCT: u16 = 0x0010;
const C1_CNTRL: u16 = 0x0020;
const C1_BLANK: u16 = 0x0040;
const C1_XDIGIT: u16 = 0x0080;
const C1_ALPHA: u16 = 0x0100;
const C1_DEFINED: u16 = 0x0200;

fn classify_cp1252(value: u16) -> u16 {
    let mut result = C1_DEFINED;
    if value < 0x20 || (0x7F..=0x9F).contains(&value) {
        result |= C1_CNTRL;
    }
    if matches!(value, 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20 | 0x00A0) {
        result |= C1_SPACE;
    }
    if matches!(value, 0x09 | 0x20 | 0x00A0) {
        result |= C1_BLANK;
    }
    if (0x30..=0x39).contains(&value) {
        result |= C1_DIGIT;
    }
    if (0x30..=0x39).contains(&value)
        || (0x41..=0x46).contains(&value)
        || (0x61..=0x66).contains(&value)
    {
        result |= C1_XDIGIT;
    }
    if (0x41..=0x5A).contains(&value) {
        result |= C1_UPPER | C1_ALPHA;
    } else if (0x61..=0x7A).contains(&value) {
        result |= C1_LOWER | C1_ALPHA;
    } else if matches!(
        value,
        0x00C0..=0x00D6
            | 0x00D8..=0x00DE
            | 0x00E0..=0x00F6
            | 0x00F8..=0x00FF
            | 0x0152
            | 0x0153
            | 0x0160
            | 0x0161
            | 0x0178
            | 0x017D
            | 0x017E
    ) {
        result |= C1_ALPHA;
        if map_cp1252_lowercase(value) != value {
            result |= C1_UPPER;
        }
        if map_cp1252_uppercase(value) != value {
            result |= C1_LOWER;
        }
    }
    if result & (C1_ALPHA | C1_DIGIT | C1_SPACE | C1_CNTRL) == 0 {
        result |= C1_PUNCT;
    }
    result
}

#[cfg(test)]
mod locale_tests {
    use super::{
        C1_ALPHA, C1_BLANK, C1_CNTRL, C1_DIGIT, C1_LOWER, C1_PUNCT, C1_SPACE, C1_UPPER,
        classify_cp1252, decode_cp1252,
    };

    #[test]
    fn cp1252_decodes_extended_bytes() {
        assert_eq!(decode_cp1252(0x80), 0x20AC);
        assert_eq!(decode_cp1252(0x8C), 0x0152);
        assert_eq!(decode_cp1252(0x8E), 0x017D);
        assert_eq!(decode_cp1252(0x91), 0x2018);
        assert_eq!(decode_cp1252(0x92), 0x2019);
        assert_eq!(decode_cp1252(0x9C), 0x0153);
        assert_eq!(decode_cp1252(0x9E), 0x017E);
        assert_eq!(decode_cp1252(0x9F), 0x0178);
        assert_eq!(super::encode_cp1252(0x0081), Some(0x81));
        assert_eq!(super::encode_cp1252(0x0152), Some(0x8C));
    }

    #[test]
    fn ctype1_distinguishes_launcher_characters() {
        assert_ne!(classify_cp1252('A' as u16) & C1_UPPER, 0);
        assert_ne!(classify_cp1252('A' as u16) & C1_ALPHA, 0);
        assert_ne!(classify_cp1252('a' as u16) & C1_LOWER, 0);
        assert_ne!(classify_cp1252('0' as u16) & C1_DIGIT, 0);
        assert_ne!(classify_cp1252(' ' as u16) & C1_SPACE, 0);
        assert_ne!(classify_cp1252(' ' as u16) & C1_BLANK, 0);
        assert_ne!(classify_cp1252('\t' as u16) & C1_SPACE, 0);
        assert_ne!(classify_cp1252('\n' as u16) & C1_CNTRL, 0);
        assert_ne!(classify_cp1252('.' as u16) & C1_PUNCT, 0);
        assert_ne!(classify_cp1252(0x0152) & C1_ALPHA, 0);
    }
}

fn get_startup_info_a(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetStartupInfoA return address")?,
    );
    let startup_info = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "GetStartupInfoA argument")?,
    );
    if !matches!(return_address, GET_STARTUP_INFO_A_RETURN | GET_STARTUP_INFO_A_SECOND_RETURN) {
        return Err("unexpected GetStartupInfoA return address");
    }
    if startup_info == 0 {
        return Err("GetStartupInfoA null pointer");
    }
    let output = guest_stack_range_mut(vm_id, startup_info, STARTUP_INFO_A_BYTES)?;
    output.fill(0);
    output[0..4].copy_from_slice(&(STARTUP_INFO_A_BYTES as u32).to_le_bytes());
    if u32::from_le_bytes(output[0..4].try_into().map_err(|_| "GetStartupInfoA cb")?)
        != STARTUP_INFO_A_BYTES as u32
        || u32::from_le_bytes(
            output[0x2c..0x30]
                .try_into()
                .map_err(|_| "GetStartupInfoA flags")?,
        ) != 0
        || u16::from_le_bytes(
            output[0x32..0x34]
                .try_into()
                .map_err(|_| "GetStartupInfoA reserved2 size")?,
        ) != 0
        || u32::from_le_bytes(
            output[0x34..0x38]
                .try_into()
                .map_err(|_| "GetStartupInfoA reserved2 pointer")?,
        ) != 0
    {
        return Err("GetStartupInfoA structure validation");
    }
    Ok((startup_info, return_address))
}

fn get_module_file_name_a(vm_id: u8) -> Result<(u32, u32, u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 16)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetModuleFileNameA return address")?,
    );
    let module = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "GetModuleFileNameA module")?,
    );
    let filename = u32::from_le_bytes(
        frame[8..12]
            .try_into()
            .map_err(|_| "GetModuleFileNameA filename")?,
    );
    let size = u32::from_le_bytes(
        frame[12..16]
            .try_into()
            .map_err(|_| "GetModuleFileNameA size")?,
    );
    super::trace::info(format_args!(
        "GetModuleFileNameA frame ret=0x{:08X} hModule=0x{:08X} lpFilename=0x{:08X} nSize={}",
        return_address, module, filename, size
    ));
    if return_address != GET_MODULE_FILE_NAME_A_RETURN
        || module != 0
        || filename != GET_MODULE_FILE_NAME_A_BUFFER
        || size != GET_MODULE_FILE_NAME_A_SIZE
    {
        return Err("unexpected GetModuleFileNameA frame");
    }
    let output = launcher_image_range_mut(
        vm_id,
        filename,
        usize::try_from(size).map_err(|_| "GetModuleFileNameA size")?,
    )?;
    output.fill(0);
    output[..MODULE_FILENAME_A.len()].copy_from_slice(MODULE_FILENAME_A);
    let length =
        u32::try_from(MODULE_FILENAME_A.len() - 1).map_err(|_| "GetModuleFileNameA length")?;
    Ok((module, filename, size, length))
}

fn get_module_handle_a(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetModuleHandleA return address")?,
    );
    let module_name = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "GetModuleHandleA module name")?,
    );
    if return_address != GET_MODULE_HANDLE_A_RETURN || module_name != 0 {
        return Err("unexpected GetModuleHandleA frame");
    }
    Ok((module_name, return_address))
}

fn get_std_handle(vm_id: u8, call: u32) -> Result<(u32, u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetStdHandle return address")?,
    );
    let which = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "GetStdHandle argument")?,
    );
    let stream = match call {
        14 => 0,
        16 => 1,
        18 => 2,
        _ => return Err("unexpected GetStdHandle sequence"),
    };
    let expected_which = 0xFFFF_FFF6u32
        .checked_sub(u32::try_from(stream).map_err(|_| "GetStdHandle stream")?)
        .ok_or("GetStdHandle argument underflow")?;
    if return_address != GET_STD_HANDLE_RETURN || which != expected_which {
        return Err("unexpected GetStdHandle frame");
    }
    let state = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock();
    let handle = state
        .as_ref()
        .ok_or("wc3 launcher state unavailable")?
        .std_handles[stream];
    if handle == 0 || handle == u32::MAX {
        return Err("invalid WC3 standard handle");
    }
    Ok((which, handle, return_address))
}

fn get_file_type(vm_id: u8, call: u32) -> Result<(u32, u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "GetFileType return address")?,
    );
    let handle = u32::from_le_bytes(frame[4..8].try_into().map_err(|_| "GetFileType argument")?);
    let stream = match call {
        15 => 0,
        17 => 1,
        19 => 2,
        _ => return Err("unexpected GetFileType sequence"),
    };
    let expected_handle = LAUNCHERS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 launcher VM id")?
        .lock()
        .as_ref()
        .ok_or("wc3 launcher state unavailable")?
        .std_handles[stream];
    if return_address != GET_FILE_TYPE_RETURN || handle != expected_handle {
        return Err("unexpected GetFileType frame");
    }
    Ok((handle, 2, return_address))
}

fn set_handle_count(vm_id: u8) -> Result<(u32, u32), &'static str> {
    let esp = crate::hv::vmx::vmread(crate::hv::vmx::VMCS_GUEST_RSP)
        .ok_or("guest ESP unavailable")? as u32;
    let frame = guest_stack_range_mut(vm_id, esp, 8)?;
    let return_address = u32::from_le_bytes(
        frame[0..4]
            .try_into()
            .map_err(|_| "SetHandleCount return address")?,
    );
    let count = u32::from_le_bytes(
        frame[4..8]
            .try_into()
            .map_err(|_| "SetHandleCount argument")?,
    );
    if return_address != SET_HANDLE_COUNT_RETURN || count != 32 {
        return Err("unexpected SetHandleCount frame");
    }
    Ok((count, return_address))
}

pub(crate) fn is_initialized_critical_section(vm_id: u8, guest_address: u32) -> bool {
    LAUNCHERS
        .get(usize::from(vm_id))
        .and_then(|slot| {
            slot.lock()
                .as_ref()
                .map(|state| state.initialized_critical_sections)
        })
        .is_some_and(|addresses| addresses.contains(&guest_address))
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
    } else if (4..=7).contains(&call) && imports::is_initialize_critical_section(import) {
        match initialize_critical_section(vm_id, call) {
            Ok((critical_section, return_address)) => {
                super::trace::info(format_args!(
                    "InitializeCriticalSection ptr=0x{:08X} ret=0x{:08X}",
                    critical_section, return_address
                ));
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!InitializeCriticalSection",
                    call
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1e failed vm={} phase=InitializeCriticalSection reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 8 && imports::is_tls_alloc(import) {
        match tls_alloc(vm_id) {
            Ok((index, return_address)) => {
                if index == u32::MAX {
                    super::trace::fail(format_args!(
                        "gate-1f failed vm={} phase=TlsAlloc reason=invalid-index",
                        vm_id
                    ));
                    return DispatchOutcome::Stop;
                }
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(index);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #8 KERNEL32.dll!TlsAlloc index={} ret=0x{:08X}",
                    index, return_address
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1f failed vm={} phase=TlsAlloc reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 9 && imports::is_heap_alloc(import) {
        match heap_alloc(vm_id, HEAP_ALLOC_RETURN, HEAP_ALLOC_FLAGS, HEAP_ALLOC_BYTES) {
            Ok((guest_ptr, return_address, heap_handle, flags, bytes)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(guest_ptr);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "HeapAlloc heap=0x{:08X} flags=0x{:08X} bytes=0x{:08X} ret=0x{:08X}",
                    heap_handle, flags, bytes, return_address
                ));
                super::trace::info(format_args!(
                    "return #9 KERNEL32.dll!HeapAlloc ptr=0x{:08X} zeroed=1",
                    guest_ptr
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1g failed vm={} phase=HeapAlloc reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 10 && imports::is_tls_set_value(import) {
        match tls_set_value(vm_id) {
            Ok((index, value, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = 1;
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "TlsSetValue index={} value=0x{:08X} ret=0x{:08X}",
                    index, value, return_address
                ));
                super::trace::info(format_args!("return #10 KERNEL32.dll!TlsSetValue eax=1"));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1h failed vm={} phase=TlsSetValue reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 11 && imports::is_get_current_thread_id(import) {
        match get_current_thread_id(vm_id) {
            Ok((thread_id, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(thread_id);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #11 KERNEL32.dll!GetCurrentThreadId tid={} ret=0x{:08X}",
                    thread_id, return_address
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1i failed vm={} phase=GetCurrentThreadId reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 12 && imports::is_heap_alloc(import) {
        match heap_alloc(
            vm_id,
            HEAP_ALLOC_SECOND_RETURN,
            HEAP_ALLOC_SECOND_FLAGS,
            HEAP_ALLOC_SECOND_BYTES,
        ) {
            Ok((guest_ptr, return_address, heap_handle, flags, bytes)) => {
                if guest_ptr == 0 {
                    super::trace::fail(format_args!(
                        "gate-1j failed vm={} phase=HeapAlloc reason=null-pointer",
                        vm_id
                    ));
                    return DispatchOutcome::Stop;
                }
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(guest_ptr);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "HeapAlloc heap=0x{:08X} flags=0x{:08X} bytes=0x{:08X} ret=0x{:08X}",
                    heap_handle, flags, bytes, return_address
                ));
                super::trace::info(format_args!(
                    "return #12 KERNEL32.dll!HeapAlloc ptr=0x{:08X}",
                    guest_ptr
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1j failed vm={} phase=HeapAlloc reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_get_startup_info_a(import) {
        match get_startup_info_a(vm_id) {
            Ok((startup_info, return_address)) => {
                super::trace::info(format_args!(
                    "GetStartupInfoA arg=0x{:08X} bytes=0x44 ret=0x{:08X}",
                    startup_info, return_address
                ));
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!GetStartupInfoA cb=0x44 flags=0 reserved2=0",
                    call
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1k failed vm={} phase=GetStartupInfoA reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if matches!(call, 14 | 16 | 18) && imports::is_get_std_handle(import) {
        match get_std_handle(vm_id, call) {
            Ok((which, handle, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(handle);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "GetStdHandle which={} handle=0x{:08X} ret=0x{:08X}",
                    which as i32, handle, return_address
                ));
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!GetStdHandle handle=0x{:08X}",
                    call, handle
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1l failed vm={} phase=GetStdHandle reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if matches!(call, 15 | 17 | 19) && imports::is_get_file_type(import) {
        match get_file_type(vm_id, call) {
            Ok((handle, file_type, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(file_type);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "GetFileType handle=0x{:08X} type={} ret=0x{:08X}",
                    handle, file_type, return_address
                ));
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!GetFileType type={}",
                    call, file_type
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1l failed vm={} phase=GetFileType reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 20 && imports::is_set_handle_count(import) {
        match set_handle_count(vm_id) {
            Ok((count, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(count);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "SetHandleCount count={} ret=0x{:08X}",
                    count, return_address
                ));
                super::trace::info(format_args!(
                    "return #20 KERNEL32.dll!SetHandleCount eax={}",
                    count
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1l failed vm={} phase=SetHandleCount reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 21 && imports::is_get_command_line_a(import) {
        match get_command_line_a(vm_id) {
            Ok((pointer, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(pointer);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #21 KERNEL32.dll!GetCommandLineA ptr=0x{:08X} value=\"Warcraft III.exe\"",
                    pointer
                ));
                let _ = return_address;
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1m failed vm={} phase=GetCommandLineA reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 22 && imports::is_get_environment_strings_w(import) {
        match get_environment_strings_w(vm_id) {
            Ok(return_address) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = 0;
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #22 KERNEL32.dll!GetEnvironmentStringsW ptr=0x00000000 ret=0x{:08X}",
                    return_address
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1m failed vm={} phase=GetEnvironmentStringsW reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 23 && imports::is_get_environment_strings_a(import) {
        match get_environment_strings_a(vm_id) {
            Ok((pointer, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(pointer);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #23 KERNEL32.dll!GetEnvironmentStrings ptr=0x{:08X} empty=1 ret=0x{:08X}",
                    pointer, return_address
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1m failed vm={} phase=GetEnvironmentStrings reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 24 && imports::is_heap_alloc(import) {
        match heap_alloc(
            vm_id,
            HEAP_ALLOC_THIRD_RETURN,
            HEAP_ALLOC_THIRD_FLAGS,
            HEAP_ALLOC_THIRD_BYTES,
        ) {
            Ok((guest_ptr, return_address, heap_handle, flags, bytes)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(guest_ptr);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "HeapAlloc heap=0x{:08X} flags=0x{:08X} bytes=0x{:08X} ret=0x{:08X}",
                    heap_handle, flags, bytes, return_address
                ));
                super::trace::info(format_args!(
                    "return #24 KERNEL32.dll!HeapAlloc ptr=0x{:08X}",
                    guest_ptr
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1m failed vm={} phase=HeapAlloc reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 25 && imports::is_free_environment_strings_a(import) {
        match free_environment_strings_a(vm_id) {
            Ok((pointer, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = 1;
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "FreeEnvironmentStringsA ptr=0x{:08X} ret=0x{:08X}",
                    pointer, return_address
                ));
                super::trace::info(format_args!(
                    "return #25 KERNEL32.dll!FreeEnvironmentStringsA eax=1"
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1m failed vm={} phase=FreeEnvironmentStringsA reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 26 && imports::is_heap_alloc(import) {
        match heap_alloc(
            vm_id,
            HEAP_ALLOC_LOCK_RETURN,
            HEAP_ALLOC_LOCK_FLAGS,
            HEAP_ALLOC_LOCK_BYTES,
        ) {
            Ok((guest_ptr, return_address, heap_handle, flags, bytes)) => {
                if guest_ptr != DYNAMIC_LOCK_25 {
                    super::trace::fail(format_args!(
                        "gate-1n failed vm={} phase=HeapAlloc reason=unexpected-pointer",
                        vm_id
                    ));
                    return DispatchOutcome::Stop;
                }
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(guest_ptr);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "HeapAlloc heap=0x{:08X} flags=0x{:08X} bytes=0x{:08X} ret=0x{:08X}",
                    heap_handle, flags, bytes, return_address
                ));
                super::trace::info(format_args!(
                    "return #26 KERNEL32.dll!HeapAlloc ptr=0x{:08X}",
                    guest_ptr
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1n failed vm={} phase=HeapAlloc reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 27 && imports::is_enter_critical_section(import) {
        match enter_critical_section(vm_id, STATIC_LOCK_17) {
            Ok((pointer, tid, return_address)) => {
                super::trace::info(format_args!(
                    "EnterCriticalSection ptr=0x{:08X} tid={} ret=0x{:08X}",
                    pointer, tid, return_address
                ));
                super::trace::info(format_args!("return #27 KERNEL32.dll!EnterCriticalSection"));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1n failed vm={} phase=EnterCriticalSection reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 28 && imports::is_initialize_critical_section(import) {
        match initialize_dynamic_critical_section(vm_id) {
            Ok((pointer, return_address)) => {
                super::trace::info(format_args!(
                    "InitializeCriticalSection ptr=0x{:08X} ret=0x{:08X}",
                    pointer, return_address
                ));
                super::trace::info(format_args!(
                    "return #28 KERNEL32.dll!InitializeCriticalSection"
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1n failed vm={} phase=InitializeCriticalSection reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_leave_critical_section(import) {
        match leave_critical_section(vm_id) {
            Ok((pointer, tid, return_address)) => {
                super::trace::info(format_args!(
                    "LeaveCriticalSection ptr=0x{:08X} tid={} ret=0x{:08X}",
                    pointer, tid, return_address
                ));
                super::trace::info(format_args!("return #29 KERNEL32.dll!LeaveCriticalSection"));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1n failed vm={} phase=LeaveCriticalSection reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 30 && imports::is_enter_critical_section(import) {
        match enter_critical_section(vm_id, DYNAMIC_LOCK_25) {
            Ok((pointer, tid, return_address)) => {
                super::trace::info(format_args!(
                    "EnterCriticalSection ptr=0x{:08X} tid={} ret=0x{:08X}",
                    pointer, tid, return_address
                ));
                super::trace::info(format_args!("return #30 KERNEL32.dll!EnterCriticalSection"));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1n failed vm={} phase=EnterCriticalSection reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 31 && imports::is_get_acp(import) {
        match get_acp(vm_id) {
            Ok(return_address) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(GET_ACP_CODE_PAGE);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #31 KERNEL32.dll!GetACP codepage=1252 ret=0x{:08X}",
                    return_address
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1o failed vm={} phase=GetACP reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 32 && imports::is_get_cp_info(import) {
        match get_cp_info(vm_id, GET_CP_INFO_FIRST_RETURN) {
            Ok((cp_info, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = 1;
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "GetCPInfo codepage=1252 max_char_size=1 default=0x3F ret=0x{:08X}",
                    return_address
                ));
                super::trace::info(format_args!(
                    "return #32 KERNEL32.dll!GetCPInfo eax=1 lp=0x{:08X}",
                    cp_info
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1o failed vm={} phase=GetCPInfo reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if call == 33 && imports::is_get_cp_info(import) {
        match get_cp_info(vm_id, GET_CP_INFO_SECOND_RETURN) {
            Ok((cp_info, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = 1;
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "GetCPInfo codepage=1252 max_char_size=1 default=0x3F ret=0x{:08X}",
                    return_address
                ));
                super::trace::info(format_args!(
                    "return #33 KERNEL32.dll!GetCPInfo eax=1 lp=0x{:08X}",
                    cp_info
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1o failed vm={} phase=GetCPInfo reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_get_string_type_w(import) {
        match get_string_type_w(vm_id) {
            Ok((output_pointer, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = 1;
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!GetStringTypeW eax=1 lp=0x{:08X} ret=0x{:08X}",
                    call, output_pointer, return_address
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "gate-1o failed vm={} phase=GetStringTypeW reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_lc_map_string_w(import) {
        match lc_map_string_w(vm_id) {
            Ok((mapped, return_address, flags, source, destination, required, first_source)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(mapped);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "LCMapStringW locale=0 flags=0x{:08X} source=0x{:08X} destination=0x{:08X} count={} required={} first=0x{:04X} ret=0x{:08X}",
                    flags, source, destination, mapped, required, first_source, return_address
                ));
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!LCMapStringW eax={}",
                    call, mapped
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "locale failed vm={} phase=LCMapStringW call={} reason={}",
                    vm_id, call, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_multi_byte_to_wide_char(import) {
        match multi_byte_to_wide_char(vm_id) {
            Ok((converted, return_address, source, destination, required)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(converted);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "MultiByteToWideChar codepage=1252 source=0x{:08X} destination=0x{:08X} count={} required={} ret=0x{:08X}",
                    source, destination, converted, required, return_address
                ));
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!MultiByteToWideChar eax={}",
                    call, converted
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "locale failed vm={} phase=MultiByteToWideChar call={} reason={}",
                    vm_id, call, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_wide_char_to_multi_byte(import) {
        match wide_char_to_multi_byte(vm_id) {
            Ok((converted, return_address, source, destination)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(converted);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "WideCharToMultiByte codepage=1252 source=0x{:08X} destination=0x{:08X} count={} ret=0x{:08X}",
                    source, destination, converted, return_address
                ));
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!WideCharToMultiByte eax={}",
                    call, converted
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "locale failed vm={} phase=WideCharToMultiByte call={} reason={}",
                    vm_id, call, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_get_module_file_name_a(import) {
        match get_module_file_name_a(vm_id) {
            Ok((module, filename, size, length)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(length);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "return #{} KERNEL32.dll!GetModuleFileNameA length={} path=C:\\Warcraft III\\Warcraft III.exe",
                    call, length
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "crt-startup failed vm={} phase=GetModuleFileNameA reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_get_module_handle_a(import) {
        match get_module_handle_a(vm_id) {
            Ok((module_name, return_address)) => {
                let mut registers = crate::hv::vmx::guest_registers();
                registers.rax = u64::from(MODULE_IMAGE_BASE);
                crate::hv::vmx::set_guest_registers(registers);
                super::trace::info(format_args!(
                    "GetModuleHandleA hModuleName=0x{:08X} image_base=0x{:08X} ret=0x{:08X}",
                    module_name, MODULE_IMAGE_BASE, return_address
                ));
                super::trace::info(format_args!(
                    "crt-startup complete vm={} main=0x00401000",
                    vm_id
                ));
                DispatchOutcome::Resume
            }
            Err(reason) => {
                super::trace::fail(format_args!(
                    "crt-startup failed vm={} phase=GetModuleHandleA reason={}",
                    vm_id, reason
                ));
                DispatchOutcome::Stop
            }
        }
    } else if imports::is_get_tick_count(import) {
        super::trace::info(format_args!("main entered vm={} address=0x00401000", vm_id));
        super::trace::info(format_args!(
            "crt-startup complete vm={} next-import={}!{} call={}",
            vm_id, import.module, import.symbol, call
        ));
        super::trace::info(format_args!(
            "get-tick-count frontier vm={} import={}!{} call={}",
            vm_id, import.module, import.symbol, call
        ));
        DispatchOutcome::Stop
    } else {
        super::trace::fail(format_args!(
            "gate-1e failed vm={} phase=unexpected-{}-import {}!{}",
            vm_id, call, import.module, import.symbol
        ));
        DispatchOutcome::Stop
    }
}
