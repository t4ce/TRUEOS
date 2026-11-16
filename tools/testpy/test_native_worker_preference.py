#!/usr/bin/env python3
"""Test production service-lane selection, real exclusive leases and wire policy.

Uses the real carrier implementation and unchanged service picker/decoders.
The host registry and executor readiness are controlled; no rig is contacted.
"""

from pathlib import Path
import re
import subprocess
import tempfile

from test_carrier_core_preference import SOURCE


ROOT = Path(__file__).resolve().parents[2]


def item(source, signature):
    start = source.index(signature)
    end = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


HARNESS = r'''
mod native_service {
    use std::sync::{Mutex, atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}};
    use crate::hv::lane::{CorePreference, LaneLease};
    use crate::workers::{CORE_KIND_EFF as E, CORE_KIND_PERF as P, CORE_KIND_UNKNOWN as U};
    type BlockingJobFn = Box<dyn FnOnce() + Send + 'static>;
    static SERVICE_LANE_STARTED: [AtomicBool; 16] = [const { AtomicBool::new(false) }; 16];
    static SERVICE_LANE_RR: AtomicU64 = AtomicU64::new(0);
    static READY: [AtomicUsize; 16] = [const { AtomicUsize::new(0) }; 16];
    static SUBMITTED: Mutex<Option<(CorePreference, BlockingJobFn)>> = Mutex::new(None);
    fn start_service_lanes() {}
    fn service_lane_executor_counts(slot: u32) -> (usize, usize) {
        (1, READY[slot as usize].load(Ordering::Acquire))
    }
    fn submit_service_lane_job(job: BlockingJobFn, preference: CorePreference) -> i32 {
        assert!(SUBMITTED.lock().unwrap().replace((preference, job)).is_none());
        0
    }
    @PRODUCTION@

    fn reset(topology: &[(u32, u8)], unstarted: &[u32], busy_executor: &[u32]) {
        crate::workers::configure(topology);
        SERVICE_LANE_RR.store(0, Ordering::SeqCst);
        for slot in 0..16 {
            SERVICE_LANE_STARTED[slot].store(!unstarted.contains(&(slot as u32)), Ordering::SeqCst);
            READY[slot].store(usize::from(busy_executor.contains(&(slot as u32))), Ordering::SeqCst);
        }
    }
    fn preferred() -> Option<(u32, LaneLease)> {
        pick_service_lane_slot(CorePreference::PreferPerformance)
    }

    #[test] fn three_simultaneous_workers_lease_three_distinct_p_slots() {
        reset(&[(0,P), (1,P), (2,E), (3,P), (4,P), (5,P)], &[], &[]);
        let a = preferred().unwrap();
        let b = preferred().unwrap();
        let c = preferred().unwrap();
        assert_eq!([a.0, b.0, c.0], [3, 4, 5]);
        assert_eq!(preferred().unwrap().0, 2);
        drop((a,b,c));
        assert!(matches!(preferred(), Some((3..=5, _))));
    }
    #[test] fn soft_preference_falls_back_when_all_p_lanes_leased() {
        reset(&[(2,E), (4,P), (7,P), (9,U)], &[], &[]);
        let a = crate::hv::lane::try_lease_tokio_blocking_lane_for_slot(4).unwrap();
        let b = crate::hv::lane::try_lease_tokio_blocking_lane_for_slot(7).unwrap();
        assert_eq!(preferred().unwrap().0, 2);
        drop((a,b));
        assert!(matches!(preferred(), Some((4 | 7, _))));
    }
    #[test] fn idle_p_executor_wins_and_busy_p_still_precedes_idle_e() {
        reset(&[(2,E), (4,P), (7,P)], &[], &[4]);
        let idle = preferred().unwrap();
        assert_eq!(idle.0, 7);
        let busy = preferred().unwrap();
        assert_eq!(busy.0, 4);
        assert_eq!(preferred().unwrap().0, 2);
    }
    #[test] fn unstarted_p_is_ineligible_and_no_p_rotates_fallback_tier() {
        reset(&[(2,E), (4,P), (7,U), (9,E)], &[4], &[]);
        let slots: Vec<_> = (0..6).map(|_| preferred().unwrap().0).collect();
        assert_eq!(slots, [2,7,9,2,7,9]);
    }
    #[test] fn p_tier_round_robin_is_unbiased_by_interleaved_e_slots() {
        reset(&[(2,E), (3,U), (4,P), (5,E), (7,P), (8,E), (9,P)], &[], &[]);
        let slots: Vec<_> = (0..9).map(|_| preferred().unwrap().0).collect();
        assert_eq!(slots, [4,7,9,4,7,9,4,7,9]);
    }
    #[test] fn any_keeps_existing_idle_first_and_round_robin_policy() {
        reset(&[(2,E), (4,P), (7,U)], &[], &[]);
        let slots: Vec<_> = (0..6).map(|_| pick_service_lane_slot(CorePreference::Any).unwrap().0).collect();
        assert_eq!(slots, [2,4,7,2,4,7]);
        reset(&[(2,E), (4,P), (7,U)], &[], &[2]);
        assert_eq!(pick_service_lane_slot(CorePreference::Any).unwrap().0, 4);
    }
    #[test] fn no_free_or_registered_lane_reports_unavailable() {
        reset(&[(2,P), (4,E)], &[], &[]);
        let a = preferred().unwrap();
        let b = preferred().unwrap();
        assert!(preferred().is_none());
        drop((a,b));
        reset(&[], &[], &[]);
        assert!(preferred().is_none());
    }
    #[test] fn wire_preference_accepts_only_empty_or_known_le_word() {
        assert_eq!(service_lane_preference_from_payload(&[]), Some(CorePreference::Any));
        assert_eq!(service_lane_preference_from_payload(&0u32.to_le_bytes()), Some(CorePreference::Any));
        assert_eq!(service_lane_preference_from_payload(&1u32.to_le_bytes()), Some(CorePreference::PreferPerformance));
        for bad in [&[1][..], &[1,0][..], &[1,0,0][..], &[1,0,0,0,0][..], &[0,0,0,1][..], &[2,0,0,0][..]] {
            assert_eq!(service_lane_preference_from_payload(bad), None);
        }
    }
    #[test] fn invalid_preference_consumes_closure_without_execution_or_submission() {
        use std::sync::Arc;
        struct DropCount(Arc<AtomicUsize>);
        impl Drop for DropCount { fn drop(&mut self) { self.0.fetch_add(1,Ordering::SeqCst); } }
        let count = Arc::new(AtomicUsize::new(0));
        let captured = DropCount(count.clone());
        let rc = trueos_service_lane_submit_job_with_preference(Box::new(move || { drop(captured); panic!("invalid job ran"); }), 9);
        assert_eq!(rc, -5);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(SUBMITTED.lock().unwrap().is_none());
        let rc = trueos_service_lane_submit_job_with_preference(Box::new(|| {}), 1);
        assert_eq!(rc, 0);
        let (preference, job) = SUBMITTED.lock().unwrap().take().unwrap();
        assert_eq!(preference, CorePreference::PreferPerformance);
        job();
    }
}
'''


def main():
    source = (ROOT / "src/r/blocking.rs").read_text()
    production = "\n".join(item(source, signature) for signature in [
        "fn pick_service_lane_slot(", "fn pick_service_lane_from_tier(",
        "fn core_preference_from_raw(", "pub(crate) fn service_lane_preference_from_payload(",
        'pub extern "Rust" fn trueos_service_lane_submit_job_with_preference(',
    ])
    harness = SOURCE.replace("extern crate alloc;", "extern crate alloc;\nextern crate self as v;\n#[path=" + repr(str(ROOT / "crates/trueos-v/src/worker_abi.rs")).replace("'", '"') + "] pub mod worker_abi;")
    harness = harness.replace("    pub fn background_slot_range()", "    pub fn background_worker_slots() -> Vec<u32> { (2..16).filter(|slot| is_general_background_worker_slot(*slot)).collect() }\n    pub fn background_slot_range()")
    harness += HARNESS.replace("@PRODUCTION@", production)
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(), re.M).group(1)
    with tempfile.TemporaryDirectory(prefix="native-worker-preference-") as directory:
        path = Path(directory) / "test.rs"
        path.write_text(harness)
        binary = Path(directory) / "test"
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
