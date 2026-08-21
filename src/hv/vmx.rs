use x86_64::registers::model_specific::Msr;

use crate::hv::{hvlogf, hvtracef};

// MSR indices
pub const IA32_FEATURE_CONTROL: u32 = 0x3A;
pub const IA32_VMX_BASIC: u32 = 0x480;
pub const IA32_VMX_TRUE_PINBASED_CTLS: u32 = 0x48D;
pub const IA32_VMX_TRUE_PROCBASED_CTLS: u32 = 0x48E;
pub const IA32_VMX_TRUE_EXIT_CTLS: u32 = 0x48F;
pub const IA32_VMX_TRUE_ENTRY_CTLS: u32 = 0x490;
pub const IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;
pub const IA32_VMX_MISC: u32 = 0x485;
pub const IA32_VMX_CR0_FIXED0: u32 = 0x486;
pub const IA32_VMX_CR0_FIXED1: u32 = 0x487;
pub const IA32_VMX_CR4_FIXED0: u32 = 0x488;
pub const IA32_VMX_CR4_FIXED1: u32 = 0x489;
pub const IA32_SYSENTER_CS: u32 = 0x174;
pub const IA32_SYSENTER_ESP: u32 = 0x175;
pub const IA32_SYSENTER_EIP: u32 = 0x176;
pub const IA32_DEBUGCTL: u32 = 0x1D9;
pub const IA32_PAT: u32 = 0x277;
pub const IA32_PERF_GLOBAL_CTRL: u32 = 0x38F;
pub const IA32_FS_BASE: u32 = 0xC000_0100;
pub const IA32_GS_BASE: u32 = 0xC000_0101;
pub const IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;
pub const IA32_EFER: u32 = 0xC000_0080;

pub const IA32_FEATURE_CONTROL_LOCK: u64 = 1 << 0;
pub const IA32_FEATURE_CONTROL_VMX_OUTSIDE_SMX: u64 = 1 << 2;

// VMCS field indices
pub const VMCS_CTRL_PIN_BASED: u64 = 0x4000;
pub const VMCS_CTRL_CPU_BASED: u64 = 0x4002;
pub const VMCS_CTRL_EXCEPTION_BITMAP: u64 = 0x4004;
pub const VMCS_CTRL_EXIT: u64 = 0x400C;
pub const VMCS_CTRL_ENTRY: u64 = 0x4012;
pub const VMCS_CTRL_SECONDARY: u64 = 0x401E;

pub const VMCS_CTRL_EPT_POINTER: u64 = 0x201A;
pub const VMCS_CTRL_VMCS_LINK_POINTER: u64 = 0x2800;

pub const VMCS_TSC_OFFSET: u64 = 0x2010;
pub const VMCS_CTRL_VMFUNC_CONTROLS: u64 = 0x2018;
pub const VMCS_CTRL_EPTP_LIST_ADDR: u64 = 0x2024;

pub const VMCS_HOST_CR0: u64 = 0x6C00;
pub const VMCS_HOST_CR3: u64 = 0x6C02;
pub const VMCS_HOST_CR4: u64 = 0x6C04;
pub const VMCS_HOST_FS_BASE: u64 = 0x6C06;
pub const VMCS_HOST_GS_BASE: u64 = 0x6C08;
pub const VMCS_HOST_TR_BASE: u64 = 0x6C0A;
pub const VMCS_HOST_GDTR_BASE: u64 = 0x6C0C;
pub const VMCS_HOST_IDTR_BASE: u64 = 0x6C0E;
pub const VMCS_HOST_SYSENTER_ESP: u64 = 0x6C10;
pub const VMCS_HOST_SYSENTER_EIP: u64 = 0x6C12;
pub const VMCS_HOST_RSP: u64 = 0x6C14;
pub const VMCS_HOST_RIP: u64 = 0x6C16;
pub const VMCS_HOST_SYSENTER_CS: u64 = 0x4C00;
pub const VMCS_HOST_CS_SELECTOR: u64 = 0x0C02;
pub const VMCS_HOST_SS_SELECTOR: u64 = 0x0C04;
pub const VMCS_HOST_DS_SELECTOR: u64 = 0x0C06;
pub const VMCS_HOST_ES_SELECTOR: u64 = 0x0C00;
pub const VMCS_HOST_FS_SELECTOR: u64 = 0x0C08;
pub const VMCS_HOST_GS_SELECTOR: u64 = 0x0C0A;
pub const VMCS_HOST_TR_SELECTOR: u64 = 0x0C0C;

pub const VMCS_GUEST_CR0: u64 = 0x6800;
pub const VMCS_GUEST_CR3: u64 = 0x6802;
pub const VMCS_GUEST_CR4: u64 = 0x6804;
pub const VMCS_GUEST_ES_SELECTOR: u64 = 0x0800;
pub const VMCS_GUEST_CS_SELECTOR: u64 = 0x0802;
pub const VMCS_GUEST_SS_SELECTOR: u64 = 0x0804;
pub const VMCS_GUEST_DS_SELECTOR: u64 = 0x0806;
pub const VMCS_GUEST_FS_SELECTOR: u64 = 0x0808;
pub const VMCS_GUEST_GS_SELECTOR: u64 = 0x080A;
pub const VMCS_GUEST_LDTR_SELECTOR: u64 = 0x080C;
pub const VMCS_GUEST_TR_SELECTOR: u64 = 0x080E;
pub const VMCS_GUEST_ES_LIMIT: u64 = 0x4800;
pub const VMCS_GUEST_CS_LIMIT: u64 = 0x4802;
pub const VMCS_GUEST_SS_LIMIT: u64 = 0x4804;
pub const VMCS_GUEST_DS_LIMIT: u64 = 0x4806;
pub const VMCS_GUEST_FS_LIMIT: u64 = 0x4808;
pub const VMCS_GUEST_GS_LIMIT: u64 = 0x480A;
pub const VMCS_GUEST_LDTR_LIMIT: u64 = 0x480C;
pub const VMCS_GUEST_TR_LIMIT: u64 = 0x480E;
pub const VMCS_GUEST_GDTR_LIMIT: u64 = 0x4810;
pub const VMCS_GUEST_IDTR_LIMIT: u64 = 0x4812;
pub const VMCS_GUEST_ES_AR: u64 = 0x4814;
pub const VMCS_GUEST_CS_AR: u64 = 0x4816;
pub const VMCS_GUEST_SS_AR: u64 = 0x4818;
pub const VMCS_GUEST_DS_AR: u64 = 0x481A;
pub const VMCS_GUEST_FS_AR: u64 = 0x481C;
pub const VMCS_GUEST_GS_AR: u64 = 0x481E;
pub const VMCS_GUEST_LDTR_AR: u64 = 0x4820;
pub const VMCS_GUEST_TR_AR: u64 = 0x4822;
pub const VMCS_GUEST_INTERRUPTIBILITY: u64 = 0x4824;
pub const VMCS_GUEST_ACTIVITY_STATE: u64 = 0x4826;
pub const VMCS_GUEST_SYSENTER_CS: u64 = 0x482A;
pub const VMCS_GUEST_VMCS_PREEMPT_TIMER: u64 = 0x482E;
pub const VMCS_GUEST_ES_BASE: u64 = 0x6806;
pub const VMCS_GUEST_CS_BASE: u64 = 0x6808;
pub const VMCS_GUEST_SS_BASE: u64 = 0x680A;
pub const VMCS_GUEST_DS_BASE: u64 = 0x680C;
pub const VMCS_GUEST_FS_BASE: u64 = 0x680E;
pub const VMCS_GUEST_GS_BASE: u64 = 0x6810;
pub const VMCS_GUEST_LDTR_BASE: u64 = 0x6812;
pub const VMCS_GUEST_TR_BASE: u64 = 0x6814;
pub const VMCS_GUEST_GDTR_BASE: u64 = 0x6816;
pub const VMCS_GUEST_IDTR_BASE: u64 = 0x6818;
pub const VMCS_GUEST_DR7: u64 = 0x681A;
pub const VMCS_GUEST_RSP: u64 = 0x681C;
pub const VMCS_GUEST_RIP: u64 = 0x681E;
pub const VMCS_GUEST_RFLAGS: u64 = 0x6820;
pub const VMCS_GUEST_LINEAR_ADDRESS: u64 = 0x640A;
pub const VMCS_GUEST_PHYSICAL_ADDRESS: u64 = 0x2400;
pub const VMCS_GUEST_PENDING_DBG: u64 = 0x6822;
pub const VMCS_GUEST_SYSENTER_ESP: u64 = 0x6824;
pub const VMCS_GUEST_SYSENTER_EIP: u64 = 0x6826;
pub const VMCS_GUEST_IA32_EFER: u64 = 0x2806;
pub const VMCS_GUEST_IA32_DEBUGCTL: u64 = 0x2802;
pub const VMCS_GUEST_IA32_PAT: u64 = 0x2804;
pub const VMCS_GUEST_IA32_PERF_GLOBAL_CTRL: u64 = 0x2808;

pub const VMCS_HOST_IA32_PAT: u64 = 0x2C00;
pub const VMCS_HOST_IA32_EFER: u64 = 0x2C02;
pub const VMCS_HOST_IA32_PERF_GLOBAL_CTRL: u64 = 0x2C04;

pub const VMCS_EXIT_REASON: u64 = 0x4402;
pub const VMCS_VMEXIT_INTERRUPTION_INFO: u64 = 0x4404;
pub const VMCS_VMEXIT_INTERRUPTION_ERROR_CODE: u64 = 0x4406;
pub const VMCS_VMEXIT_INSTRUCTION_LEN: u64 = 0x440C;
pub const VMCS_EXIT_QUALIFICATION: u64 = 0x6400;
pub const VMCS_VM_INSTRUCTION_ERROR: u64 = 0x4400;
pub const VMCS_VMEXIT_GUEST_RIP: u64 = 0x681E;

pub const PROC_BASED_HLT_EXITING: u64 = 1 << 7;
pub const PIN_BASED_EXTERNAL_INTERRUPT_EXITING: u64 = 1 << 0;
pub const PIN_BASED_VMX_PREEMPTION_TIMER: u64 = 1 << 6;
pub const PROC_BASED_PAUSE_EXITING: u64 = 1 << 30;
pub const PROC_BASED_ACTIVATE_SECONDARY: u64 = 1 << 31;
pub const PROC2_BASED_ENABLE_EPT: u64 = 1 << 1;
pub const PROC_BASED_USE_TSC_OFFSETTING: u64 = 1 << 3;
pub const PROC2_BASED_ENABLE_VMFUNC: u64 = 1 << 13;
pub const VMFUNC_EPTP_SWITCHING: u64 = 1 << 0;
pub const EXIT_CTL_HOST_ADDR_SPACE_SIZE: u64 = 1 << 9;
pub const EXIT_CTL_ACKNOWLEDGE_INTERRUPT_ON_EXIT: u64 = 1 << 15;
pub const ENTRY_CTL_IA32E_MODE_GUEST: u64 = 1 << 9;
pub const RFLAGS_RESERVED_BIT1: u64 = 1 << 1;
pub const RFLAGS_IF: u64 = 1 << 9;
pub const EXCEPTION_BITMAP_ALL: u64 = 0xFFFF_FFFF;
pub const VMEXIT_REASON_EXTERNAL_INTERRUPT: u64 = 0x01;
pub const VMEXIT_REASON_VMCALL: u64 = 0x12;
pub const VMEXIT_REASON_PAUSE: u64 = 0x28;
pub const VMEXIT_REASON_VMX_PREEMPTION_TIMER: u64 = 0x34;

const VMEXIT_INTERRUPTION_INFO_VALID: u32 = 1 << 31;
const VMEXIT_INTERRUPTION_INFO_ERROR_CODE_VALID: u32 = 1 << 11;
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
const VMEXIT_INTERRUPTION_INFO_NMI_UNBLOCKING: u32 = 1 << 12;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VmExitInterruptionInfo {
    raw: u32,
}

impl VmExitInterruptionInfo {
    #[inline]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        if (raw & VMEXIT_INTERRUPTION_INFO_VALID) != 0 {
            Some(Self { raw })
        } else {
            None
        }
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.raw
    }

    #[inline]
    pub const fn vector(self) -> u8 {
        (self.raw & 0xFF) as u8
    }

    #[inline]
    pub const fn interruption_type(self) -> u8 {
        ((self.raw >> 8) & 0x7) as u8
    }

    #[inline]
    pub const fn error_code_valid(self) -> bool {
        (self.raw & VMEXIT_INTERRUPTION_INFO_ERROR_CODE_VALID) != 0
    }

    #[inline]
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn nmi_unblocking_due_to_iret(self) -> bool {
        (self.raw & VMEXIT_INTERRUPTION_INFO_NMI_UNBLOCKING) != 0
    }

    #[inline]
    pub const fn is_external_interrupt(self) -> bool {
        self.interruption_type() == 0
    }
}

pub const HV_GDT_SEL_CODE: u16 = 0x08;
pub const HV_GDT_SEL_DATA: u16 = 0x10;
pub const HV_GDT_SEL_TSS: u16 = 0x18;
pub const HV_GDT_DESC_CODE64: u64 = 0x00AF_9B00_0000_FFFF;
pub const HV_GDT_DESC_DATA64: u64 = 0x00AF_9300_0000_FFFF;

#[derive(Copy, Clone)]
#[repr(C, align(4096))]
pub struct VmxPage(pub [u8; 4096]);

#[derive(Copy, Clone, Default)]
pub struct LaunchResult {
    pub entered: u8,
    pub launch_failed: u8,
    pub _pad: [u8; 6],
    pub exit_reason: u64,
    pub exit_qualification: u64,
    pub guest_rip: u64,
    pub instr_err: u64,
}

#[derive(Copy, Clone, Default)]
#[repr(C)]
pub struct GuestRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

pub struct HvSyntheticHostState {
    pub gdt_base: u64,
    pub tr_base: u64,
    pub tr_sel: u16,
    pub cs_sel: u16,
    pub data_sel: u16,
}

const VMX_SCRATCH_SLOTS: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;
const VMX_GUEST_SLOTS: usize = crate::allcaps::hv::VM_ID_LIMIT;
pub const VMX_EXTENDED_STATE_BYTES: usize = 832;

#[derive(Copy, Clone)]
#[repr(C, align(64))]
struct VmxExtendedState([u8; VMX_EXTENDED_STATE_BYTES]);

const EMPTY_EXTENDED_STATE: VmxExtendedState = VmxExtendedState([0; VMX_EXTENDED_STATE_BYTES]);

const EMPTY_LAUNCH_RESULT: LaunchResult = LaunchResult {
    entered: 0,
    launch_failed: 0,
    _pad: [0; 6],
    exit_reason: 0,
    exit_qualification: 0,
    guest_rip: 0,
    instr_err: 0,
};

const EMPTY_GUEST_REGISTERS: GuestRegisters = GuestRegisters {
    rax: 0,
    rbx: 0,
    rcx: 0,
    rdx: 0,
    rsi: 0,
    rdi: 0,
    rbp: 0,
    r8: 0,
    r9: 0,
    r10: 0,
    r11: 0,
    r12: 0,
    r13: 0,
    r14: 0,
    r15: 0,
};

static mut VMX_WRAPPER_RESULTS: [LaunchResult; VMX_SCRATCH_SLOTS] =
    [EMPTY_LAUNCH_RESULT; VMX_SCRATCH_SLOTS];
static mut VMX_GUEST_REGS_BY_SLOT: [GuestRegisters; VMX_SCRATCH_SLOTS] =
    [EMPTY_GUEST_REGISTERS; VMX_SCRATCH_SLOTS];
// Host state belongs to the AP currently executing the VMX wrapper. Guest
// state belongs to the VM and therefore survives a yield or lane migration.
static mut VMX_HOST_EXTENDED_STATE_BY_SLOT: [VmxExtendedState; VMX_SCRATCH_SLOTS] =
    [EMPTY_EXTENDED_STATE; VMX_SCRATCH_SLOTS];
static mut VMX_GUEST_EXTENDED_STATE_BY_VM: [VmxExtendedState; VMX_GUEST_SLOTS] =
    [EMPTY_EXTENDED_STATE; VMX_GUEST_SLOTS];

fn current_scratch_slot() -> usize {
    let slot = crate::percpu::current_slot();
    if slot < VMX_SCRATCH_SLOTS { slot } else { 0 }
}

fn wrapper_result_ptr() -> *mut LaunchResult {
    unsafe { core::ptr::addr_of_mut!(VMX_WRAPPER_RESULTS[current_scratch_slot()]) }
}

fn guest_regs_ptr() -> *mut GuestRegisters {
    unsafe { core::ptr::addr_of_mut!(VMX_GUEST_REGS_BY_SLOT[current_scratch_slot()]) }
}

fn host_extended_state_ptr() -> *mut VmxExtendedState {
    unsafe { core::ptr::addr_of_mut!(VMX_HOST_EXTENDED_STATE_BY_SLOT[current_scratch_slot()]) }
}

fn guest_extended_state_ptr(vm_id: u8) -> Option<*mut VmxExtendedState> {
    let index = usize::from(vm_id);
    if index >= VMX_GUEST_SLOTS {
        return None;
    }
    Some(unsafe { core::ptr::addr_of_mut!(VMX_GUEST_EXTENDED_STATE_BY_VM[index]) })
}

pub fn reset_guest_extended_state(vm_id: u8) -> Result<(), &'static str> {
    let state = guest_extended_state_ptr(vm_id).ok_or("unsupported VM extended-state owner")?;
    unsafe {
        state.write(EMPTY_EXTENDED_STATE);
        // Seed the architectural x87 control word and MXCSR defaults in the
        // legacy region used by both FXSAVE and XSAVE. Standard XRSTOR loads
        // MXCSR whenever SSE or AVX is requested even when XSTATE_BV is zero,
        // so an all-zero XSAVE image would incorrectly unmask every SIMD
        // floating-point exception. The remaining zero state and header
        // describe clean x87/MMX/XMM/YMM state.
        let bytes = &mut (*state).0;
        bytes[0..2].copy_from_slice(&0x037Fu16.to_le_bytes());
        bytes[24..28].copy_from_slice(&0x1F80u32.to_le_bytes());
    }
    Ok(())
}

pub fn guest_extended_state_snapshot(
    vm_id: u8,
) -> Result<(u64, [u8; VMX_EXTENDED_STATE_BYTES]), &'static str> {
    let state = guest_extended_state_ptr(vm_id).ok_or("unsupported VM extended-state owner")?;
    Ok((crate::cpu::vmx_xsave_mask(), unsafe { (*state).0 }))
}

pub fn restore_guest_extended_state(
    vm_id: u8,
    saved_mask: u64,
    bytes: &[u8; VMX_EXTENDED_STATE_BYTES],
) -> Result<(), &'static str> {
    if saved_mask != crate::cpu::vmx_xsave_mask() {
        return Err("snapshot extended-state mask is incompatible with this CPU");
    }
    let state = guest_extended_state_ptr(vm_id).ok_or("unsupported VM extended-state owner")?;
    unsafe {
        (*state).0.copy_from_slice(bytes);
    }
    Ok(())
}

pub fn reset_guest_registers() {
    unsafe {
        guest_regs_ptr().write(EMPTY_GUEST_REGISTERS);
    }
}

pub fn guest_registers() -> GuestRegisters {
    unsafe { guest_regs_ptr().read() }
}

pub fn set_guest_registers(regs: GuestRegisters) {
    unsafe {
        guest_regs_ptr().write(regs);
    }
}

pub fn vmwrite(field: u64, val: u64) -> Result<(), &'static str> {
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmwrite {field}, {val}",
            "setna {fail}",
            val = in(reg) val,
            field = in(reg) field,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        if fail == 0 {
            Ok(())
        } else {
            let err = vmread(VMCS_VM_INSTRUCTION_ERROR).unwrap_or(!0u64);
            hvlogf(format_args!(
                "hv: vm{} reporting: vmwrite failed field=0x{:X} val=0x{:016X} instr_err={} rip=0x{:016X}",
                crate::hv::current_vm_id().unwrap_or(0),
                field,
                val,
                err,
                current_rip()
            ));
            Err("vmwrite")
        }
    }
}

pub fn vmread(field: u64) -> Option<u64> {
    unsafe {
        let mut out: u64 = 0;
        let mut fail: u8;
        core::arch::asm!(
            "vmread {out}, {field}",
            "setna {fail}",
            field = in(reg) field,
            out = lateout(reg) out,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        if fail == 0 { Some(out) } else { None }
    }
}

pub fn vmxon(pa: u64) -> bool {
    let pa_ptr = pa;
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmxon [{pa}]",
            "setna {fail}",
            pa = in(reg) &pa_ptr,
            fail = lateout(reg_byte) fail,
            options(nostack),
        );
        fail == 0
    }
}

/// Leave VMX root operation on the current logical processor.
pub fn vmxoff() -> bool {
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmxoff",
            "setna {fail}",
            fail = lateout(reg_byte) fail,
            options(nostack),
        );
        fail == 0
    }
}

pub fn vmclear(pa: u64) -> bool {
    let pa_ptr = pa;
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmclear [{pa}]",
            "setna {fail}",
            pa = in(reg) &pa_ptr,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        fail == 0
    }
}

pub fn vmptrld(pa: u64) -> bool {
    let pa_ptr = pa;
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmptrld [{pa}]",
            "setna {fail}",
            pa = in(reg) &pa_ptr,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        fail == 0
    }
}

pub fn adjust_vmx_ctrl(msr: u32, desired: u64) -> u64 {
    let caps = unsafe { Msr::new(msr).read() };
    let must_be_1 = caps & 0xFFFF_FFFF;
    let may_be_1 = (caps >> 32) & 0xFFFF_FFFF;
    ((desired & 0xFFFF_FFFF) | must_be_1) & may_be_1
}

pub fn decode_vmexit_int_type(kind: u64) -> &'static str {
    match kind {
        0 => "ext-int",
        2 => "nmi",
        3 => "hw-exc",
        4 => "sw-int",
        5 => "priv-sw-exc",
        6 => "sw-exc",
        7 => "other",
        _ => "unknown",
    }
}

pub fn decode_exception_vector(vector: u8) -> &'static str {
    match vector {
        0 => "#DE Divide Error",
        1 => "#DB Debug",
        2 => "NMI",
        3 => "#BP Breakpoint",
        4 => "#OF Overflow",
        5 => "#BR BOUND Range Exceeded",
        6 => "#UD Invalid Opcode",
        7 => "#NM Device Not Available",
        8 => "#DF Double Fault",
        9 => "Coprocessor Segment Overrun",
        10 => "#TS Invalid TSS",
        11 => "#NP Segment Not Present",
        12 => "#SS Stack Fault",
        13 => "#GP General Protection",
        14 => "#PF Page Fault",
        16 => "#MF x87 Floating-Point",
        17 => "#AC Alignment Check",
        18 => "#MC Machine Check",
        19 => "#XM SIMD Floating-Point",
        20 => "#VE Virtualization",
        21 => "#CP Control Protection",
        28 => "#HV Hypervisor Injection",
        29 => "#VC VMM Communication",
        30 => "#SX Security",
        _ => "unknown-exception",
    }
}

#[inline]
pub fn read_vmexit_interruption_info() -> Option<VmExitInterruptionInfo> {
    VmExitInterruptionInfo::from_raw(vmread(VMCS_VMEXIT_INTERRUPTION_INFO)? as u32)
}

pub fn log_vmexit_interrupt_info(label: &str) {
    let Some(info) = read_vmexit_interruption_info() else {
        return;
    };

    let vector = info.vector();
    let kind = info.interruption_type() as u64;
    let err_valid = info.error_code_valid();
    let err = if err_valid {
        vmread(VMCS_VMEXIT_INTERRUPTION_ERROR_CODE).unwrap_or(0)
    } else {
        0
    };
    let vector_name = if kind == 3 || kind == 5 || kind == 6 {
        decode_exception_vector(vector)
    } else {
        ""
    };

    hvtracef(format_args!(
        "hv: vm{} reporting: {} vmexit intr vector={} name={} type={}({}) err_valid={} err=0x{:X} intr_info=0x{:08X}",
        crate::hv::current_vm_id().unwrap_or(0),
        label,
        vector,
        vector_name,
        kind,
        decode_vmexit_int_type(kind),
        err_valid as u8,
        err,
        info.raw()
    ));

    let guest_rsp = vmread(VMCS_GUEST_RSP).unwrap_or(0);
    let guest_linear = vmread(VMCS_GUEST_LINEAR_ADDRESS).unwrap_or(0);
    let guest_physical = vmread(VMCS_GUEST_PHYSICAL_ADDRESS).unwrap_or(0);
    hvtracef(format_args!(
        "hv: vm{} reporting: {} vmexit addr guest_linear=0x{:016X} guest_physical=0x{:016X} guest_rsp=0x{:016X}",
        crate::hv::current_vm_id().unwrap_or(0),
        label,
        guest_linear,
        guest_physical,
        guest_rsp,
    ));
}

pub fn is_canonical(addr: u64) -> bool {
    let high = addr >> 48;
    high == 0 || high == 0xFFFF
}

pub fn current_rip() -> u64 {
    let rip: u64;
    unsafe {
        core::arch::asm!(
            "lea {}, [rip + 0]",
            out(reg) rip,
            options(nomem, nostack, preserves_flags),
        );
    }
    rip
}

pub fn read_tr_selector() -> u16 {
    let mut tr: u16 = 0;
    unsafe {
        core::arch::asm!(
            "str {0:x}",
            out(reg) tr,
            options(nostack, preserves_flags),
        );
    }
    tr
}

pub fn load_tr_selector(sel: u16) {
    unsafe {
        core::arch::asm!(
            "ltr {sel:x}",
            sel = in(reg) sel,
            options(nostack),
        );
    }
}

pub fn find_tss_selector(gdt_base: u64, gdt_limit: u16) -> Option<(u16, u8)> {
    let bytes = (gdt_limit as usize).checked_add(1)?;
    let entries = bytes / 8;
    if entries < 2 {
        return None;
    }

    for i in 1..entries {
        let ptr = (gdt_base as usize).checked_add(i.checked_mul(8)?)? as *const u64;
        let low = unsafe { core::ptr::read_unaligned(ptr) };
        let present = ((low >> 47) & 1) != 0;
        let system = ((low >> 44) & 1) == 0;
        let ty = ((low >> 40) & 0xF) as u8;
        if present && system && (ty == 0x9 || ty == 0xB) {
            return Some(((i as u16) << 3, ty));
        }
    }
    None
}

pub fn tss_base_from_gdt(gdt_base: u64, tr_sel: u16) -> Option<u64> {
    let index = (tr_sel as usize) & !0x7;
    let ptr = (gdt_base as usize).checked_add(index)? as *const u8;
    let low = unsafe { core::ptr::read_unaligned(ptr as *const u64) };
    let high = unsafe { core::ptr::read_unaligned(ptr.add(8) as *const u64) };
    let base_low = (low >> 16) & 0xFF_FFFF;
    let base_mid2 = (low >> 56) & 0xFF;
    let base_high = high & 0xFFFF_FFFF;
    Some(base_low | (base_mid2 << 24) | (base_high << 32))
}

pub fn synthesize_host_gdt_tss() -> HvSyntheticHostState {
    let slot = current_scratch_slot();
    let gdt = unsafe { core::ptr::addr_of_mut!(super::HV_HOST_GDTS[slot]) };
    let tss = unsafe { core::ptr::addr_of_mut!(super::HV_HOST_TSSS[slot]) };
    let tss_base = tss as u64;
    let tss_limit = (core::mem::size_of::<[u8; 104]>() as u64) - 1;

    let tss_low = (tss_limit & 0xFFFF)
        | ((tss_base & 0xFFFF) << 16)
        | (((tss_base >> 16) & 0xFF) << 32)
        | (0xBu64 << 40)
        | (1u64 << 47)
        | (((tss_limit >> 16) & 0xF) << 48)
        | (((tss_base >> 24) & 0xFF) << 56);
    let tss_high = (tss_base >> 32) & 0xFFFF_FFFF;

    unsafe {
        (*gdt)[0] = 0;
        (*gdt)[1] = HV_GDT_DESC_CODE64;
        (*gdt)[2] = HV_GDT_DESC_DATA64;
        (*gdt)[3] = tss_low;
        (*gdt)[4] = tss_high;
    }

    HvSyntheticHostState {
        gdt_base: gdt as u64,
        tr_base: tss_base,
        tr_sel: HV_GDT_SEL_TSS,
        cs_sel: HV_GDT_SEL_CODE,
        data_sel: HV_GDT_SEL_DATA,
    }
}

pub fn vmlaunch_once_wrapper(vm_id: u8, out: &mut LaunchResult) {
    unsafe {
        let result_ptr = wrapper_result_ptr();
        let guest_regs_ptr = guest_regs_ptr();
        let host_extended_state_ptr = host_extended_state_ptr();
        let guest_extended_state_ptr =
            guest_extended_state_ptr(vm_id).expect("unsupported VM extended-state owner");
        let extended_state_mask = crate::cpu::vmx_xsave_mask();
        result_ptr.write(EMPTY_LAUNCH_RESULT);
        core::arch::asm!(
            "push rbx",
            "push rbp",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "push {guest_regs_base}",
            "push {result_base}",
            "push {host_extended_state_base}",
            "push {guest_extended_state_base}",
            "push {extended_state_mask}",
            "mov rax, rsp",
            "mov rcx, {host_rsp_field}",
            "vmwrite rcx, rax",
            "lea rax, [rip + 2f]",
            "mov rcx, {host_rip_field}",
            "vmwrite rcx, rax",

            // VMX deliberately does not switch x87/SSE/AVX state. Save the
            // current AP host immediately before entry, then install the
            // VM-owned state after all host Rust code has finished.
            "mov r10, [rsp + 16]",
            "mov rax, [rsp]",
            "test rax, rax",
            "jz 5f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xsave64 [r10]",
            "jmp 6f",
            "5:",
            "fxsave64 [r10]",
            "6:",
            "mov r10, [rsp + 8]",
            "mov rax, [rsp]",
            "test rax, rax",
            "jz 7f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xrstor64 [r10]",
            "jmp 8f",
            "7:",
            "fxrstor64 [r10]",
            "8:",

            "mov r10, [rsp + 32]",
            "mov rax, [r10 + {guest_rax_off}]",
            "mov rbx, [r10 + {guest_rbx_off}]",
            "mov rcx, [r10 + {guest_rcx_off}]",
            "mov rdx, [r10 + {guest_rdx_off}]",
            "mov rsi, [r10 + {guest_rsi_off}]",
            "mov rdi, [r10 + {guest_rdi_off}]",
            "mov rbp, [r10 + {guest_rbp_off}]",
            "mov r8, [r10 + {guest_r8_off}]",
            "mov r9, [r10 + {guest_r9_off}]",
            "mov r11, [r10 + {guest_r11_off}]",
            "mov r12, [r10 + {guest_r12_off}]",
            "mov r13, [r10 + {guest_r13_off}]",
            "mov r14, [r10 + {guest_r14_off}]",
            "mov r15, [r10 + {guest_r15_off}]",
            "mov r10, [r10 + {guest_r10_off}]",

            "vmlaunch",
            "setna al",
            "mov r11, [rsp + 24]",
            "mov byte ptr [r11 + {launch_failed_off}], al",
            "cmp al, 0",
            "je 4f",
            "mov rcx, {vm_instr_err_field}",
            "vmread rax, rcx",
            "mov [r11 + {instr_err_off}], rax",
            "4:",

            // A failed VM-entry does not visit the VM-exit label, so restore
            // the AP host state here before returning to Rust.
            "mov r10, [rsp + 16]",
            "mov rax, [rsp]",
            "test rax, rax",
            "jz 9f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xrstor64 [r10]",
            "jmp 20f",
            "9:",
            "fxrstor64 [r10]",
            "20:",
            "jmp 3f",

            "2:",
            "push r10",
            "push r11",
            "mov r10, [rsp + 48]",
            "mov [r10 + {guest_rax_off}], rax",
            "mov [r10 + {guest_rbx_off}], rbx",
            "mov [r10 + {guest_rcx_off}], rcx",
            "mov [r10 + {guest_rdx_off}], rdx",
            "mov [r10 + {guest_rsi_off}], rsi",
            "mov [r10 + {guest_rdi_off}], rdi",
            "mov [r10 + {guest_rbp_off}], rbp",
            "mov [r10 + {guest_r8_off}], r8",
            "mov [r10 + {guest_r9_off}], r9",
            "mov rdx, [rsp + 8]",
            "mov [r10 + {guest_r10_off}], rdx",
            "mov rdx, [rsp]",
            "mov [r10 + {guest_r11_off}], rdx",
            "mov [r10 + {guest_r12_off}], r12",
            "mov [r10 + {guest_r13_off}], r13",
            "mov [r10 + {guest_r14_off}], r14",
            "mov [r10 + {guest_r15_off}], r15",

            // This is the first stateful work after VM exit. Preserve the
            // guest before any compiler-generated host SIMD can run, then
            // restore the AP host state saved immediately before entry.
            "mov r10, [rsp + 24]",
            "mov rax, [rsp + 16]",
            "test rax, rax",
            "jz 21f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xsave64 [r10]",
            "jmp 12f",
            "21:",
            "fxsave64 [r10]",
            "12:",
            "mov r10, [rsp + 32]",
            "mov rax, [rsp + 16]",
            "test rax, rax",
            "jz 13f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xrstor64 [r10]",
            "jmp 14f",
            "13:",
            "fxrstor64 [r10]",
            "14:",

            "add rsp, 16",
            "mov r11, [rsp + 24]",
            "mov byte ptr [r11 + {entered_off}], 1",
            "mov rcx, {exit_reason_field}",
            "vmread rax, rcx",
            "mov [r11 + {exit_reason_off}], rax",
            "mov rcx, {exit_qual_field}",
            "vmread rax, rcx",
            "mov [r11 + {exit_qual_off}], rax",
            "mov rcx, {guest_rip_field}",
            "vmread rax, rcx",
            "mov [r11 + {guest_rip_off}], rax",
            "3:",
            "cld",
            "add rsp, 40",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbp",
            "pop rbx",
            host_rsp_field = const VMCS_HOST_RSP,
            host_rip_field = const VMCS_HOST_RIP,
            vm_instr_err_field = const VMCS_VM_INSTRUCTION_ERROR,
            exit_reason_field = const VMCS_EXIT_REASON,
            exit_qual_field = const VMCS_EXIT_QUALIFICATION,
            guest_rip_field = const VMCS_VMEXIT_GUEST_RIP,
            guest_regs_base = in(reg) guest_regs_ptr,
            result_base = in(reg) result_ptr,
            host_extended_state_base = in(reg) host_extended_state_ptr,
            guest_extended_state_base = in(reg) guest_extended_state_ptr,
            extended_state_mask = in(reg) extended_state_mask,
            entered_off = const core::mem::offset_of!(LaunchResult, entered),
            launch_failed_off = const core::mem::offset_of!(LaunchResult, launch_failed),
            exit_reason_off = const core::mem::offset_of!(LaunchResult, exit_reason),
            exit_qual_off = const core::mem::offset_of!(LaunchResult, exit_qualification),
            guest_rip_off = const core::mem::offset_of!(LaunchResult, guest_rip),
            instr_err_off = const core::mem::offset_of!(LaunchResult, instr_err),
            guest_rax_off = const core::mem::offset_of!(GuestRegisters, rax),
            guest_rbx_off = const core::mem::offset_of!(GuestRegisters, rbx),
            guest_rcx_off = const core::mem::offset_of!(GuestRegisters, rcx),
            guest_rdx_off = const core::mem::offset_of!(GuestRegisters, rdx),
            guest_rsi_off = const core::mem::offset_of!(GuestRegisters, rsi),
            guest_rdi_off = const core::mem::offset_of!(GuestRegisters, rdi),
            guest_rbp_off = const core::mem::offset_of!(GuestRegisters, rbp),
            guest_r8_off = const core::mem::offset_of!(GuestRegisters, r8),
            guest_r9_off = const core::mem::offset_of!(GuestRegisters, r9),
            guest_r10_off = const core::mem::offset_of!(GuestRegisters, r10),
            guest_r11_off = const core::mem::offset_of!(GuestRegisters, r11),
            guest_r12_off = const core::mem::offset_of!(GuestRegisters, r12),
            guest_r13_off = const core::mem::offset_of!(GuestRegisters, r13),
            guest_r14_off = const core::mem::offset_of!(GuestRegisters, r14),
            guest_r15_off = const core::mem::offset_of!(GuestRegisters, r15),
            out("rax") _,
            out("rcx") _,
            out("rdx") _,
            out("r11") _,
            clobber_abi("sysv64"),
        );
        *out = result_ptr.read();
    }
}

pub fn vmresume_once_wrapper(vm_id: u8, out: &mut LaunchResult) {
    unsafe {
        let result_ptr = wrapper_result_ptr();
        let guest_regs_ptr = guest_regs_ptr();
        let host_extended_state_ptr = host_extended_state_ptr();
        let guest_extended_state_ptr =
            guest_extended_state_ptr(vm_id).expect("unsupported VM extended-state owner");
        let extended_state_mask = crate::cpu::vmx_xsave_mask();
        result_ptr.write(EMPTY_LAUNCH_RESULT);
        core::arch::asm!(
            "push rbx",
            "push rbp",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            "push {guest_regs_base}",
            "push {result_base}",
            "push {host_extended_state_base}",
            "push {guest_extended_state_base}",
            "push {extended_state_mask}",
            "mov rax, rsp",
            "mov rcx, {host_rsp_field}",
            "vmwrite rcx, rax",
            "lea rax, [rip + 2f]",
            "mov rcx, {host_rip_field}",
            "vmwrite rcx, rax",

            // The VM may resume on a different reserved AP. The host save is
            // per CPU slot while the installed state remains VM-owned.
            "mov r10, [rsp + 16]",
            "mov rax, [rsp]",
            "test rax, rax",
            "jz 5f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xsave64 [r10]",
            "jmp 6f",
            "5:",
            "fxsave64 [r10]",
            "6:",
            "mov r10, [rsp + 8]",
            "mov rax, [rsp]",
            "test rax, rax",
            "jz 7f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xrstor64 [r10]",
            "jmp 8f",
            "7:",
            "fxrstor64 [r10]",
            "8:",

            "mov r10, [rsp + 32]",
            "mov rax, [r10 + {guest_rax_off}]",
            "mov rbx, [r10 + {guest_rbx_off}]",
            "mov rcx, [r10 + {guest_rcx_off}]",
            "mov rdx, [r10 + {guest_rdx_off}]",
            "mov rsi, [r10 + {guest_rsi_off}]",
            "mov rdi, [r10 + {guest_rdi_off}]",
            "mov rbp, [r10 + {guest_rbp_off}]",
            "mov r8, [r10 + {guest_r8_off}]",
            "mov r9, [r10 + {guest_r9_off}]",
            "mov r11, [r10 + {guest_r11_off}]",
            "mov r12, [r10 + {guest_r12_off}]",
            "mov r13, [r10 + {guest_r13_off}]",
            "mov r14, [r10 + {guest_r14_off}]",
            "mov r15, [r10 + {guest_r15_off}]",
            "mov r10, [r10 + {guest_r10_off}]",

            "vmresume",
            "setna al",
            "mov r11, [rsp + 24]",
            "mov byte ptr [r11 + {launch_failed_off}], al",
            "cmp al, 0",
            "je 4f",
            "mov rcx, {vm_instr_err_field}",
            "vmread rax, rcx",
            "mov [r11 + {instr_err_off}], rax",
            "4:",

            // VMRESUME failure does not execute the VM-exit label.
            "mov r10, [rsp + 16]",
            "mov rax, [rsp]",
            "test rax, rax",
            "jz 9f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xrstor64 [r10]",
            "jmp 20f",
            "9:",
            "fxrstor64 [r10]",
            "20:",
            "jmp 3f",

            "2:",
            "push r10",
            "push r11",
            "mov r10, [rsp + 48]",
            "mov [r10 + {guest_rax_off}], rax",
            "mov [r10 + {guest_rbx_off}], rbx",
            "mov [r10 + {guest_rcx_off}], rcx",
            "mov [r10 + {guest_rdx_off}], rdx",
            "mov [r10 + {guest_rsi_off}], rsi",
            "mov [r10 + {guest_rdi_off}], rdi",
            "mov [r10 + {guest_rbp_off}], rbp",
            "mov [r10 + {guest_r8_off}], r8",
            "mov [r10 + {guest_r9_off}], r9",
            "mov rdx, [rsp + 8]",
            "mov [r10 + {guest_r10_off}], rdx",
            "mov rdx, [rsp]",
            "mov [r10 + {guest_r11_off}], rdx",
            "mov [r10 + {guest_r12_off}], r12",
            "mov [r10 + {guest_r13_off}], r13",
            "mov [r10 + {guest_r14_off}], r14",
            "mov [r10 + {guest_r15_off}], r15",

            "mov r10, [rsp + 24]",
            "mov rax, [rsp + 16]",
            "test rax, rax",
            "jz 21f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xsave64 [r10]",
            "jmp 12f",
            "21:",
            "fxsave64 [r10]",
            "12:",
            "mov r10, [rsp + 32]",
            "mov rax, [rsp + 16]",
            "test rax, rax",
            "jz 13f",
            "mov rdx, rax",
            "shr rdx, 32",
            "xrstor64 [r10]",
            "jmp 14f",
            "13:",
            "fxrstor64 [r10]",
            "14:",

            "add rsp, 16",
            "mov r11, [rsp + 24]",
            "mov byte ptr [r11 + {entered_off}], 1",
            "mov rcx, {exit_reason_field}",
            "vmread rax, rcx",
            "mov [r11 + {exit_reason_off}], rax",
            "mov rcx, {exit_qual_field}",
            "vmread rax, rcx",
            "mov [r11 + {exit_qual_off}], rax",
            "mov rcx, {guest_rip_field}",
            "vmread rax, rcx",
            "mov [r11 + {guest_rip_off}], rax",
            "3:",
            "cld",
            "add rsp, 40",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbp",
            "pop rbx",
            host_rsp_field = const VMCS_HOST_RSP,
            host_rip_field = const VMCS_HOST_RIP,
            vm_instr_err_field = const VMCS_VM_INSTRUCTION_ERROR,
            exit_reason_field = const VMCS_EXIT_REASON,
            exit_qual_field = const VMCS_EXIT_QUALIFICATION,
            guest_rip_field = const VMCS_VMEXIT_GUEST_RIP,
            guest_regs_base = in(reg) guest_regs_ptr,
            result_base = in(reg) result_ptr,
            host_extended_state_base = in(reg) host_extended_state_ptr,
            guest_extended_state_base = in(reg) guest_extended_state_ptr,
            extended_state_mask = in(reg) extended_state_mask,
            entered_off = const core::mem::offset_of!(LaunchResult, entered),
            launch_failed_off = const core::mem::offset_of!(LaunchResult, launch_failed),
            exit_reason_off = const core::mem::offset_of!(LaunchResult, exit_reason),
            exit_qual_off = const core::mem::offset_of!(LaunchResult, exit_qualification),
            guest_rip_off = const core::mem::offset_of!(LaunchResult, guest_rip),
            instr_err_off = const core::mem::offset_of!(LaunchResult, instr_err),
            guest_rax_off = const core::mem::offset_of!(GuestRegisters, rax),
            guest_rbx_off = const core::mem::offset_of!(GuestRegisters, rbx),
            guest_rcx_off = const core::mem::offset_of!(GuestRegisters, rcx),
            guest_rdx_off = const core::mem::offset_of!(GuestRegisters, rdx),
            guest_rsi_off = const core::mem::offset_of!(GuestRegisters, rsi),
            guest_rdi_off = const core::mem::offset_of!(GuestRegisters, rdi),
            guest_rbp_off = const core::mem::offset_of!(GuestRegisters, rbp),
            guest_r8_off = const core::mem::offset_of!(GuestRegisters, r8),
            guest_r9_off = const core::mem::offset_of!(GuestRegisters, r9),
            guest_r10_off = const core::mem::offset_of!(GuestRegisters, r10),
            guest_r11_off = const core::mem::offset_of!(GuestRegisters, r11),
            guest_r12_off = const core::mem::offset_of!(GuestRegisters, r12),
            guest_r13_off = const core::mem::offset_of!(GuestRegisters, r13),
            guest_r14_off = const core::mem::offset_of!(GuestRegisters, r14),
            guest_r15_off = const core::mem::offset_of!(GuestRegisters, r15),
            out("rax") _,
            out("rcx") _,
            out("rdx") _,
            out("r11") _,
            clobber_abi("sysv64"),
        );
        *out = result_ptr.read();
    }
}
