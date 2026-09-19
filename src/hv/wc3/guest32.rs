use core::sync::atomic::{AtomicU8, Ordering};

use crate::hv::memory::PAGE_SIZE_4K;
use crate::hv::vmcall::DispatchOutcome;
use crate::phys::HeapArena;
use spin::Mutex;

use super::super::{VmBootMode, vmx};

pub(super) const PROBE_CODE_VA: u64 = 0x0020_0000;
pub(super) const PROBE_TEB_VA: u64 = PROBE_CODE_VA + PAGE_SIZE_4K as u64;
pub(super) const PROBE_BACKING_BYTES: usize = PAGE_SIZE_4K * 2;
pub(super) const PROBE_GUEST_MARKER_OFFSET: usize = 0x100;
pub(super) const PROBE_HOST_MARKER_OFFSET: usize = 0x104;

const STEP_GUEST: u32 = 0xC357_3311;
const STEP_HOST_ACK: u32 = 0xA55A_C33C;
const STEP_RESUME: u32 = 0x52E5_0A32;
const STACK_MARKER: u32 = 0x51AC_C032;
const HOST_MEMORY_MARKER: u32 = 0xF5BA_5E32;
const STEP_FAILURE: u32 = 0xFA11_0032;

const STAGE_IDLE: u8 = 0;
const STAGE_ARMED: u8 = 1;
const STAGE_RESUMED: u8 = 2;
const STAGE_COMPLETE: u8 = 3;
const STAGE_FAILED: u8 = 4;

// 32-bit compatibility-mode code, assembled explicitly so the kernel build
// never needs a second Rust target or a runtime assembler:
//
//   mov eax, STEP_GUEST
//   mov ebx, STACK_MARKER
//   push ebx; pop ecx
//   mov fs:[PROBE_GUEST_MARKER_OFFSET], ecx
//   vmcall
//   cmp eax, STEP_HOST_ACK
//   jne failed
//   mov ecx, fs:[PROBE_HOST_MARKER_OFFSET]
//   mov eax, STEP_RESUME
//   vmcall
//   ud2
// failed:
//   mov eax, STEP_FAILURE
//   vmcall
//   ud2
pub(super) const PROBE_CODE: &[u8] = &[
    0xB8, 0x11, 0x33, 0x57, 0xC3, // mov eax, STEP_GUEST
    0xBB, 0x32, 0xC0, 0xAC, 0x51, // mov ebx, STACK_MARKER
    0x53, 0x59, // push ebx; pop ecx
    0x64, 0x89, 0x0D, 0x00, 0x01, 0x00, 0x00, // mov fs:[0x100], ecx
    0x0F, 0x01, 0xC1, // vmcall
    0x3D, 0x3C, 0xC3, 0x5A, 0xA5, // cmp eax, STEP_HOST_ACK
    0x75, 0x11, // jne failed
    0x64, 0x8B, 0x0D, 0x04, 0x01, 0x00, 0x00, // mov ecx, fs:[0x104]
    0xB8, 0x32, 0x0A, 0xE5, 0x52, // mov eax, STEP_RESUME
    0x0F, 0x01, 0xC1, // vmcall
    0x0F, 0x0B, // ud2: host must stop at the second VMCALL
    0xB8, 0x32, 0x00, 0x11, 0xFA, // mov eax, STEP_FAILURE
    0x0F, 0x01, 0xC1, // vmcall
    0x0F, 0x0B, // ud2
];

#[derive(Copy, Clone)]
pub(crate) struct GuestMapping {
    pub phys_start: u64,
    pub bytes: usize,
    pub code_va: u64,
    pub teb_va: u64,
}

static PROBE_BACKINGS: [Mutex<Option<HeapArena>>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(None) }; crate::allcaps::hv::VM_ID_LIMIT];
static PROBE_STAGES: [AtomicU8; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicU8::new(STAGE_IDLE) }; crate::allcaps::hv::VM_ID_LIMIT];

pub(crate) fn prepare_gate0(vm_id: u8) -> Result<(), &'static str> {
    let backing = PROBE_BACKINGS
        .get(usize::from(vm_id))
        .ok_or("unsupported wc3 probe VM id")?;
    let mut backing = backing.lock();
    if backing.is_none() {
        *backing = Some(
            crate::phys::reserve_heap_arena(PROBE_BACKING_BYTES, PAGE_SIZE_4K)
                .ok_or("wc3 probe backing allocation")?,
        );
    }
    let arena = backing.as_ref().ok_or("wc3 probe backing missing")?;
    unsafe {
        core::ptr::write_bytes(arena.virt_start as *mut u8, 0, arena.length);
        core::ptr::copy_nonoverlapping(
            PROBE_CODE.as_ptr(),
            arena.virt_start as *mut u8,
            PROBE_CODE.len(),
        );
        let teb = (arena.virt_start as *mut u8).add(PAGE_SIZE_4K);
        // The initial SEH chain sentinel makes FS:[0] useful to the next gate,
        // while the probe uses private offsets that do not invent a TEB ABI.
        core::ptr::write_unaligned(teb.cast::<u32>(), u32::MAX);
    }
    PROBE_STAGES[usize::from(vm_id)].store(STAGE_ARMED, Ordering::Release);
    super::trace::info(format_args!(
        "gate-0 armed vm={} entry=0x{:08X} fs_base=0x{:08X}",
        vm_id, PROBE_CODE_VA, PROBE_TEB_VA
    ));
    Ok(())
}

pub(crate) fn guest_mapping(vm_id: u8) -> Option<GuestMapping> {
    let arena = *PROBE_BACKINGS.get(usize::from(vm_id))?.lock();
    arena.map(|arena| GuestMapping {
        phys_start: arena.phys_start,
        bytes: PROBE_BACKING_BYTES,
        code_va: PROBE_CODE_VA,
        teb_va: PROBE_TEB_VA,
    })
}

pub(crate) fn purge_one_shot_state(vm_id: u8) -> bool {
    let Some(backing) = PROBE_BACKINGS.get(usize::from(vm_id)) else {
        return false;
    };
    let released = backing
        .lock()
        .take()
        .is_some_and(|arena| crate::phys::free_phys_range(arena.phys_start, arena.length));
    if let Some(stage) = PROBE_STAGES.get(usize::from(vm_id)) {
        stage.store(STAGE_IDLE, Ordering::Release);
    }
    released
}

pub(crate) const fn entry_for_mode(mode: VmBootMode, normal: u64) -> u64 {
    match mode {
        VmBootMode::Wc3Probe => PROBE_CODE_VA,
        _ => normal,
    }
}

pub(crate) const fn fs_base_for_mode(mode: VmBootMode) -> u64 {
    match mode {
        VmBootMode::Wc3Probe => PROBE_TEB_VA,
        _ => 0,
    }
}

fn teb_word(vm_id: u8, offset: usize) -> Option<*mut u32> {
    if offset.checked_add(core::mem::size_of::<u32>())? > PAGE_SIZE_4K {
        return None;
    }
    let arena = *PROBE_BACKINGS.get(usize::from(vm_id))?.lock();
    let arena = arena?;
    Some(unsafe {
        (arena.virt_start as *mut u8)
            .add(PAGE_SIZE_4K + offset)
            .cast::<u32>()
    })
}

fn fail(vm_id: u8, reason: &'static str, regs: vmx::GuestRegisters) -> DispatchOutcome {
    PROBE_STAGES[usize::from(vm_id)].store(STAGE_FAILED, Ordering::Release);
    super::trace::fail(format_args!(
        "vm={} reason={} eax=0x{:08X} ebx=0x{:08X} ecx=0x{:08X}",
        vm_id, reason, regs.rax as u32, regs.rbx as u32, regs.rcx as u32
    ));
    DispatchOutcome::Stop
}

pub(crate) fn handle_vmcall(vm_id: u8) -> DispatchOutcome {
    let mut regs = vmx::guest_registers();
    let stage = PROBE_STAGES[usize::from(vm_id)].load(Ordering::Acquire);
    match regs.rax as u32 {
        STEP_GUEST if stage == STAGE_ARMED => {
            let Some(marker) = teb_word(vm_id, PROBE_GUEST_MARKER_OFFSET) else {
                return fail(vm_id, "guest marker address unavailable", regs);
            };
            let marker = unsafe { core::ptr::read_volatile(marker) };
            if regs.rbx as u32 != STACK_MARKER
                || regs.rcx as u32 != STACK_MARKER
                || marker != STACK_MARKER
            {
                return fail(vm_id, "32-bit stack or FS write mismatch", regs);
            }
            let Some(host_marker) = teb_word(vm_id, PROBE_HOST_MARKER_OFFSET) else {
                return fail(vm_id, "host marker address unavailable", regs);
            };
            unsafe { core::ptr::write_volatile(host_marker, HOST_MEMORY_MARKER) };
            regs.rax = u64::from(STEP_HOST_ACK);
            vmx::set_guest_registers(regs);
            PROBE_STAGES[usize::from(vm_id)].store(STAGE_RESUMED, Ordering::Release);
            super::trace::info(format_args!(
                "gate-0 first VMCALL vm={} code=32 stack=ok fs-write=ok host-write=armed",
                vm_id
            ));
            DispatchOutcome::Resume
        }
        STEP_RESUME if stage == STAGE_RESUMED => {
            if regs.rcx as u32 != HOST_MEMORY_MARKER {
                return fail(vm_id, "resumed guest did not observe host memory write", regs);
            }
            PROBE_STAGES[usize::from(vm_id)].store(STAGE_COMPLETE, Ordering::Release);
            super::trace::info(format_args!(
                "gate-0 complete vm={} 32-bit-code=ok stack=ok vmcall=ok registers=ok memory=ok resume=ok fs=ok",
                vm_id
            ));
            DispatchOutcome::Stop
        }
        STEP_FAILURE => fail(vm_id, "guest rejected host register acknowledgement", regs),
        _ => fail(vm_id, "unexpected VMCALL or probe stage", regs),
    }
}
