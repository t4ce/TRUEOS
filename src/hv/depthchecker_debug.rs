//! Explicit debugger handoff for one diagnostic Blueprint's armed INT3 sites.
//! No guest exception is consumed unless the host debugger enabled this exact
//! probe and whitelisted its current RIP. All other exits keep crash handling.

use core::hint::black_box;
use core::sync::atomic::{AtomicU64, Ordering, compiler_fence};

use super::vmx::{self, LaunchResult};

pub const MAX_DEBUG_ADDRESSES: usize = 64;
pub const SNAPSHOT_MAGIC: u64 = 0x5041_4e53_4346_4344; // "DCFCSNAP" in memory.
pub const SNAPSHOT_SCHEMA_VERSION: u64 = 1;

// The debugger publishes addresses/count before ENABLE=1 while QEMU is
// stopped. Atomic cells keep the runtime reader well defined and are laid out
// as contiguous 64-bit words for raw GDB memory writes.
#[unsafe(no_mangle)]
pub static DEPTHCHECKER_DEBUG_ENABLE: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
pub static DEPTHCHECKER_DEBUG_ADDRESS_COUNT: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
pub static DEPTHCHECKER_DEBUG_ADDRESSES: [AtomicU64; MAX_DEBUG_ADDRESSES] =
    [const { AtomicU64::new(0) }; MAX_DEBUG_ADDRESSES];
#[unsafe(no_mangle)]
pub static DEPTHCHECKER_DEBUG_HIT_COUNT: AtomicU64 = AtomicU64::new(0);

/// Versioned host-readable state from the actual L2 exit, never the host GDB
/// register view. GPR order is vmx::GuestRegisters: rax,rbx,rcx,rdx,rsi,rdi,rbp,
/// r8,r9,r10,r11,r12,r13,r14,r15. The XSAVE image is the saved guest image.
#[repr(C)]
pub struct DepthcheckerDebugSnapshot {
    pub magic: u64,
    pub schema_version: u64,
    pub snapshot_bytes: u64,
    pub sequence: u64,
    pub vm_id: u64,
    pub cpu_slot: u64,
    pub guest_rip: u64,
    pub guest_rsp: u64,
    pub guest_rflags: u64,
    pub guest_cr3: u64,
    pub guest_fs_base: u64,
    pub guest_gs_base: u64,
    pub guest_cr0: u64,
    pub guest_cr2: u64,
    pub guest_cr4: u64,
    pub guest_efer: u64,
    pub guest_cs: u64,
    pub guest_ss: u64,
    pub eptp: u64,
    pub hhdm_offset: u64,
    pub guest_stack_base: u64,
    pub stack_host_base: u64,
    pub stack_bytes: u64,
    pub guest_gpr: [u64; 15],
    pub guest_xsave_mask: u64,
    pub guest_xsave_size: u64,
    pub guest_xsave: [u8; vmx::VMX_EXTENDED_STATE_BYTES],
}

/// Host software breakpoint target. While stopped here, the debugger reads
/// this snapshot, restores the original guest opcode, and updates its armed
/// sites. There is no guest instruction skip or replacement register state.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn trueos_depthchecker_debug_stop(snapshot: *const DepthcheckerDebugSnapshot) {
    black_box(snapshot);
    compiler_fence(Ordering::SeqCst);
}

fn armed_resume_rip(enabled: u64, archive: &str, interruption: u32, rip: u64) -> Option<u64> {
    // INT3 is an intercepted software exception (VM-exit interruption type 6).
    // A vector number alone must not admit an external/software interrupt.
    if enabled != 1
        || archive != "depthchecker_factchecker.bp"
        || interruption & (1 << 31) == 0
        || interruption & 0xff != 3
        || (interruption >> 8) & 7 != 6
        || rip == 0
    {
        return None;
    }
    let count = DEPTHCHECKER_DEBUG_ADDRESS_COUNT.load(Ordering::Acquire);
    if count == 0 || count > MAX_DEBUG_ADDRESSES as u64 {
        return None;
    }
    DEPTHCHECKER_DEBUG_ADDRESSES[..count as usize]
        .iter()
        .any(|address| address.load(Ordering::Acquire) == rip)
        .then_some(rip)
}

/// Translate a bounded raw stack read using the snapshot's retained backing.
/// Callers must use CR3/EPT translation for addresses outside this low stack.

/// Return true only for the explicit debugger-owned exception. All locks used
/// to resolve archive/stack identity have been released before the host hook.
pub(super) fn try_handoff(vm_id: u8, exit: &LaunchResult) -> Result<bool, &'static str> {
    let enabled = DEPTHCHECKER_DEBUG_ENABLE.load(Ordering::Acquire);
    if enabled != 1 {
        return Ok(false);
    }
    let Some(interruption) = vmx::vmread(vmx::VMCS_VMEXIT_INTERRUPTION_INFO) else {
        return Ok(false);
    };
    let Some(archive) = super::app_vm_archive(vm_id) else {
        return Ok(false);
    };
    let rip = vmx::vmread(vmx::VMCS_GUEST_RIP).ok_or("depthchecker guest RIP unavailable")?;
    let Some(resume_rip) = armed_resume_rip(enabled, &archive, interruption as u32, rip) else {
        return Ok(false);
    };
    let mut snapshot = capture_snapshot(vm_id, exit, rip)?;
    snapshot.sequence = DEPTHCHECKER_DEBUG_HIT_COUNT.fetch_add(1, Ordering::AcqRel) + 1;
    trueos_depthchecker_debug_stop(&snapshot);
    // VMX intercepted the INT3 before advancing RIP. The host debugger has
    // restored its byte; resume that same instruction, including function
    // prologues. Advancing one byte would corrupt the captured execution.
    vmx::vmwrite(vmx::VMCS_GUEST_RIP, resume_rip)?;
    Ok(true)
}

fn capture_snapshot(
    vm_id: u8,
    exit: &LaunchResult,
    rip: u64,
) -> Result<DepthcheckerDebugSnapshot, &'static str> {
    let regs = vmx::guest_registers();
    let (guest_xsave_mask, guest_xsave) = vmx::guest_extended_state_snapshot(vm_id)?;
    let stack = super::memory::guest_stack_slice_for_vm(vm_id);
    Ok(DepthcheckerDebugSnapshot {
        magic: SNAPSHOT_MAGIC,
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_bytes: core::mem::size_of::<DepthcheckerDebugSnapshot>() as u64,
        sequence: 0,
        vm_id: u64::from(vm_id),
        cpu_slot: crate::percpu::current_slot() as u64,
        guest_rip: rip,
        guest_rsp: vmx::vmread(vmx::VMCS_GUEST_RSP).ok_or("depthchecker guest RSP unavailable")?,
        guest_rflags: vmx::vmread(vmx::VMCS_GUEST_RFLAGS)
            .ok_or("depthchecker guest RFLAGS unavailable")?,
        guest_cr3: vmx::vmread(vmx::VMCS_GUEST_CR3).ok_or("depthchecker guest CR3 unavailable")?,
        guest_fs_base: vmx::vmread(vmx::VMCS_GUEST_FS_BASE).unwrap_or(0),
        guest_gs_base: vmx::vmread(vmx::VMCS_GUEST_GS_BASE).unwrap_or(0),
        guest_cr0: vmx::vmread(vmx::VMCS_GUEST_CR0).unwrap_or(0),
        guest_cr2: exit.guest_cr2,
        guest_cr4: vmx::vmread(vmx::VMCS_GUEST_CR4).unwrap_or(0),
        guest_efer: vmx::vmread(vmx::VMCS_GUEST_IA32_EFER).unwrap_or(0),
        guest_cs: vmx::vmread(vmx::VMCS_GUEST_CS_SELECTOR).unwrap_or(0),
        guest_ss: vmx::vmread(vmx::VMCS_GUEST_SS_SELECTOR).unwrap_or(0),
        eptp: vmx::vmread(vmx::VMCS_CTRL_EPT_POINTER).ok_or("depthchecker EPTP unavailable")?,
        hhdm_offset: crate::limine::hhdm_offset().unwrap_or(0),
        guest_stack_base: super::memory::GUEST_STACK_VA_BASE,
        stack_host_base: stack.map_or(0, |bytes| bytes.as_ptr() as u64),
        stack_bytes: stack.map_or(0, |bytes| bytes.len() as u64),
        guest_gpr: [
            regs.rax, regs.rbx, regs.rcx, regs.rdx, regs.rsi, regs.rdi, regs.rbp, regs.r8, regs.r9,
            regs.r10, regs.r11, regs.r12, regs.r13, regs.r14, regs.r15,
        ],
        guest_xsave_mask,
        guest_xsave_size: vmx::VMX_EXTENDED_STATE_BYTES as u64,
        guest_xsave,
    })
}
