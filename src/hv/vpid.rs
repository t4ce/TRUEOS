//! Lane-local Virtual Processor Identifier (VPID) ownership.
//!
//! A VPID is disposable translation-cache state on one logical processor. It
//! is not a VM principal and is never snapshot state. The VM run generation
//! stored here only makes an executor assignment auditable; correctness comes
//! from invalidating the nonzero VPID before first entry and before reuse.

use core::sync::atomic::{AtomicU8, AtomicU16, AtomicU64, Ordering};

use x86_64::registers::model_specific::Msr;

use crate::hv::{hverrorf, hvlogf};

const LANE_OFFLINE: u8 = 0;
const LANE_READY: u8 = 1;
const LANE_BOOTSTRAPPING: u8 = 2;
const LANE_ASSIGNING: u8 = 3;
const LANE_ACTIVE: u8 = 4;
const LANE_RETIRING: u8 = 5;
const LANE_DRAINING: u8 = 6;
const LANE_QUARANTINED: u8 = u8::MAX;

const EPT_VPID_CAP_INVEPT: u64 = 1 << 20;
const EPT_VPID_CAP_INVEPT_SINGLE_CONTEXT: u64 = 1 << 25;
const EPT_VPID_CAP_INVVPID: u64 = 1 << 32;
const EPT_VPID_CAP_INVVPID_SINGLE_CONTEXT: u64 = 1 << 41;

struct LaneVpidState {
    state: AtomicU8,
    owner_tag: AtomicU16,
    vpid: AtomicU16,
    generation: AtomicU64,
}

impl LaneVpidState {
    const fn new() -> Self {
        Self {
            state: AtomicU8::new(LANE_OFFLINE),
            owner_tag: AtomicU16::new(0),
            vpid: AtomicU16::new(0),
            generation: AtomicU64::new(0),
        }
    }
}

static LANE_VPID_STATE: [LaneVpidState; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { LaneVpidState::new() }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VpidCapabilities {
    pub secondary_controls: u64,
    pub ept_vpid: u64,
}

impl VpidCapabilities {
    fn read_current_cpu() -> Self {
        Self {
            secondary_controls: unsafe {
                Msr::new(crate::hv::vmx::IA32_VMX_PROCBASED_CTLS2).read()
            },
            ept_vpid: unsafe { Msr::new(crate::hv::vmx::IA32_VMX_EPT_VPID_CAP).read() },
        }
    }

    pub const fn enable_vpid(self) -> bool {
        ((self.secondary_controls >> 32) & crate::hv::vmx::PROC2_BASED_ENABLE_VPID) != 0
    }

    pub const fn invvpid(self) -> bool {
        (self.ept_vpid & EPT_VPID_CAP_INVVPID) != 0
    }

    pub const fn invvpid_single_context(self) -> bool {
        (self.ept_vpid & EPT_VPID_CAP_INVVPID_SINGLE_CONTEXT) != 0
    }

    pub const fn invept(self) -> bool {
        (self.ept_vpid & EPT_VPID_CAP_INVEPT) != 0
    }

    pub const fn invept_single_context(self) -> bool {
        (self.ept_vpid & EPT_VPID_CAP_INVEPT_SINGLE_CONTEXT) != 0
    }

    pub const fn satisfies_lane_contract(self) -> bool {
        self.enable_vpid()
            && self.invvpid()
            && self.invvpid_single_context()
            && self.invept()
            && self.invept_single_context()
    }
}

/// Establish the VPID half of the per-CPU VMX contract after VMXON.
///
/// Flushing every VPID that TRUEOS can allocate closes retained linear and
/// combined mappings across AP reset, VMXOFF/VMXON, and a replacement kernel.
/// Each rebuilt EPTP receives its separate INVEPT fence before entry. Per-run
/// VPID assignment and retirement still issue their own single-context flushes.
pub fn initialize_current_lane() -> Result<(), &'static str> {
    let slot = current_lane_slot()?;
    let lane = &LANE_VPID_STATE[slot];
    let capabilities = VpidCapabilities::read_current_cpu();
    hvlogf(format_args!(
        "hv: vpid capability slot={} proc2_caps=0x{:016X} ept_vpid_caps=0x{:016X} enable_vpid={} invvpid={} invvpid_single={} invept={} invept_single={} contract=required",
        slot,
        capabilities.secondary_controls,
        capabilities.ept_vpid,
        capabilities.enable_vpid() as u8,
        capabilities.invvpid() as u8,
        capabilities.invvpid_single_context() as u8,
        capabilities.invept() as u8,
        capabilities.invept_single_context() as u8,
    ));
    if !capabilities.satisfies_lane_contract() {
        quarantine_lane(slot, "required VPID translation capability missing");
        return Err("required VPID/INVVPID/INVEPT capability missing");
    }

    if lane
        .state
        .compare_exchange(LANE_OFFLINE, LANE_BOOTSTRAPPING, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        quarantine_lane(slot, "VPID lane was not offline at VMXON");
        return Err("VPID lane state invalid at VMXON");
    }

    if flush_allocatable_vpids().is_err() {
        quarantine_lane(slot, "VPID boot invalidation failed");
        return Err("VPID boot invalidation failed");
    }

    lane.owner_tag.store(0, Ordering::Release);
    lane.vpid.store(0, Ordering::Release);
    lane.generation.store(0, Ordering::Release);
    lane.state.store(LANE_READY, Ordering::Release);
    hvlogf(format_args!(
        "hv: vpid lane ready slot={} invalidated={} policy=invvpid-assign+retire/invept-before-entry ept=isolation-boundary snapshot=excluded",
        slot,
        crate::allcaps::hv::VM_ID_LIMIT,
    ));
    Ok(())
}

/// Flush every allocatable VPID before leaving VMX operation.
pub fn prepare_current_lane_for_vmxoff() -> Result<(), &'static str> {
    let slot = current_lane_slot()?;
    let lane = &LANE_VPID_STATE[slot];
    if lane
        .state
        .compare_exchange(LANE_READY, LANE_DRAINING, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        quarantine_lane(slot, "VPID lane was not idle before VMXOFF");
        return Err("VPID lane active during VMXOFF");
    }

    if flush_allocatable_vpids().is_err() {
        quarantine_lane(slot, "VPID VMXOFF invalidation failed");
        return Err("VPID VMXOFF invalidation failed");
    }

    lane.state.store(LANE_READY, Ordering::Release);
    hvlogf(format_args!(
        "hv: vpid lane drained slot={} invalidated={} boundary=vmxoff",
        slot,
        crate::allcaps::hv::VM_ID_LIMIT,
    ));
    Ok(())
}

/// Publish that VMXOFF completed after `prepare_current_lane_for_vmxoff`.
pub fn mark_current_lane_offline() -> Result<(), &'static str> {
    let slot = current_lane_slot()?;
    let lane = &LANE_VPID_STATE[slot];
    if lane
        .state
        .compare_exchange(LANE_READY, LANE_OFFLINE, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        quarantine_lane(slot, "VPID lane changed while completing VMXOFF");
        return Err("VPID lane state invalid after VMXOFF");
    }
    Ok(())
}

#[must_use = "dropping an active VPID assignment retires it and may quarantine its lane"]
pub struct VpidAssignment {
    slot: usize,
    vm_id: u8,
    vpid: u16,
    generation: u64,
    armed: bool,
}

impl VpidAssignment {
    pub const fn vpid(&self) -> u16 {
        self.vpid
    }

    /// Confirm that an async executor yield did not move this VMCS/VPID
    /// assignment to another logical processor before the next VM entry.
    pub fn verify_entry_lane(&self) -> Result<(), &'static str> {
        self.verify_active_assignment("VM entry")
    }

    /// Fence the per-VM EPT tree after it has been rebuilt in place and before
    /// VM entry. INVVPID alone is not required to remove guest-physical EPT
    /// mappings, so the strict assignment boundary needs both instructions.
    pub fn fence_ept(&self, eptp: u64) -> Result<(), &'static str> {
        self.verify_active_assignment("EPT fence")?;

        if !crate::hv::vmx::invept_single_context(eptp) {
            quarantine_lane(self.slot, "EPT single-context invalidation failed");
            return Err("EPT single-context invalidation failed");
        }

        hvlogf(format_args!(
            "hv: vpid ept fenced vm={} vpid={} generation={} slot={} eptp=0x{:016X} invalidation=single-context",
            self.vm_id, self.vpid, self.generation, self.slot, eptp
        ));
        Ok(())
    }

    fn verify_active_assignment(&self, boundary: &'static str) -> Result<(), &'static str> {
        if !self.armed || crate::percpu::current_slot() != self.slot {
            quarantine_lane(self.slot, "active VPID assignment moved to another lane");
            hverrorf(format_args!(
                "hv: vpid assignment rejected vm={} vpid={} generation={} assigned_slot={} current_slot={} boundary={} action=quarantine-lane",
                self.vm_id,
                self.vpid,
                self.generation,
                self.slot,
                crate::percpu::current_slot(),
                boundary,
            ));
            return Err("active VPID assignment is on the wrong lane");
        }

        let lane = &LANE_VPID_STATE[self.slot];
        if lane.state.load(Ordering::Acquire) != LANE_ACTIVE
            || lane.owner_tag.load(Ordering::Acquire) != u16::from(self.vm_id) + 1
            || lane.vpid.load(Ordering::Acquire) != self.vpid
            || lane.generation.load(Ordering::Acquire) != self.generation
        {
            quarantine_lane(self.slot, "active VPID assignment metadata mismatch");
            return Err("active VPID assignment metadata mismatch");
        }
        Ok(())
    }

    /// Retire the assignment before its executor lane can be reused.
    pub fn retire(mut self) -> Result<(), &'static str> {
        self.retire_inner("teardown")
    }

    fn retire_inner(&mut self, boundary: &'static str) -> Result<(), &'static str> {
        if !self.armed {
            return Ok(());
        }
        self.armed = false;

        let current = crate::percpu::current_slot();
        if current != self.slot {
            quarantine_lane(self.slot, "VPID assignment migrated before retirement");
            hverrorf(format_args!(
                "hv: vpid retirement rejected vm={} vpid={} generation={} assigned_slot={} current_slot={} action=quarantine-lane",
                self.vm_id, self.vpid, self.generation, self.slot, current
            ));
            return Err("VPID assignment migrated before retirement");
        }

        let lane = &LANE_VPID_STATE[self.slot];
        if lane
            .state
            .compare_exchange(LANE_ACTIVE, LANE_RETIRING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            quarantine_lane(self.slot, "VPID assignment state invalid at retirement");
            return Err("VPID assignment state invalid at retirement");
        }

        if !crate::hv::vmx::invvpid_single_context(self.vpid) {
            quarantine_lane(self.slot, "VPID retirement invalidation failed");
            return Err("VPID retirement invalidation failed");
        }

        hvlogf(format_args!(
            "hv: vpid retired vm={} vpid={} generation={} slot={} boundary={} invalidation=single-context",
            self.vm_id, self.vpid, self.generation, self.slot, boundary
        ));
        lane.owner_tag.store(0, Ordering::Release);
        lane.vpid.store(0, Ordering::Release);
        lane.generation.store(0, Ordering::Release);
        lane.state.store(LANE_READY, Ordering::Release);
        Ok(())
    }
}

impl Drop for VpidAssignment {
    fn drop(&mut self) {
        let _ = self.retire_inner("drop");
    }
}

/// Bind one VM run generation to the current executor lane.
pub fn assign_current_lane(
    vm_id: u8,
    generation: u64,
    expected_slot: usize,
) -> Result<VpidAssignment, &'static str> {
    let vpid = vpid_for_vm(vm_id).ok_or("VM id cannot be represented as a VPID")?;
    let current_slot = crate::percpu::current_slot();
    if current_slot != expected_slot || current_slot < 2 || current_slot >= LANE_VPID_STATE.len() {
        if expected_slot < LANE_VPID_STATE.len() {
            quarantine_lane(expected_slot, "executor moved VPID assignment to another lane");
        }
        return Err("VM executor lane changed before VPID assignment");
    }

    let capabilities = VpidCapabilities::read_current_cpu();
    if !capabilities.satisfies_lane_contract() {
        quarantine_lane(current_slot, "required VPID translation capability disappeared");
        return Err("required VPID/INVVPID/INVEPT capability unavailable");
    }

    let lane = &LANE_VPID_STATE[current_slot];
    if lane
        .state
        .compare_exchange(LANE_READY, LANE_ASSIGNING, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        quarantine_lane(current_slot, "VPID lane was not reusable at assignment");
        return Err("VPID lane not reusable");
    }

    lane.owner_tag
        .store(u16::from(vm_id).saturating_add(1), Ordering::Release);
    lane.vpid.store(vpid, Ordering::Release);
    lane.generation.store(generation, Ordering::Release);

    if !crate::hv::vmx::invvpid_single_context(vpid) {
        quarantine_lane(current_slot, "VPID assignment invalidation failed");
        return Err("VPID assignment invalidation failed");
    }

    lane.state.store(LANE_ACTIVE, Ordering::Release);
    hvlogf(format_args!(
        "hv: vpid assigned vm={} vpid={} generation={} slot={} boundary=first-entry invalidation=single-context",
        vm_id, vpid, generation, current_slot
    ));
    Ok(VpidAssignment {
        slot: current_slot,
        vm_id,
        vpid,
        generation,
        armed: true,
    })
}

/// Invalidate cached guest-linear and combined translations after the host
/// changes an active guest's page tables while its VPID remains assigned.
///
/// VPID deliberately preserves translations across VM exits, so changing a
/// guest PTE from NX to executable (or back to NX) is not complete until this
/// fence succeeds on the exact executor lane that owns the assignment.
pub fn invalidate_active_guest_translations(
    vm_id: u8,
    boundary: &'static str,
) -> Result<(), &'static str> {
    let slot = current_lane_slot()?;
    let expected_vpid = vpid_for_vm(vm_id).ok_or("VM id cannot be represented as a VPID")?;
    let lane = &LANE_VPID_STATE[slot];
    let state = lane.state.load(Ordering::Acquire);
    let owner_tag = lane.owner_tag.load(Ordering::Acquire);
    let vpid = lane.vpid.load(Ordering::Acquire);
    let generation = lane.generation.load(Ordering::Acquire);

    if state != LANE_ACTIVE
        || owner_tag != u16::from(vm_id) + 1
        || vpid != expected_vpid
        || generation == 0
    {
        quarantine_lane(slot, "active VPID invalidation metadata mismatch");
        hverrorf(format_args!(
            "hv: vpid invalidation rejected vm={} expected_vpid={} observed_vpid={} generation={} slot={} state={} owner_tag={} boundary={} action=quarantine-lane",
            vm_id,
            expected_vpid,
            vpid,
            generation,
            slot,
            state_name(state),
            owner_tag,
            boundary,
        ));
        return Err("active VPID invalidation metadata mismatch");
    }

    if !crate::hv::vmx::invvpid_single_context(vpid) {
        quarantine_lane(slot, "active VPID invalidation failed");
        return Err("active VPID invalidation failed");
    }

    hvlogf(format_args!(
        "hv: vpid invalidated vm={} vpid={} generation={} slot={} boundary={} invalidation=single-context",
        vm_id, vpid, generation, slot, boundary
    ));
    Ok(())
}

/// A VM hull lane may return to the executor pool only in the clean state
/// established by successful VPID retirement.
pub fn lane_reusable(slot: usize) -> bool {
    LANE_VPID_STATE
        .get(slot)
        .map(|lane| lane.state.load(Ordering::Acquire) == LANE_READY)
        .unwrap_or(false)
}

pub fn lane_state_name(slot: usize) -> &'static str {
    let Some(lane) = LANE_VPID_STATE.get(slot) else {
        return "out-of-range";
    };
    state_name(lane.state.load(Ordering::Acquire))
}

fn current_lane_slot() -> Result<usize, &'static str> {
    let slot = crate::percpu::current_slot();
    if slot < LANE_VPID_STATE.len() && slot >= 2 {
        Ok(slot)
    } else {
        Err("VPID requires an AP2+ executor lane")
    }
}

const fn vpid_for_vm(vm_id: u8) -> Option<u16> {
    if vm_id as usize >= crate::allcaps::hv::VM_ID_LIMIT {
        return None;
    }
    Some(vm_id as u16 + 1)
}

fn flush_allocatable_vpids() -> Result<(), &'static str> {
    for vm_index in 0..crate::allcaps::hv::VM_ID_LIMIT {
        let vm_id = u8::try_from(vm_index).map_err(|_| "VM id exceeds VPID allocator")?;
        let vpid = vpid_for_vm(vm_id).ok_or("VM id cannot be represented as a VPID")?;
        if !crate::hv::vmx::invvpid_single_context(vpid) {
            return Err("single-context INVVPID failed");
        }
    }
    Ok(())
}

fn quarantine_lane(slot: usize, reason: &'static str) {
    let Some(lane) = LANE_VPID_STATE.get(slot) else {
        return;
    };
    let previous = lane.state.swap(LANE_QUARANTINED, Ordering::AcqRel);
    if previous == LANE_QUARANTINED {
        return;
    }
    hverrorf(format_args!(
        "hv: vpid lane quarantined slot={} owner_tag={} vpid={} generation={} previous_state={} reason={} action=deny-lane-reuse-until-reboot",
        slot,
        lane.owner_tag.load(Ordering::Acquire),
        lane.vpid.load(Ordering::Acquire),
        lane.generation.load(Ordering::Acquire),
        state_name(previous),
        reason,
    ));
}

const fn state_name(state: u8) -> &'static str {
    match state {
        LANE_OFFLINE => "offline",
        LANE_READY => "ready",
        LANE_BOOTSTRAPPING => "bootstrapping",
        LANE_ASSIGNING => "assigning",
        LANE_ACTIVE => "active",
        LANE_RETIRING => "retiring",
        LANE_DRAINING => "draining",
        LANE_QUARANTINED => "quarantined",
        _ => "invalid",
    }
}

const _: () = assert!(crate::allcaps::hv::VM_ID_LIMIT <= u8::MAX as usize);
const _: () = assert!(matches!(vpid_for_vm(0), Some(1)));
const _: () = assert!(vpid_for_vm((crate::allcaps::hv::VM_ID_LIMIT - 1) as u8).is_some());
const _: () = assert!(
    VpidCapabilities {
        secondary_controls: (crate::hv::vmx::PROC2_BASED_ENABLE_VPID << 32),
        ept_vpid: EPT_VPID_CAP_INVEPT
            | EPT_VPID_CAP_INVEPT_SINGLE_CONTEXT
            | EPT_VPID_CAP_INVVPID
            | EPT_VPID_CAP_INVVPID_SINGLE_CONTEXT,
    }
    .satisfies_lane_contract()
);
const _: () = assert!(
    !VpidCapabilities {
        secondary_controls: (crate::hv::vmx::PROC2_BASED_ENABLE_VPID << 32),
        ept_vpid: EPT_VPID_CAP_INVVPID | EPT_VPID_CAP_INVVPID_SINGLE_CONTEXT,
    }
    .satisfies_lane_contract()
);
