#!/usr/bin/env python3
"""Exercise production carrier selection and leases with a host CPU registry."""

from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
SOURCE = r'''
#![allow(dead_code)]
extern crate alloc;

mod allcaps { pub mod hv { pub const VM_CPU_SLOT_LIMIT: usize = 16; } }
mod r { pub mod spawn_spec {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum SpawnPlacement { Worker, ReservedVmLane }
} }
mod wls {
    #[derive(Debug)] pub struct WorkerIdentityLease;
    pub struct WorkerIdentityGuard;
    impl WorkerIdentityLease {
        pub fn enter(&self) -> WorkerIdentityGuard { WorkerIdentityGuard }
    }
    pub fn try_lease_worker_identity(_: u32) -> Option<WorkerIdentityLease> {
        Some(WorkerIdentityLease)
    }
}
mod workers {
    use core::sync::atomic::{AtomicU8, Ordering};
    pub const CORE_KIND_UNKNOWN: u8 = 0;
    pub const CORE_KIND_PERF: u8 = 1;
    pub const CORE_KIND_EFF: u8 = 2;
    const ABSENT: u8 = u8::MAX;
    static KINDS: [AtomicU8; 16] = [const { AtomicU8::new(ABSENT) }; 16];
    #[derive(Clone, Copy)] pub struct WorkerSpawner { pub slot: u32 }
    pub fn configure(topology: &[(u32, u8)]) {
        for value in &KINDS { value.store(ABSENT, Ordering::SeqCst); }
        for &(slot, kind) in topology { KINDS[slot as usize].store(kind, Ordering::SeqCst); }
    }
    pub fn background_slot_range() -> core::ops::Range<u32> { 0..16 }
    pub fn is_general_background_worker_slot(slot: u32) -> bool {
        KINDS[slot as usize].load(Ordering::SeqCst) != ABSENT
    }
    pub fn spawner_for_slot(slot: u32) -> Option<WorkerSpawner> {
        is_general_background_worker_slot(slot).then_some(WorkerSpawner { slot })
    }
    pub fn core_kind_for_slot(slot: u32) -> u8 {
        KINDS[slot as usize].load(Ordering::SeqCst)
    }
}
mod hv {
    pub mod vpid {
        pub fn lane_reusable(_: usize) -> bool { true }
        pub fn lane_state_name(_: usize) -> &'static str { "reusable" }
    }
    pub fn hverrorf(_: core::fmt::Arguments<'_>) {}
    pub mod lane {
        include!(@LANE_PATH@);

        #[cfg(test)] mod tests {
            use super::*;
            use crate::workers::{CORE_KIND_EFF as E, CORE_KIND_PERF as P, CORE_KIND_UNKNOWN as U};

            fn reset(topology: &[(u32, u8)]) {
                for owner in &LANE_OWNER { owner.store(LANE_FREE, Ordering::SeqCst); }
                for owner in &LANE_VM_OWNER { owner.store(0, Ordering::SeqCst); }
                LANE_RR.store(0, Ordering::SeqCst);
                crate::workers::configure(topology);
            }
            fn profile() -> LaneProfile {
                LaneProfile { role: LaneRole::VmHull, placement: SpawnPlacement::ReservedVmLane }
            }
            fn preferred() -> LaneTarget {
                pick_carrier_lane_with_preference(profile(), CorePreference::PreferPerformance).unwrap()
            }

            #[test] fn free_performance_carrier_wins_over_earlier_efficiency_carrier() {
                reset(&[(0, P), (1, P), (2, E), (5, U), (7, P)]);
                let mut target = preferred();
                assert_eq!((target.slot, target.core_kind, target.spawner.slot), (7, P, 7));
                assert!(!is_carrier_lane_free(7));
                target.lease.set_vm_owner(3);
                assert_eq!(vm_owner_for_slot(7), Some(3));
                assert_eq!(role_for_slot(7), Some(LaneRole::VmHull));
                drop(target);
                assert!(is_carrier_lane_free(7));
                assert_eq!(vm_owner_for_slot(7), None);
            }

            #[test] fn busy_performance_carriers_fall_back_without_waiting() {
                reset(&[(2, E), (4, P), (7, P), (9, U)]);
                let first = try_lease(4, LaneRole::Worker).unwrap();
                let second = try_lease(7, LaneRole::TokioBlocking).unwrap();
                let fallback = preferred();
                assert_eq!(fallback.slot, 2);
                drop(fallback);
                drop(first);
                assert_eq!(preferred().slot, 4);
                drop(second);
            }

            #[test] fn no_performance_cores_keep_efficiency_and_unknown_cores_eligible() {
                reset(&[(2, E), (4, U), (8, E)]);
                let slots: Vec<_> = (0..6).map(|_| preferred().slot).collect();
                assert_eq!(slots, [2, 4, 8, 2, 4, 8]);
            }

            #[test] fn preference_rotates_evenly_within_interleaved_performance_tier() {
                reset(&[(2, E), (3, U), (4, P), (5, E), (7, P), (8, E), (9, P)]);
                let slots: Vec<_> = (0..9).map(|_| preferred().slot).collect();
                assert_eq!(slots, [4, 7, 9, 4, 7, 9, 4, 7, 9]);
            }

            #[test] fn existing_picker_keeps_unbiased_round_robin_and_skips_leases() {
                reset(&[(2, E), (4, P), (8, U)]);
                let slots: Vec<_> = (0..6)
                    .map(|_| pick_carrier_lane(profile()).unwrap().slot).collect();
                assert_eq!(slots, [2, 4, 8, 2, 4, 8]);
                let lease = try_lease(2, LaneRole::Worker).unwrap();
                assert_eq!(pick_carrier_lane(profile()).unwrap().slot, 4);
                drop(lease);
            }

            #[test] fn all_busy_and_empty_registry_preserve_errors() {
                reset(&[(2, E), (4, P)]);
                let first = preferred();
                let second = preferred();
                assert_eq!((first.slot, second.slot), (4, 2));
                assert_eq!(pick_carrier_lane_with_preference(profile(), CorePreference::PreferPerformance).err(), Some(LanePickError::Busy));
                drop((first, second));
                reset(&[(0, P), (1, P)]);
                assert_eq!(pick_carrier_lane_with_preference(profile(), CorePreference::PreferPerformance).err(), Some(LanePickError::MissingReservedVmLane));
                let worker = LaneProfile { role: LaneRole::Worker, placement: SpawnPlacement::Worker };
                assert_eq!(pick_carrier_lane(worker).err(), Some(LanePickError::MissingWorkerLane));
            }
        }
    }
}
'''.replace("@LANE_PATH@", '"' + str(ROOT / "src/hv/lane.rs") + '"')


def main():
    with tempfile.TemporaryDirectory(prefix="carrier-core-preference-") as directory:
        path = Path(directory) / "test.rs"
        path.write_text(SOURCE)
        binary = Path(directory) / "test"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)],
            check=True,
        )
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
