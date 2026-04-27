extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

type TokioBlockingJob = Box<dyn FnOnce() + Send + 'static>;

static LOGGED_NO_WORKER: AtomicBool = AtomicBool::new(false);
static LOGGED_NO_LANE: AtomicBool = AtomicBool::new(false);
static LOGGED_SPAWN: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task(pool_size = 64)]
async fn tokio_blocking_job_task(job: TokioBlockingJob, lane: crate::stackkeeper::TokioLaneLease) {
    let _guard = crate::stackkeeper::enter_tokio_lane(lane, "tokio-blocking-job");
    job();
    drop(_guard);
    let _ = crate::stackkeeper::release_tokio_lane(lane);
}

fn reject_until_background_ap_ready() -> i32 {
    if !LOGGED_NO_WORKER.swap(true, Ordering::AcqRel) {
        crate::log!("tokio-worker: no AP2+ background spawner yet; blocking job not launched\n");
    }
    -2
}

fn spawn_on_background_ap(job: TokioBlockingJob) -> i32 {
    let Some((cpu_slot, core_kind, spawner)) = crate::workers::pick_background_spawner_with_slot()
    else {
        let _ = job;
        return reject_until_background_ap_ready();
    };

    let Some(lane) =
        crate::stackkeeper::try_acquire_tokio_lane(cpu_slot, core_kind, "tokio-blocking-job")
    else {
        let _ = job;
        if !LOGGED_NO_LANE.swap(true, Ordering::AcqRel) {
            crate::log!(
                "tokio-worker: no free TRUEOS Tokio lane for cpu_slot={}; blocking job not launched\n",
                cpu_slot
            );
        }
        return -4;
    };

    let token = match tokio_blocking_job_task(job, lane) {
        Ok(token) => token,
        Err(_) => {
            let _ = crate::stackkeeper::release_tokio_lane(lane);
            return -3;
        }
    };

    if !LOGGED_SPAWN.swap(true, Ordering::AcqRel) {
        let tag = lane.tag();
        crate::log!(
            "tokio-worker: using TRUEOS AP2+ background spawners for blocking jobs tag=0x{:08X} vm={} domain={} role={} lane{} cpu_slot={} core_kind={} scratch={:#x}+{}\n",
            tag.magic,
            tag.vm_id,
            tag.domain,
            tag.role,
            tag.lane_id,
            tag.cpu_slot,
            tag.core_kind,
            tag.scratch_base,
            tag.scratch_len
        );
    }

    spawner.spawn(token);
    0
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_spawn_blocking_job(job: TokioBlockingJob) -> i32 {
    spawn_on_background_ap(job)
}
