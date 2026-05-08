extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

type TokioBlockingJob = Box<dyn FnOnce() + Send + 'static>;

static LOGGED_NO_WORKER: AtomicBool = AtomicBool::new(false);
static LOGGED_NO_LANE: AtomicBool = AtomicBool::new(false);
static LOGGED_SPAWN: AtomicBool = AtomicBool::new(false);
static LOGGED_SUBMITTED: AtomicBool = AtomicBool::new(false);
static LOGGED_TASK_ENTER: AtomicBool = AtomicBool::new(false);
static LOGGED_TASK_EXIT: AtomicBool = AtomicBool::new(false);
static LOGGED_VTHREAD_BACKING: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task(pool_size = 64)]
async fn tokio_blocking_job_task(
    job: TokioBlockingJob,
    lane: crate::stackkeeper::TokioLaneLease,
    _carrier_lease: crate::r::lane::LaneLease,
    purpose: &'static str,
) {
    if !LOGGED_TASK_ENTER.swap(true, Ordering::AcqRel) {
        let tag = lane.tag();
        crate::log!(
            "tokio-worker: entered {} lane{} cpu_slot={} core_kind={}\n",
            purpose,
            tag.lane_id,
            crate::percpu::current_slot(),
            tag.core_kind
        );
    }
    let _vthread_guard = if crate::th::vthread::tokio_blocking_backing_enabled() {
        if !LOGGED_VTHREAD_BACKING.swap(true, Ordering::AcqRel) {
            crate::log!("tokio-worker: vthread backing enabled for blocking workers\n");
        }
        Some(crate::th::vthread::enter(lane.vthread_record()))
    } else {
        None
    };
    let _guard = crate::stackkeeper::enter_tokio_lane(lane, purpose);
    job();
    drop(_guard);
    drop(_vthread_guard);
    let _ = crate::stackkeeper::release_tokio_lane(lane);
    if !LOGGED_TASK_EXIT.swap(true, Ordering::AcqRel) {
        crate::log!("tokio-worker: exited {}\n", purpose);
    }
}

fn reject_until_background_ap_ready() -> i32 {
    if !LOGGED_NO_WORKER.swap(true, Ordering::AcqRel) {
        crate::log!("tokio-worker: no AP2+ background spawner yet; blocking job not launched\n");
    }
    -2
}

fn spawn_on_background_ap(job: TokioBlockingJob, purpose: &'static str) -> i32 {
    let carrier = match crate::r::lane::pick_tokio_blocking_lane() {
        Ok(carrier) => carrier,
        Err(crate::r::lane::LanePickError::MissingWorkerLane) => {
            let _ = job;
            return reject_until_background_ap_ready();
        }
        Err(error) => {
            let _ = job;
            if !LOGGED_NO_LANE.swap(true, Ordering::AcqRel) {
                crate::log!(
                    "tokio-worker: no TRUEOS runtime carrier lane for {}; reason={}\n",
                    purpose,
                    error.as_str()
                );
            }
            return -4;
        }
    };
    let cpu_slot = carrier.slot;
    let core_kind = carrier.core_kind;
    let spawner = carrier.spawner.clone();

    let Some(lane) = crate::stackkeeper::try_acquire_tokio_lane(cpu_slot, core_kind, purpose)
    else {
        let _ = job;
        if !LOGGED_NO_LANE.swap(true, Ordering::AcqRel) {
            crate::log!(
                "tokio-worker: no free TRUEOS Tokio lane for cpu_slot={}; {} not launched\n",
                cpu_slot,
                purpose
            );
        }
        return -4;
    };

    let token = match tokio_blocking_job_task(job, lane, carrier.lease, purpose) {
        Ok(token) => token,
        Err(_) => {
            let _ = crate::stackkeeper::release_tokio_lane(lane);
            return -3;
        }
    };

    if !LOGGED_SPAWN.swap(true, Ordering::AcqRel) {
        let tag = lane.tag();
        crate::log!(
            "tokio-worker: using TRUEOS AP2+ background spawners for {} tag=0x{:08X} vm={} domain={} role={} lane{} cpu_slot={} core_kind={} scratch={:#x}+{}\n",
            purpose,
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
    if purpose == "chat-http-runtime" {
        crate::lumen::burn_baby::protect_service_compute_slot(cpu_slot, purpose);
    }

    spawner.spawn(token);
    if !LOGGED_SUBMITTED.swap(true, Ordering::AcqRel) {
        crate::log!("tokio-worker: submitted {}\n", purpose);
    }
    0
}

pub fn spawn_blocking_job_with_purpose(
    job: Box<dyn FnOnce() + Send + 'static>,
    purpose: &'static str,
) -> i32 {
    spawn_on_background_ap(job, purpose)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_spawn_blocking_job(job: TokioBlockingJob) -> i32 {
    spawn_blocking_job_with_purpose(job, "tokio-blocking-job")
}
