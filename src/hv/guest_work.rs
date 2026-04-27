use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::SendSpawner;

use crate::r::spawn_spec::SpawnPlacement;

const VM_RESERVED_FIRST_SLOT: u32 = 2;
const AP1_SERVICE_SLOT: u32 = 1;

static GUEST_WORK_RR: AtomicU64 = AtomicU64::new(0);

/// TRUEOS carrier-lane roles.
///
/// The architectural choice here is to treat executor-backed VM lanes as the
/// native carrier substrate for both hull execution and future Tokio blocking
/// work, instead of introducing a separate host-thread-style abstraction.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VmLaneRole {
    VmHull,
    TokioBlocking,
    Worker,
    Service,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VmLaneProfile {
    pub role: VmLaneRole,
    pub placement: SpawnPlacement,
}

impl VmLaneProfile {
    /// Default placement contract for TRUEOS VM hull execution.
    ///
    /// Keep guest hulls on HV-reserved VM lanes only:
    /// - never on BSP/local
    /// - never on the AP1 UI2/service lane
    /// - use every registered AP2+ worker lane
    ///
    /// This is the placement policy that lane-indexed `vm[n]` scheduling must
    /// preserve now that VMXON/VMCS backing is per CPU slot.
    pub const fn vm_default() -> Self {
        Self {
            role: VmLaneRole::VmHull,
            placement: SpawnPlacement::ReservedVmLane,
        }
    }

    /// Default placement for synchronous offload work that should stay off the
    /// BSP and the first two AP service lanes while still using disposable executor carriers.
    pub const fn tokio_blocking_default() -> Self {
        Self {
            role: VmLaneRole::TokioBlocking,
            placement: SpawnPlacement::Worker,
        }
    }

    pub const fn worker_default() -> Self {
        Self {
            role: VmLaneRole::Worker,
            placement: SpawnPlacement::Worker,
        }
    }

    pub const fn service_default() -> Self {
        Self {
            role: VmLaneRole::Service,
            placement: SpawnPlacement::Ap1,
        }
    }

    pub const fn requires_ap1_lane(self) -> bool {
        matches!(self.placement, SpawnPlacement::Ap1)
    }

    pub const fn requires_reserved_vm_lane(self) -> bool {
        matches!(self.placement, SpawnPlacement::ReservedVmLane)
    }

    pub const fn requires_disposable_worker_lane(self) -> bool {
        matches!(self.placement, SpawnPlacement::Worker)
    }

    pub const fn role_name(self) -> &'static str {
        match self.role {
            VmLaneRole::VmHull => "vm-hull",
            VmLaneRole::TokioBlocking => "tokio-blocking",
            VmLaneRole::Worker => "worker",
            VmLaneRole::Service => "service",
        }
    }

    pub const fn placement_name(self) -> &'static str {
        match self.placement {
            SpawnPlacement::Local => "local",
            SpawnPlacement::Ap1 => "ap1",
            SpawnPlacement::Worker => "worker",
            SpawnPlacement::ReservedVmLane => "reserved-vm-lane",
        }
    }
}

#[derive(Clone)]
pub struct VmLaneTarget {
    pub slot: u32,
    pub core_kind: u8,
    pub spawner: SendSpawner,
}

impl VmLaneTarget {
    pub fn core_kind_name(&self) -> &'static str {
        match self.core_kind {
            crate::workers::CORE_KIND_PERF => "perf",
            crate::workers::CORE_KIND_EFF => "eff",
            _ => "unknown",
        }
    }

    pub const fn is_ap1_service_lane(&self) -> bool {
        self.slot == AP1_SERVICE_SLOT
    }

    pub const fn is_reserved_vm_lane(&self) -> bool {
        self.slot >= VM_RESERVED_FIRST_SLOT
    }

    pub fn supports(&self, profile: VmLaneProfile) -> bool {
        match profile.placement {
            SpawnPlacement::Local => false,
            SpawnPlacement::Ap1 => self.is_ap1_service_lane(),
            SpawnPlacement::Worker => self.is_reserved_vm_lane(),
            SpawnPlacement::ReservedVmLane => self.is_reserved_vm_lane(),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VmLanePickError {
    LocalPlacementUnsupported,
    MissingAp1Lane,
    MissingWorkerLane,
    MissingReservedVmLane,
}

impl VmLanePickError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalPlacementUnsupported => "local placement is not a VM lane",
            Self::MissingAp1Lane => "ap1 service lane is not registered",
            Self::MissingWorkerLane => "no disposable worker lanes are registered",
            Self::MissingReservedVmLane => "no reserved VM lanes are registered at AP2+",
        }
    }
}

pub type GuestWorkProfile = VmLaneProfile;
pub type GuestWorkTarget = VmLaneTarget;

pub fn select_vm_lane_target(profile: VmLaneProfile) -> Result<VmLaneTarget, VmLanePickError> {
    match profile.placement {
        SpawnPlacement::ReservedVmLane => pick_reserved_vm_lane(),
        SpawnPlacement::Worker => pick_background_worker(),
        SpawnPlacement::Ap1 => pick_ap1_lane(),
        SpawnPlacement::Local => Err(VmLanePickError::LocalPlacementUnsupported),
    }
}

pub fn pick_vm_lane_target(profile: VmLaneProfile) -> Option<VmLaneTarget> {
    select_vm_lane_target(profile).ok()
}

pub fn pick_vm_hull_lane() -> Result<VmLaneTarget, VmLanePickError> {
    select_vm_lane_target(VmLaneProfile::vm_default())
}

pub fn pick_tokio_blocking_lane() -> Result<VmLaneTarget, VmLanePickError> {
    select_vm_lane_target(VmLaneProfile::tokio_blocking_default())
}

pub fn pick_worker_lane() -> Result<VmLaneTarget, VmLanePickError> {
    select_vm_lane_target(VmLaneProfile::worker_default())
}

pub fn pick_service_lane() -> Result<VmLaneTarget, VmLanePickError> {
    select_vm_lane_target(VmLaneProfile::service_default())
}

pub fn pick_guest_work_target(profile: GuestWorkProfile) -> Option<GuestWorkTarget> {
    pick_vm_lane_target(profile)
}

fn pick_ap1_lane() -> Result<VmLaneTarget, VmLanePickError> {
    let profile = crate::cpu::CpuProfile::for_slot(AP1_SERVICE_SLOT)
        .ok_or(VmLanePickError::MissingAp1Lane)?;
    let spawner =
        crate::workers::spawner_for_slot(profile.slot()).ok_or(VmLanePickError::MissingAp1Lane)?;
    Ok(VmLaneTarget {
        slot: profile.slot(),
        core_kind: profile.core_kind(),
        spawner,
    })
}

fn pick_background_worker() -> Result<VmLaneTarget, VmLanePickError> {
    let pool = collect_disposable_worker_lanes();
    pick_round_robin(&pool).ok_or(VmLanePickError::MissingWorkerLane)
}

fn pick_reserved_vm_lane() -> Result<VmLaneTarget, VmLanePickError> {
    let pool = collect_disposable_worker_lanes();
    let mut reserved: Vec<VmLaneTarget> = Vec::new();

    for target in pool {
        // Slot 0 is BSP/local and slot 1 is UI2/service work.
        // VM hull work owns AP2+ lanes as the host/user reserved carrier set.
        if !target.is_reserved_vm_lane() {
            continue;
        }
        reserved.push(target);
    }

    pick_round_robin(&reserved).ok_or(VmLanePickError::MissingReservedVmLane)
}

fn collect_disposable_worker_lanes() -> Vec<VmLaneTarget> {
    let slots = crate::workers::background_worker_slots();
    let mut pool: Vec<VmLaneTarget> = Vec::new();
    for slot in slots {
        let Some(spawner) = crate::workers::spawner_for_slot(slot) else {
            continue;
        };
        let profile = crate::cpu::CpuProfile::for_slot(slot);
        let core_kind = profile
            .map(|profile| profile.core_kind())
            .unwrap_or(crate::workers::CORE_KIND_UNKNOWN);
        pool.push(VmLaneTarget {
            slot,
            core_kind,
            spawner,
        });
    }
    pool
}

fn pick_round_robin(pool: &[VmLaneTarget]) -> Option<VmLaneTarget> {
    if pool.is_empty() {
        return None;
    }
    let idx = GUEST_WORK_RR.fetch_add(1, Ordering::Relaxed) as usize;
    Some(pool[idx % pool.len()].clone())
}
