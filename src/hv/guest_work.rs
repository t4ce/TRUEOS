use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::SendSpawner;

use crate::r::spawn_spec::SpawnPlacement;

const VM_RESERVED_FIRST_SLOT: u32 = 2;

static GUEST_WORK_RR: AtomicU64 = AtomicU64::new(0);

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
    /// - never on the first AP service lane (`Ap1` / slot 1)
    /// - prefer AP>2 perf workers, with AP>2 fallback when needed
    ///
    /// This is the placement policy that future lane-indexed `vm[n]`
    /// scheduling must preserve as we scale past the current singleton path.
    pub const fn vm_default() -> Self {
        Self {
            role: VmLaneRole::VmHull,
            placement: SpawnPlacement::ReservedVmLane,
        }
    }

    /// Default placement for synchronous offload work that should stay off the
    /// BSP and AP1 service lane while still using disposable executor carriers.
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

    pub const fn role_name(self) -> &'static str {
        match self.role {
            VmLaneRole::VmHull => "vm-hull",
            VmLaneRole::TokioBlocking => "tokio-blocking",
            VmLaneRole::Worker => "worker",
            VmLaneRole::Service => "service",
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
            trueos_qjs::workers::CORE_KIND_PERF => "perf",
            trueos_qjs::workers::CORE_KIND_EFF => "eff",
            _ => "unknown",
        }
    }
}

pub type GuestWorkProfile = VmLaneProfile;
pub type GuestWorkTarget = VmLaneTarget;

pub fn pick_vm_lane_target(profile: VmLaneProfile) -> Option<VmLaneTarget> {
    match profile.placement {
        SpawnPlacement::ReservedVmLane => pick_reserved_vm_lane(),
        SpawnPlacement::Worker => pick_background_worker(),
        SpawnPlacement::Ap1 => pick_ap1_lane(),
        SpawnPlacement::Local => None,
    }
}

pub fn pick_guest_work_target(profile: GuestWorkProfile) -> Option<GuestWorkTarget> {
    pick_vm_lane_target(profile)
}

fn pick_ap1_lane() -> Option<VmLaneTarget> {
    let profile = crate::cpu::CpuProfile::for_slot(1)?;
    let spawner = trueos_qjs::workers::spawner_for_slot(profile.slot())?;
    Some(VmLaneTarget {
        slot: profile.slot(),
        core_kind: profile.core_kind(),
        spawner,
    })
}

fn pick_background_worker() -> Option<VmLaneTarget> {
    let slots = trueos_qjs::workers::background_worker_slots();
    if slots.is_empty() {
        return None;
    }

    let mut pool: Vec<VmLaneTarget> = Vec::new();
    for slot in slots {
        let Some(spawner) = trueos_qjs::workers::spawner_for_slot(slot) else {
            continue;
        };
        let profile = crate::cpu::CpuProfile::for_slot(slot);
        let core_kind = profile
            .map(|profile| profile.core_kind())
            .unwrap_or(trueos_qjs::workers::CORE_KIND_UNKNOWN);
        pool.push(VmLaneTarget {
            slot,
            core_kind,
            spawner,
        });
    }

    pick_round_robin(&pool)
}

fn pick_reserved_vm_lane() -> Option<VmLaneTarget> {
    let slots = trueos_qjs::workers::background_worker_slots();
    if slots.is_empty() {
        return None;
    }

    let mut perf: Vec<VmLaneTarget> = Vec::new();
    let mut fallback: Vec<VmLaneTarget> = Vec::new();

    for slot in slots {
        // Slot 0 is BSP/local and slot 1 is the first AP service lane.
        // HV guest execution must stay on reserved VM lanes at AP>2.
        if slot < VM_RESERVED_FIRST_SLOT {
            continue;
        }
        let Some(spawner) = trueos_qjs::workers::spawner_for_slot(slot) else {
            continue;
        };
        let profile = crate::cpu::CpuProfile::for_slot(slot);
        let core_kind = profile
            .map(|profile| profile.core_kind())
            .unwrap_or(trueos_qjs::workers::CORE_KIND_UNKNOWN);
        let target = VmLaneTarget {
            slot,
            core_kind,
            spawner,
        };
        if core_kind == trueos_qjs::workers::CORE_KIND_PERF {
            perf.push(target);
        } else {
            fallback.push(target);
        }
    }

    if !perf.is_empty() {
        pick_round_robin(&perf)
    } else {
        pick_round_robin(&fallback)
    }
}

fn pick_round_robin(pool: &[VmLaneTarget]) -> Option<VmLaneTarget> {
    if pool.is_empty() {
        return None;
    }
    let idx = GUEST_WORK_RR.fetch_add(1, Ordering::Relaxed) as usize;
    Some(pool[idx % pool.len()].clone())
}
