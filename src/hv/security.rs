//! Hypervisor security hardening ledger and hook points.
//!
//! These hooks are intentionally cheap stubs until the corresponding mitigations
//! are implemented. The risk IDs are stable breadcrumbs for audit notes, boot
//! logs, and follow-up patches.

pub const HVSR_0001_VMEXIT_PREDICTOR_ISOLATION: &str = "HVSR-0001";
pub const HVSR_0002_EPT_HOST_MEMORY_EXPOSURE: &str = "HVSR-0002";
pub const HVSR_0003_EPT_PERMISSION_NARROWING: &str = "HVSR-0003";
pub const HVSR_0004_GUEST_MSR_SURFACE: &str = "HVSR-0004";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VmexitMitigationMode {
    StubOnly,
}

pub fn vmexit_mitigation_mode() -> VmexitMitigationMode {
    VmexitMitigationMode::StubOnly
}

#[inline(always)]
pub fn legacy_host_heap_ept_enabled() -> bool {
    // Securit Risk and a Id to it: HVSR-0002
    // Keep host-private heap memory out of guest EPT by default. A legacy
    // bring-up escape hatch can live here if an old path still depends on it,
    // but new VMX plumbing should use per-VM heaps or explicit shared pages.
    false
}

#[inline(always)]
pub fn before_host_handles_vmexit(_vm_id: u8) {
    // Securit Risk and a Id to it: HVSR-0001
    // Guest-controlled branch predictor state can cross VMEXIT on affected CPUs.
    // Future patch: detect IBPB support and issue IA32_PRED_CMD.IBPB here, behind
    // a policy bit, before host code consumes guest-controlled VMEXIT state.
}

#[inline(always)]
pub fn before_building_guest_ept(_vm_id: u8) {
    // Securit Risk and a Id to it: HVSR-0002
    // The current sparse EPT builder still maps selected host-owned spans for
    // bring-up. Future patch: replace broad host spans with explicit per-VM
    // shared pages and copy-in/copy-out vmcall buffers.
}

#[inline(always)]
pub fn ept_permissions_for_span(_label: &str, default_perms: u64) -> u64 {
    // Securit Risk and a Id to it: HVSR-0003
    // Today callers pass the legacy RWX-style EPT permission bits. Future patch:
    // assign R/W/X per span so host data mappings are never executable and guest
    // code mappings are not writable unless explicitly staged.
    default_perms
}
