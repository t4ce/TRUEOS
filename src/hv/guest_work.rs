use crate::r::spawn_spec::SpawnPlacement;
use crate::workers::WorkerSpawner;

const VM_RESERVED_FIRST_SLOT: u32 = 2;

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
        }
    }

    pub const fn placement_name(self) -> &'static str {
        match self.placement {
            SpawnPlacement::Worker => "worker",
            SpawnPlacement::ReservedVmLane => "reserved-vm-lane",
        }
    }
}

pub struct VmLaneTarget {
    pub slot: u32,
    pub core_kind: u8,
    pub spawner: WorkerSpawner,
    pub lease: crate::hv::lane::LaneLease,
}

impl VmLaneTarget {
    pub fn core_kind_name(&self) -> &'static str {
        match self.core_kind {
            crate::workers::CORE_KIND_PERF => "perf",
            crate::workers::CORE_KIND_EFF => "eff",
            _ => "unknown",
        }
    }

    pub const fn is_reserved_vm_lane(&self) -> bool {
        self.slot >= VM_RESERVED_FIRST_SLOT
    }

    pub fn supports(&self, profile: VmLaneProfile) -> bool {
        match profile.placement {
            SpawnPlacement::Worker => self.is_reserved_vm_lane(),
            SpawnPlacement::ReservedVmLane => self.is_reserved_vm_lane(),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VmLanePickError {
    MissingWorkerLane,
    MissingReservedVmLane,
    Busy,
}

impl VmLanePickError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingWorkerLane => "no disposable worker lanes are registered",
            Self::MissingReservedVmLane => "no reserved VM lanes are registered at AP2+",
            Self::Busy => "matching runtime carrier lanes are currently leased",
        }
    }
}

pub type GuestWorkProfile = VmLaneProfile;
pub type GuestWorkTarget = VmLaneTarget;

pub fn select_vm_lane_target(profile: VmLaneProfile) -> Result<VmLaneTarget, VmLanePickError> {
    let target = crate::hv::lane::pick_carrier_lane(crate::hv::lane::LaneProfile {
        role: match profile.role {
            VmLaneRole::VmHull => crate::hv::lane::LaneRole::VmHull,
            VmLaneRole::TokioBlocking => crate::hv::lane::LaneRole::TokioBlocking,
            VmLaneRole::Worker => crate::hv::lane::LaneRole::Worker,
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

pub fn pick_guest_work_target(profile: GuestWorkProfile) -> Option<GuestWorkTarget> {
    pick_vm_lane_target(profile)
}

fn map_lane_pick_error(error: crate::hv::lane::LanePickError) -> VmLanePickError {
    match error {
        crate::hv::lane::LanePickError::MissingWorkerLane => VmLanePickError::MissingWorkerLane,
        crate::hv::lane::LanePickError::MissingReservedVmLane => {
            VmLanePickError::MissingReservedVmLane
        }
        crate::hv::lane::LanePickError::Busy => VmLanePickError::Busy,
    }
}
