#!/usr/bin/env python3
"""Exercise production service-queue discard and retained native ownership.

Compile the real blocking-job prefix, queue publisher, and complete lifetime
module. Hardware/topology and diagnostics are mocked; payload execution,
ownership transitions, generation checks and registry locking are production.
"""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item

ROOT = Path(__file__).resolve().parents[2]

SUPPORT = r'''
#![allow(dead_code, unused_imports, unexpected_cfgs)]
extern crate alloc;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 8; pub const VM_CPU_SLOT_LIMIT: usize = 2; } }
mod embassy_time_driver { pub const TICK_HZ: u64 = 1000; pub fn now() -> u64 { 0 } }
mod trueos_time {
    pub struct Duration;
    impl Duration { pub fn from_millis(_: u64) -> Self { Self } }
    pub struct Timer;
    impl Timer { pub async fn after(_: Duration) {} }
}
mod hv {
    use super::*;
    pub static GENERATIONS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
    pub static KILLED: [AtomicBool; 8] = [const { AtomicBool::new(false) }; 8];
    pub fn vm_run_generation(vm: u8) -> Option<u64> { GENERATIONS.get(vm as usize).map(|value| value.load(Ordering::Acquire)) }
    pub fn guest_kill_requested(vm: u8) -> bool { KILLED[vm as usize].load(Ordering::Acquire) }
    pub mod lane {
        pub struct LaneLease;
        impl Drop for LaneLease {
            fn drop(&mut self) {
                crate::r::blocking::assert_registry_locks_free();
                crate::LEASE_DROPS.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
            }
        }
    }
}
mod workers {
    pub struct Spawner;
    impl Spawner { pub fn spawned_task_count(&self) -> usize { 0 } pub fn ready_task_count(&self) -> usize { 0 } }
    pub fn spawner_for_slot(_: u32) -> Option<Spawner> { Some(Spawner) }
}
mod wait {
    pub struct WaitQueue;
    impl WaitQueue { pub const fn new() -> Self { Self } pub fn notify_one(&self) {} }
}
mod remote_work_wake { pub fn wake_cpu_for_remote_work(_: u32) {} }
mod allocators {
    use super::*;
    pub static REALM_AVAILABLE: AtomicBool = AtomicBool::new(true);
    pub static KILL_ON_REALM_FAILURE: AtomicBool = AtomicBool::new(false);
    pub fn with_host_alloc_domain_strong<T>(call: impl FnOnce() -> T) -> T { call() }
    pub fn with_hv_guest_alloc_domain<T>(vm: u8, call: impl FnOnce() -> T) -> Option<T> {
        if REALM_AVAILABLE.load(Ordering::Acquire) { Some(call()) } else {
            if KILL_ON_REALM_FAILURE.swap(false, Ordering::AcqRel) {
                crate::r::blocking::force(vm);
                let generation = crate::hv::vm_run_generation(vm).unwrap();
                assert_eq!(crate::r::blocking::discard_guest_jobs(vm, generation).retained, 0);
            }
            None
        }
    }
}
mod log_os {
    pub mod flags { pub enum LogArea { Blueprint } }
    pub fn log_with_area_purpose(_: flags::LogArea, _: crate::log_os_core::LogLevel, _: Option<&str>, _: core::fmt::Arguments<'_>) {}
    pub fn blueprint_important_line(_: core::fmt::Arguments<'_>) {}
}
mod log_os_core { pub enum LogLevel { Info } }
#[macro_export] macro_rules! log_info { (target: $target:expr; $($tokens:tt)*) => { { let _ = format_args!($($tokens)*); } } }
#[macro_export] macro_rules! log_warn { (target: $target:expr; $($tokens:tt)*) => { { let _ = format_args!($($tokens)*); } } }
#[macro_export] macro_rules! log_error { (target: $target:expr; $($tokens:tt)*) => { { let _ = format_args!($($tokens)*); } } }
static LEASE_DROPS: AtomicUsize = AtomicUsize::new(0);
static CONTROL_WAKES: AtomicUsize = AtomicUsize::new(0);
static KILL_ON_LANE_PICK: AtomicBool = AtomicBool::new(false);
mod r {
    pub mod threads {
        pub fn wake_guest_control(_: u8) { crate::CONTROL_WAKES.fetch_add(1, core::sync::atomic::Ordering::AcqRel); }
    }
    pub mod kernel_task_domain {
        pub enum KernelTaskDomain { TokioCarrier, HostService }
        pub fn with<T>(_: KernelTaskDomain, _: Option<u8>, call: impl FnOnce() -> T) -> T { call() }
    }
    pub mod blocking {
        use crate::{embassy_time_driver, log_os_core};
        __PRODUCTION__
        fn service_lane_activity_begin(_: u32, _: u64, _: Option<u8>, _: &'static str) {}
        fn service_lane_activity_finish(_: u32) {}
        fn pick_service_lane_slot() -> Option<(u32, crate::hv::lane::LaneLease)> {
            if crate::KILL_ON_LANE_PICK.swap(false, Ordering::AcqRel) {
                force(0); discard_guest_jobs(0, crate::hv::vm_run_generation(0).unwrap());
            }
            Some((0, crate::hv::lane::LaneLease))
        }
        fn service_lane_wait(slot: u32) -> &'static crate::wait::WaitQueue { &SERVICE_LANE_WAITS[slot as usize] }
        pub fn assert_registry_locks_free() {
            for queue in &SERVICE_LANE_QUEUES { assert!(queue.try_lock().is_some(), "owner/lease dropped under queue lock"); }
            for jobs in &RETAINED_GUEST_JOBS { assert!(jobs.try_lock().is_some(), "owner dropped under retained registry lock"); }
        }
        pub fn force(vm: u8) {
            crate::hv::KILLED[vm as usize].store(true, core::sync::atomic::Ordering::Release);
            close_guest_jobs(vm);
        }
        #[cfg(test)] mod recovery_tests {
            use super::*;
            use alloc::sync::Arc;
            use core::sync::atomic::{AtomicUsize, Ordering};
            static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
            struct Capture(Arc<AtomicUsize>);
            impl Drop for Capture { fn drop(&mut self) { self.0.fetch_add(1, Ordering::AcqRel); } }
            fn open(vm: u8, generation: u64) {
                crate::hv::GENERATIONS[vm as usize].store(generation, Ordering::Release);
                crate::hv::KILLED[vm as usize].store(false, Ordering::Release);
                assert!(open_guest_jobs(vm, generation));
            }
            fn entry(vm: u8, id: u64, drops: &Arc<AtomicUsize>) -> BlockingJobEntry {
                let capture = Capture(drops.clone());
                let call = BlockingJobCall::Host(Box::new(move || { drop(capture); panic!("discarded guest closure executed"); }));
                BlockingJobEntry { id, vm_id: Some(vm), purpose: "test-native", policy_tag: "test", call,
                    owner: Some(lifetime::reserve(vm).unwrap()) }
            }
            fn queue(slot: usize, entry: BlockingJobEntry) {
                SERVICE_LANE_QUEUES[slot].lock().push_back(ServiceLaneRequest {
                    entry, lease: crate::hv::lane::LaneLease, enqueued_ms: 0, lane_depth_at_enqueue: 0,
                });
            }
            #[test]
            fn purge_bypasses_worker_and_keeps_other_vm_and_host_work() {
                let _serial = SERIAL.lock().unwrap();
                open(0, 41); open(1, 51);
                let drops = Arc::new(AtomicUsize::new(0));
                queue(0, BlockingJobEntry { id: 10, vm_id: None, purpose: "blocked-host", policy_tag: "host",
                    call: BlockingJobCall::Host(Box::new(|| {})), owner: None });
                queue(0, entry(0, 11, &drops)); queue(0, entry(1, 12, &drops)); queue(1, entry(0, 13, &drops));
                let lease_drops = crate::LEASE_DROPS.load(Ordering::Acquire);
                force(0);
                assert_eq!(discard_guest_jobs(0, 41), GuestDiscardSummary { queued: 2, retained: 0, stale_generation: false });
                assert_eq!(guest_jobs_in_flight(0), 0); assert_eq!(guest_jobs_in_flight(1), 1);
                assert_eq!(crate::LEASE_DROPS.load(Ordering::Acquire) - lease_drops, 2);
                assert_eq!(drops.load(Ordering::Acquire), 0, "guest Drop invoked during discard");
                assert_eq!(SERVICE_LANE_QUEUES[0].lock().iter().map(|r| r.entry.id).collect::<Vec<_>>(), [10, 12]);
                force(1); assert_eq!(discard_guest_jobs(1, 51).queued, 1);
                let host = SERVICE_LANE_QUEUES[0].lock().pop_front().unwrap(); drop(host);
            }
            #[test]
            fn generation_and_force_are_required_before_any_purge() {
                let _serial = SERIAL.lock().unwrap();
                open(0, 61); let drops = Arc::new(AtomicUsize::new(0)); queue(0, entry(0, 20, &drops));
                assert!(discard_guest_jobs(0, 61).stale_generation);
                force(0); assert!(discard_guest_jobs(0, 60).stale_generation);
                assert_eq!(guest_jobs_in_flight(0), 1);
                // Deliberately inject an impossible incarnation mismatch to
                // ensure the actual token, rather than only the VM tag, wins.
                crate::hv::GENERATIONS[0].store(62, Ordering::Release);
                assert_eq!(discard_guest_jobs(0, 62).queued, 0);
                assert_eq!(guest_jobs_in_flight(0), 1);
                crate::hv::GENERATIONS[0].store(61, Ordering::Release);
                assert_eq!(discard_guest_jobs(0, 61).queued, 1);
                assert_eq!(drops.load(Ordering::Acquire), 0);
            }
            #[test]
            fn accepted_before_kill_but_published_after_scan_is_discarded_once() {
                let _serial = SERIAL.lock().unwrap();
                open(0, 71); let drops = Arc::new(AtomicUsize::new(0)); let pending = entry(0, 30, &drops);
                force(0); assert_eq!(discard_guest_jobs(0, 71).queued, 0);
                assert_eq!(submit_service_lane_request(pending, false).ok(), Some(30));
                assert_eq!(guest_jobs_in_flight(0), 0); assert_eq!(queued_blocking_jobs(), 0);
                assert_eq!(drops.load(Ordering::Acquire), 0);
            }
            #[test]
            fn kill_between_lane_selection_and_queue_publication_is_discarded_once() {
                let _serial = SERIAL.lock().unwrap();
                open(0, 72); let drops = Arc::new(AtomicUsize::new(0)); let pending = entry(0, 31, &drops);
                crate::KILL_ON_LANE_PICK.store(true, Ordering::Release);
                let lease_drops = crate::LEASE_DROPS.load(Ordering::Acquire);
                assert_eq!(submit_service_lane_request(pending, false).ok(), Some(31));
                assert_eq!(guest_jobs_in_flight(0), 0); assert_eq!(queued_blocking_jobs(), 0);
                assert_eq!(crate::LEASE_DROPS.load(Ordering::Acquire) - lease_drops, 1);
                assert_eq!(drops.load(Ordering::Acquire), 0);
            }
            #[test]
            fn realm_failure_keeps_a_real_recoverable_reservation() {
                let _serial = SERIAL.lock().unwrap();
                open(0, 81); let drops = Arc::new(AtomicUsize::new(0));
                crate::allocators::REALM_AVAILABLE.store(false, Ordering::Release);
                run_blocking_job_entry(0, entry(0, 40, &drops));
                assert_eq!(guest_jobs_in_flight(0), 1); assert_eq!(retained_guest_jobs(0), 1);
                force(0); assert_eq!(discard_guest_jobs(0, 81).retained, 1);
                assert_eq!(guest_jobs_in_flight(0), 0); assert_eq!(retained_guest_jobs(0), 0);
                assert_eq!(drops.load(Ordering::Acquire), 0);
                crate::allocators::REALM_AVAILABLE.store(true, Ordering::Release);
            }
            #[test]
            fn realm_failure_after_kill_scan_does_not_create_a_late_orphan() {
                let _serial = SERIAL.lock().unwrap();
                open(0, 91); let drops = Arc::new(AtomicUsize::new(0));
                crate::allocators::REALM_AVAILABLE.store(false, Ordering::Release);
                crate::allocators::KILL_ON_REALM_FAILURE.store(true, Ordering::Release);
                run_blocking_job_entry(0, entry(0, 50, &drops));
                assert_eq!(guest_jobs_in_flight(0), 0); assert_eq!(retained_guest_jobs(0), 0);
                assert_eq!(drops.load(Ordering::Acquire), 0);
                crate::allocators::REALM_AVAILABLE.store(true, Ordering::Release);
            }
            #[test]
            fn owner_and_hull_acknowledgements_wake_the_collective_barrier() {
                let _serial = SERIAL.lock().unwrap();
                open(0, 101);
                let mut stopped = lifetime::reserve(0).unwrap();
                let executing = lifetime::reserve(0).unwrap();
                force(0); stopped.retire();
                assert!(!stopped.reclaimable());
                assert!(acknowledge_guest_hull_stopped(0, 101));
                assert!(!stopped.reclaimable());
                let wakes = crate::CONTROL_WAKES.load(Ordering::Acquire);
                drop(executing);
                assert!(stopped.reclaimable());
                assert!(crate::CONTROL_WAKES.load(Ordering::Acquire) > wakes);
                drop(stopped); assert_eq!(guest_jobs_in_flight(0), 0);
            }
            #[test]
            fn final_retirement_and_hull_acknowledgement_both_wake_waiters() {
                let _serial = SERIAL.lock().unwrap();
                open(0, 111);
                let mut first = lifetime::reserve(0).unwrap();
                let mut second = lifetime::reserve(0).unwrap();
                force(0); first.retire();
                let wakes = crate::CONTROL_WAKES.load(Ordering::Acquire);
                second.retire();
                assert!(!first.reclaimable());
                assert_eq!(crate::CONTROL_WAKES.load(Ordering::Acquire), wakes);
                assert!(acknowledge_guest_hull_stopped(0, 111));
                assert!(first.reclaimable());
                assert!(crate::CONTROL_WAKES.load(Ordering::Acquire) > wakes);
                drop(first); drop(second);
                open(0, 112);
                let mut first = lifetime::reserve(0).unwrap();
                let mut second = lifetime::reserve(0).unwrap();
                force(0); first.retire(); acknowledge_guest_hull_stopped(0, 112);
                let wakes = crate::CONTROL_WAKES.load(Ordering::Acquire);
                second.retire();
                assert!(first.reclaimable());
                assert!(crate::CONTROL_WAKES.load(Ordering::Acquire) > wakes);
                drop(first); drop(second);
            }
            #[test]
            fn concurrent_retained_publication_and_purge_leave_no_orphan() {
                let _serial = SERIAL.lock().unwrap();
                let drops = Arc::new(AtomicUsize::new(0));
                for generation in 200..264 {
                    open(0, generation);
                    let BlockingJobEntry { call, owner, .. } = entry(0, generation, &drops);
                    let barrier = Arc::new(std::sync::Barrier::new(2));
                    let publisher_barrier = barrier.clone();
                    let publisher = std::thread::spawn(move || {
                        publisher_barrier.wait();
                        retain_unstarted_guest_call(0, call, owner.unwrap());
                    });
                    barrier.wait(); force(0); discard_guest_jobs(0, generation);
                    publisher.join().unwrap();
                    assert_eq!(guest_jobs_in_flight(0), 0);
                    assert_eq!(retained_guest_jobs(0), 0);
                }
                assert_eq!(drops.load(Ordering::Acquire), 0);
            }
        }
    }
}
'''

DIAGNOSTICS = r'''
pub(crate) mod diagnostics {
    pub const RETAINED: usize = 5; pub const RUNNING: usize = 1;
    pub struct JobDiagnostic;
    impl JobDiagnostic {
        pub fn new(_: u8, _: u64) -> Self { Self }
        pub fn phase(&self, _: usize, _: usize, _: u64) {}
        pub fn identify(&self, _: &'static str, _: usize, _: usize) {}
    }
    impl Drop for JobDiagnostic { fn drop(&mut self) { super::assert_registry_locks_free(); } }
    pub fn report(_: u8) {}
}
'''


def main():
    path = ROOT / "src/r/blocking.rs"
    production = path.read_text().split("#[trueos_executor::task(pool_size = SERVICE_LANE_TASK_POOL)]")[0]
    production = production.replace("use trueos_time::", "use crate::trueos_time::")
    production = production.replace("pub(crate) mod diagnostics;", DIAGNOSTICS)
    lifetime = (ROOT / "src/r/blocking/lifetime.rs").read_text().replace("//!", "//")
    lifetime = lifetime.replace("trueos_time::", "crate::trueos_time::")
    production = production.replace("mod lifetime;", "mod lifetime {\n" + lifetime + "\n}")
    production += item(str(path), "submit_service_lane_request")
    source = SUPPORT.replace("__PRODUCTION__", production)
    with tempfile.TemporaryDirectory(prefix="trueos-native-recovery-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "Cargo.toml").write_text(
            '[package]\nname="native-recovery-tests"\nversion="0.1.0"\nedition="2024"\n'
            '[dependencies]\nspin="0.10"\nheapless="0.9"\n[workspace]\n'
        )
        (root / "src/lib.rs").write_text(source)
        subprocess.run([
            "cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--target-dir", str(ROOT / "bld/native-recovery-host-tests"), "--", "--test-threads=1",
        ], cwd=root, check=True)


if __name__ == "__main__":
    main()
