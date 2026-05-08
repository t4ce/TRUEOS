use embassy_executor::SendSpawner;

use crate::r::spawn_spec::SpawnPlacement;

const VM_RESERVED_FIRST_SLOT: u32 = 2;
const AP1_SERVICE_SLOT: u32 = 1;

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

pub struct VmLaneTarget {
    pub slot: u32,
    pub core_kind: u8,
    pub spawner: SendSpawner,
    pub lease: crate::r::lane::LaneLease,
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
    Busy,
}

impl VmLanePickError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalPlacementUnsupported => "local placement is not a VM lane",
            Self::MissingAp1Lane => "ap1 service lane is not registered",
            Self::MissingWorkerLane => "no disposable worker lanes are registered",
            Self::MissingReservedVmLane => "no reserved VM lanes are registered at AP2+",
            Self::Busy => "matching runtime carrier lanes are currently leased",
        }
    }
}

pub type GuestWorkProfile = VmLaneProfile;
pub type GuestWorkTarget = VmLaneTarget;

pub fn select_vm_lane_target(profile: VmLaneProfile) -> Result<VmLaneTarget, VmLanePickError> {
    let target = crate::r::lane::pick_carrier_lane(crate::r::lane::LaneProfile {
        role: match profile.role {
            VmLaneRole::VmHull => crate::r::lane::LaneRole::VmHull,
            VmLaneRole::TokioBlocking => crate::r::lane::LaneRole::TokioBlocking,
            VmLaneRole::Worker => crate::r::lane::LaneRole::Worker,
            VmLaneRole::Service => crate::r::lane::LaneRole::Service,
        },
        placement: profile.placement,
    })
    .map_err(map_lane_pick_error)?;

    Ok(VmLaneTarget {
        slot: target.slot,
        core_kind: target.core_kind,
        spawner: target.spawner,
        lease: target.lease,
    })
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

fn map_lane_pick_error(error: crate::r::lane::LanePickError) -> VmLanePickError {
    match error {
        crate::r::lane::LanePickError::LocalPlacementUnsupported => {
            VmLanePickError::LocalPlacementUnsupported
        }
        crate::r::lane::LanePickError::MissingAp1Lane => VmLanePickError::MissingAp1Lane,
        crate::r::lane::LanePickError::MissingWorkerLane => VmLanePickError::MissingWorkerLane,
        crate::r::lane::LanePickError::MissingReservedVmLane => {
            VmLanePickError::MissingReservedVmLane
        }
        crate::r::lane::LanePickError::Busy => VmLanePickError::Busy,
    }
}
