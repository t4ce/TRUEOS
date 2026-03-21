// Intel VMX capability probe — hardware primitives available for layered virtualization.
// EPT, VPID, VMCS Shadowing, EPTP Switching, TSC Scaling determine which accel tier
// each level may use. SVM is a backend alias for the same concepts; this module is Intel-only.

const MSR_VMX_PROCBASED_CTLS2: u32 = 0x48B;
const MSR_VMX_VMFUNC: u32 = 0x491;

// Secondary proc-based control capability bits (high 32 of MSR = allowed-1)
const CAP_EPT: u64 = 1 << 1;
const CAP_UNRESTRICTED_GUEST: u64 = 1 << 7;
const CAP_VMCS_SHADOWING: u64 = 1 << 14;
const CAP_EPTP_SWITCHING: u64 = 1 << 19;
const CAP_VPID: u64 = 1 << 5;

unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") lo,
        out("edx") hi,
    );
    (hi as u64) << 32 | lo as u64
}

#[derive(Clone, Copy)]
pub struct VmxCaps {
    /// EPT available: per-level memory isolation, copy-on-write snapshot layers.
    pub ept: bool,
    /// VPID available: TLB tagging per VM, no flush on level switch.
    pub vpid: bool,
    /// Unrestricted guest: guest may run in real/protected mode without emulation.
    pub unrestricted_guest: bool,
    /// VMCS Shadowing: L1's VMREAD/VMWRITE operate on shadow VMCS without exiting L0.
    pub vmcs_shadowing: bool,
    /// EPTP Switching (VMFUNC leaf 0): guest switches EPT pointer with zero exits.
    /// This is the container memory-namespace switch primitive.
    pub eptp_switching: bool,
}

impl VmxCaps {
    /// Probe capabilities from VMX MSRs. Call only after VMXON is confirmed.
    pub fn probe() -> Self {
        // High 32 bits of secondary proc-based ctls MSR = allowed-1 (feature available).
        let ctls2_hi = unsafe { rdmsr(MSR_VMX_PROCBASED_CTLS2) } >> 32;
        let vmfunc = unsafe { rdmsr(MSR_VMX_VMFUNC) };
        Self {
            ept: ctls2_hi & CAP_EPT != 0,
            vpid: ctls2_hi & CAP_VPID != 0,
            unrestricted_guest: ctls2_hi & CAP_UNRESTRICTED_GUEST != 0,
            vmcs_shadowing: ctls2_hi & CAP_VMCS_SHADOWING != 0,
            eptp_switching: vmfunc & CAP_EPTP_SWITCHING != 0,
        }
    }

    pub fn log(&self) {
        crate::log!(
            "hvv-caps: ept={} vpid={} unrestricted={} vmcs_shadow={} eptp_switch={}",
            self.ept,
            self.vpid,
            self.unrestricted_guest,
            self.vmcs_shadowing,
            self.eptp_switching,
        );
    }
}
