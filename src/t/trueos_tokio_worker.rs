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
    _carrier_lease: crate::hv::lane::LaneLease,
    guest_vm_id: Option<u8>,
    purpose: &'static str,
) {
    if !LOGGED_TASK_ENTER.swap(true, Ordering::AcqRel) {
        let tag = lane.tag();
        crate::log_info!(target: "service";
            "tokio-worker: entered {} lane{} cpu_slot={} core_kind={}\n",
            purpose,
            tag.lane_id,
            crate::percpu::current_slot(),
            tag.core_kind
        );
    }
    let _task_domain = crate::t::kernel_task_domain::enter(
        crate::t::kernel_task_domain::KernelTaskDomain::TokioCarrier,
        guest_vm_id,
    );
    // Keep guest-originated blocking work on host allocation by default. The
    // VM/vthread tag below is ownership/TLS identity, not heap ownership.
    let vthread_guard = if crate::t::th::vthread::tokio_blocking_backing_enabled() {
        if !LOGGED_VTHREAD_BACKING.swap(true, Ordering::AcqRel) {
            crate::log_info!(
                target: "service";
                "tokio-worker: vthread backing enabled for blocking workers\n"
            );
        }
        // Guest-originated blocking jobs run on a host carrier, but their TLS
        // identity must remain the VM hull identity.
        let record = guest_vm_id
            .map(crate::t::th::vthread::record_for_vm_hull)
            .unwrap_or_else(|| lane.vthread_record());
        Some(crate::t::th::vthread::enter(record))
    } else {
        None
    };
    let guard = crate::stackkeeper::enter_tokio_lane(lane, purpose);
    job();
    drop(guard);
    drop(vthread_guard);
    let _ = crate::stackkeeper::release_tokio_lane(lane);
    if !LOGGED_TASK_EXIT.swap(true, Ordering::AcqRel) {
        crate::log_info!(target: "service"; "tokio-worker: exited {}\n", purpose);
    }
}

fn reject_until_background_ap_ready() -> i32 {
    if !LOGGED_NO_WORKER.swap(true, Ordering::AcqRel) {
        crate::log_warn!(
            target: "service";
            "tokio-worker: no AP2+ background spawner yet; blocking job not launched\n"
        );
    }
    -2
}

fn spawn_on_background_ap(
    job: TokioBlockingJob,
    purpose: &'static str,
    guest_vm_id: Option<u8>,
) -> i32 {
    let carrier = match crate::hv::lane::pick_tokio_blocking_lane() {
        Ok(carrier) => carrier,
        Err(crate::hv::lane::LanePickError::MissingWorkerLane) => {
            let _ = job;
            return reject_until_background_ap_ready();
        }
        Err(error) => {
            let _ = job;
            if !LOGGED_NO_LANE.swap(true, Ordering::AcqRel) {
                crate::log_warn!(target: "service";
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

    let lane = if let Some(vm_id) = guest_vm_id {
        crate::stackkeeper::try_acquire_tokio_lane_for_vm(cpu_slot, core_kind, vm_id, purpose)
    } else {
        crate::stackkeeper::try_acquire_tokio_lane(cpu_slot, core_kind, purpose)
    };

    let Some(lane) = lane else {
        let _ = job;
        if !LOGGED_NO_LANE.swap(true, Ordering::AcqRel) {
            crate::log_warn!(target: "service";
                "tokio-worker: no free TRUEOS Tokio lane for cpu_slot={}; {} not launched\n",
                cpu_slot,
                purpose
            );
        }
        return -4;
    };

    let token = match tokio_blocking_job_task(job, lane, carrier.lease, guest_vm_id, purpose) {
        Ok(token) => token,
        Err(_) => {
            let _ = crate::stackkeeper::release_tokio_lane(lane);
            return -3;
        }
    };

    if !LOGGED_SPAWN.swap(true, Ordering::AcqRel) {
        let tag = lane.tag();
        crate::log_info!(target: "service";
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
        crate::log_info!(target: "service"; "tokio-worker: submitted {}\n", purpose);
    }
    0
}

pub fn spawn_blocking_job_with_purpose(
    job: Box<dyn FnOnce() + Send + 'static>,
    purpose: &'static str,
) -> i32 {
    spawn_on_background_ap(job, purpose, None)
}

pub unsafe fn spawn_guest_blocking_job_from_raw(
    vm_id: u8,
    data: usize,
    vtable: usize,
    purpose: &'static str,
) -> i32 {
    if data == 0 || vtable == 0 {
        return -5;
    }
    let raw: *mut (dyn FnOnce() + Send + 'static) = core::mem::transmute((data, vtable));
    let job = Box::from_raw(raw);
    spawn_on_background_ap(job, purpose, Some(vm_id))
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_spawn_blocking_job(job: TokioBlockingJob) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let raw = Box::into_raw(job);
        let (data, vtable): (usize, usize) = unsafe { core::mem::transmute(raw) };
        let (status, rc) = crate::hv::vmcall::guest_call(
            crate::hv::vmcall::OP_BP_TOKIO_BLOCKING_SPAWN,
            data as u64,
            vtable as u64,
        );
        return if status == crate::hv::vmcall::STATUS_OK {
            rc as i32
        } else {
            -6
        };
    }
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return spawn_on_background_ap(job, "guest-tokio-blocking-job", Some(vm_id));
    }
    spawn_blocking_job_with_purpose(job, "tokio-blocking-job")
}
