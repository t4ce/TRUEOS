extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use embassy_executor::SendSpawner;

use crate::r::spawn_spec::SpawnPlacement;

const AP2_FIRST_CARRIER_SLOT: u32 = 2;

const LANE_FREE: u8 = 0;
const LANE_VM_HULL: u8 = 1;
const LANE_TOKIO_BLOCKING: u8 = 2;

static LANE_OWNER: [AtomicU8; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { AtomicU8::new(LANE_FREE) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
static LANE_RR: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LaneRole {
    VmHull,
    TokioBlocking,
}

impl LaneRole {
    const fn code(self) -> u8 {
        match self {
            Self::VmHull => LANE_VM_HULL,
            Self::TokioBlocking => LANE_TOKIO_BLOCKING,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct LaneProfile {
    pub role: LaneRole,
    pub placement: SpawnPlacement,
}

#[derive(Clone)]
struct LaneCandidate {
    slot: u32,
    core_kind: u8,
    spawner: SendSpawner,
}

pub struct LaneTarget {
    pub slot: u32,
    pub core_kind: u8,
    pub spawner: SendSpawner,
    pub lease: LaneLease,
}

impl LaneTarget {
    pub fn core_kind_name(&self) -> &'static str {
        match self.core_kind {
            crate::workers::CORE_KIND_PERF => "perf",
            crate::workers::CORE_KIND_EFF => "eff",
            _ => "unknown",
        }
    }

    pub const fn is_ap2_carrier_lane(&self) -> bool {
        self.slot >= AP2_FIRST_CARRIER_SLOT
    }
}

#[derive(Debug)]
pub struct LaneLease {
    slot: u32,
    role: LaneRole,
    armed: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LanePickError {
    MissingWorkerLane,
    MissingReservedVmLane,
    Busy,
}

impl LanePickError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingWorkerLane => "no AP2+ worker lanes are registered",
            Self::MissingReservedVmLane => "no AP2+ reserved VM lanes are registered",
            Self::Busy => "matching runtime carrier lanes are currently leased",
        }
    }
}

impl LaneLease {
    fn release(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(owner) = LANE_OWNER.get(self.slot as usize) {
            owner
                .compare_exchange(self.role.code(), LANE_FREE, Ordering::AcqRel, Ordering::Acquire)
                .ok();
        }
        self.armed = false;
    }
}

impl Drop for LaneLease {
    fn drop(&mut self) {
        self.release();
    }
}

pub fn pick_carrier_lane(profile: LaneProfile) -> Result<LaneTarget, LanePickError> {
    let missing_error = match profile.placement {
        SpawnPlacement::Worker => LanePickError::MissingWorkerLane,
        SpawnPlacement::ReservedVmLane => LanePickError::MissingReservedVmLane,
    };

    let pool = collect_candidates(profile);
    if pool.is_empty() {
        return Err(missing_error);
    }

    let start = LANE_RR.fetch_add(1, Ordering::Relaxed) as usize;
    for offset in 0..pool.len() {
        let candidate = &pool[(start + offset) % pool.len()];
        if let Some(lease) = try_lease(candidate.slot, profile.role) {
            return Ok(LaneTarget {
                slot: candidate.slot,
                core_kind: candidate.core_kind,
                spawner: candidate.spawner.clone(),
                lease,
            });
        }
    }

    Err(LanePickError::Busy)
}

pub fn pick_tokio_blocking_lane() -> Result<LaneTarget, LanePickError> {
    pick_carrier_lane(LaneProfile {
        role: LaneRole::TokioBlocking,
        placement: SpawnPlacement::Worker,
    })
}

pub fn pick_vm_hull_lane() -> Result<LaneTarget, LanePickError> {
    pick_carrier_lane(LaneProfile {
        role: LaneRole::VmHull,
        placement: SpawnPlacement::ReservedVmLane,
    })
}

fn collect_candidates(profile: LaneProfile) -> Vec<LaneCandidate> {
    match profile.placement {
        SpawnPlacement::Worker | SpawnPlacement::ReservedVmLane => collect_ap2_candidates(),
    }
}

fn collect_ap2_candidates() -> Vec<LaneCandidate> {
    let mut pool = Vec::new();
    for slot in crate::workers::background_slot_range() {
        if slot < AP2_FIRST_CARRIER_SLOT {
            continue;
        }
        let Some(spawner) = crate::workers::spawner_for_slot(slot) else {
            continue;
        };
        let core_kind = crate::workers::core_kind_for_slot(slot);
        pool.push(LaneCandidate {
            slot,
            core_kind,
            spawner,
        });
    }
    pool
}

fn try_lease(slot: u32, role: LaneRole) -> Option<LaneLease> {
    let owner = LANE_OWNER.get(slot as usize)?;
    owner
        .compare_exchange(LANE_FREE, role.code(), Ordering::AcqRel, Ordering::Acquire)
        .ok()?;
    Some(LaneLease {
        slot,
        role,
        armed: true,
    })
}

pub fn is_carrier_lane_free(slot: u32) -> bool {
    LANE_OWNER
        .get(slot as usize)
        .map(|owner| owner.load(Ordering::Acquire) == LANE_FREE)
        .unwrap_or(false)
}
