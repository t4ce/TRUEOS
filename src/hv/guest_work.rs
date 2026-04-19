use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::SendSpawner;

use crate::r::spawn_spec::SpawnPlacement;

const VM_RESERVED_FIRST_SLOT: u32 = 2;

static GUEST_WORK_RR: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct GuestWorkProfile {
    pub placement: SpawnPlacement,
}

impl GuestWorkProfile {
    pub const fn vm_default() -> Self {
        Self {
            placement: SpawnPlacement::ReservedVmLane,
        }
    }
}

#[derive(Clone)]
pub struct GuestWorkTarget {
    pub slot: u32,
    pub core_kind: u8,
    pub spawner: SendSpawner,
}

impl GuestWorkTarget {
    pub fn core_kind_name(&self) -> &'static str {
        match self.core_kind {
            trueos_qjs::workers::CORE_KIND_PERF => "perf",
            trueos_qjs::workers::CORE_KIND_EFF => "eff",
            _ => "unknown",
        }
    }
}

pub fn pick_guest_work_target(profile: GuestWorkProfile) -> Option<GuestWorkTarget> {
    match profile.placement {
        SpawnPlacement::ReservedVmLane => pick_reserved_vm_lane(),
        SpawnPlacement::Worker => pick_background_worker(),
        SpawnPlacement::Ap1 => pick_ap1_lane(),
        SpawnPlacement::Local => None,
    }
}

fn pick_ap1_lane() -> Option<GuestWorkTarget> {
    let profile = crate::cpu::CpuProfile::for_slot(1)?;
    let spawner = trueos_qjs::workers::spawner_for_slot(profile.slot())?;
    Some(GuestWorkTarget {
        slot: profile.slot(),
        core_kind: profile.core_kind(),
        spawner,
    })
}

fn pick_background_worker() -> Option<GuestWorkTarget> {
    let slots = trueos_qjs::workers::background_worker_slots();
    if slots.is_empty() {
        return None;
    }

    let mut pool: Vec<GuestWorkTarget> = Vec::new();
    for slot in slots {
        let Some(spawner) = trueos_qjs::workers::spawner_for_slot(slot) else {
            continue;
        };
        let profile = crate::cpu::CpuProfile::for_slot(slot);
        let core_kind = profile
            .map(|profile| profile.core_kind())
            .unwrap_or(trueos_qjs::workers::CORE_KIND_UNKNOWN);
        pool.push(GuestWorkTarget {
            slot,
            core_kind,
            spawner,
        });
    }

    pick_round_robin(&pool)
}

fn pick_reserved_vm_lane() -> Option<GuestWorkTarget> {
    let slots = trueos_qjs::workers::background_worker_slots();
    if slots.is_empty() {
        return None;
    }

    let mut perf: Vec<GuestWorkTarget> = Vec::new();
    let mut fallback: Vec<GuestWorkTarget> = Vec::new();

    for slot in slots {
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
        let target = GuestWorkTarget {
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

fn pick_round_robin(pool: &[GuestWorkTarget]) -> Option<GuestWorkTarget> {
    if pool.is_empty() {
        return None;
    }
    let idx = GUEST_WORK_RR.fetch_add(1, Ordering::Relaxed) as usize;
    Some(pool[idx % pool.len()].clone())
}
