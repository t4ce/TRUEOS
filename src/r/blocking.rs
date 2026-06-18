extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub type BlockingJobFn = Box<dyn FnOnce() + Send + 'static>;

const BLOCKING_JOB_QUEUE_WARN_DEPTH: usize = 100;
const BLOCKING_JOB_QUEUE_CAP: usize = 4094;
const SERVICE_LANE_IDLE_POLL_MS: u64 = 10;
const SERVICE_LANE_BUSY_RETRY_MS: u64 = 1;
const SERVICE_LANE_SUPERVISOR_MS: u64 = 250;
const SERVICE_LANE_TASK_POOL: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;
const BLOCKING_JOB_TAG_HOST: &str = "host-blocking-job";
const BLOCKING_JOB_TAG_VMX: &str = "vmx-respect-architecture";
static NEXT_BLOCKING_JOB_ID: AtomicU64 = AtomicU64::new(1);
static SERVICE_LANE_RR: AtomicU64 = AtomicU64::new(0);
static SERVICE_LANE_STARTED: [AtomicBool; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { AtomicBool::new(false) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
static SERVICE_LANE_QUEUES: [Mutex<VecDeque<ServiceLaneRequest>>;
    crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { Mutex::new(VecDeque::new()) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
static SERVICE_LANE_WAITS: [crate::wait::WaitQueue; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { crate::wait::WaitQueue::new() }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

pub enum BlockingJobCall {
    Host(BlockingJobFn),
    GuestRaw { data: usize, vtable: usize },
}

pub struct BlockingJobEntry {
    pub id: u64,
    pub vm_id: Option<u8>,
    pub purpose: &'static str,
    pub policy_tag: &'static str,
    pub call: BlockingJobCall,
}

struct ServiceLaneRequest {
    entry: BlockingJobEntry,
    lease: crate::hv::lane::LaneLease,
}

pub fn queued_blocking_jobs() -> usize {
    SERVICE_LANE_QUEUES
        .iter()
        .map(|queue| queue.lock().len())
        .sum()
}

pub fn pop_blocking_job() -> Option<BlockingJobEntry> {
    for queue in SERVICE_LANE_QUEUES.iter() {
        if let Some(request) = queue.lock().pop_front() {
            return Some(request.entry);
        }
    }
    None
}

#[inline]
fn now_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

fn run_blocking_job_call(call: BlockingJobCall) {
    match call {
        BlockingJobCall::Host(job) => job(),
        BlockingJobCall::GuestRaw { data, vtable } => unsafe {
            let raw: *mut (dyn FnOnce() + Send + 'static) = core::mem::transmute((data, vtable));
            let job: BlockingJobFn = Box::from_raw(raw);
            job();
        },
    }
}

fn run_blocking_job_entry(entry: BlockingJobEntry) {
    let BlockingJobEntry {
        id,
        vm_id,
        purpose,
        policy_tag,
        call,
    } = entry;
    let started_ms = now_ms();
    crate::log_info!(
        target: "service";
        "blocking-job: run begin id={} vm={:?} purpose={} tag={}\n",
        id,
        vm_id,
        purpose,
        policy_tag
    );
    if let Some(vm_id) = vm_id {
        crate::r::kernel_task_domain::with(
            crate::r::kernel_task_domain::KernelTaskDomain::TokioCarrier,
            Some(vm_id),
            || run_blocking_job_call(call),
        );
    } else {
        crate::r::kernel_task_domain::with(
            crate::r::kernel_task_domain::KernelTaskDomain::HostService,
            None,
            || run_blocking_job_call(call),
        );
    }
    crate::log_info!(
        target: "service";
        "blocking-job: run done id={} vm={:?} purpose={} tag={} elapsed_ms={}\n",
        id,
        vm_id,
        purpose,
        policy_tag,
        now_ms().saturating_sub(started_ms)
    );
}

#[embassy_executor::task(pool_size = SERVICE_LANE_TASK_POOL)]
async fn service_lane_worker_task(slot: u32, core_kind: u8) {
    crate::log_info!(
        target: "service";
        "service-lane: worker start slot={} core_kind={}\n",
        slot,
        core_kind
    );

    loop {
        if service_lane_queue_depth(slot) == 0 {
            service_lane_wait(slot)
                .wait_for_event_timeout(SERVICE_LANE_IDLE_POLL_MS)
                .await;
            continue;
        }

        let Some(mut request) = pop_service_lane_request(slot) else {
            Timer::after(EmbassyDuration::from_millis(SERVICE_LANE_BUSY_RETRY_MS)).await;
            continue;
        };
        let entry = request.entry;
        let lease = &mut request.lease;
        if let Some(vm_id) = entry.vm_id {
            lease.set_vm_owner(vm_id);
        } else {
            lease.clear_vm_owner();
        }
        run_blocking_job_entry(entry);
        lease.clear_vm_owner();
    }
}

#[embassy_executor::task]
pub async fn blocking_job_dispatcher_task() {
    let spawned = start_service_lanes();
    crate::log_info!(
        target: "service";
        "service-lane: supervisor start spawned={}\n",
        spawned
    );
    loop {
        Timer::after(EmbassyDuration::from_millis(SERVICE_LANE_SUPERVISOR_MS)).await;
        start_service_lanes();
    }
}

pub fn start_service_lane_for_slot(slot: u32) -> bool {
    if !crate::workers::is_background_worker_slot(slot) {
        return false;
    }
    let Some(started) = SERVICE_LANE_STARTED.get(slot as usize) else {
        return false;
    };
    if started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }

    let Some(spawner) = crate::workers::spawner_for_slot(slot) else {
        started.store(false, Ordering::Release);
        return false;
    };
    let core_kind = crate::workers::core_kind_for_slot(slot);
    match service_lane_worker_task(slot, core_kind) {
        Ok(token) => {
            spawner.spawn(token);
            crate::log_info!(
                target: "service";
                "service-lane: spawned slot={} core_kind={}\n",
                slot,
                core_kind
            );
            true
        }
        Err(err) => {
            started.store(false, Ordering::Release);
            crate::log_error!(
                target: "service";
                "service-lane: spawn failed slot={} core_kind={} err={:?}\n",
                slot,
                core_kind,
                err
            );
            false
        }
    }
}

pub fn start_service_lanes() -> usize {
    crate::workers::background_worker_slots()
        .into_iter()
        .filter(|slot| start_service_lane_for_slot(*slot))
        .count()
}

pub fn service_lane_started_for_slot(slot: usize) -> bool {
    SERVICE_LANE_STARTED
        .get(slot)
        .map(|started| started.load(Ordering::Acquire))
        .unwrap_or(false)
}

fn service_lane_wait(slot: u32) -> &'static crate::wait::WaitQueue {
    SERVICE_LANE_WAITS
        .get(slot as usize)
        .unwrap_or(&SERVICE_LANE_WAITS[0])
}

fn service_lane_queue_depth(slot: u32) -> usize {
    SERVICE_LANE_QUEUES
        .get(slot as usize)
        .map(|queue| queue.lock().len())
        .unwrap_or(0)
}

fn pop_service_lane_request(slot: u32) -> Option<ServiceLaneRequest> {
    SERVICE_LANE_QUEUES
        .get(slot as usize)
        .and_then(|queue| queue.lock().pop_front())
}

fn pick_service_lane_slot() -> Option<(u32, crate::hv::lane::LaneLease)> {
    start_service_lanes();
    let slots = crate::workers::background_worker_slots();
    if slots.is_empty() {
        return None;
    }

    let start = SERVICE_LANE_RR.fetch_add(1, Ordering::Relaxed) as usize;
    for offset in 0..slots.len() {
        let slot = slots[(start + offset) % slots.len()];
        if SERVICE_LANE_STARTED
            .get(slot as usize)
            .map(|started| started.load(Ordering::Acquire))
            .unwrap_or(false)
            && let Some(lease) = crate::hv::lane::try_lease_tokio_blocking_lane_for_slot(slot)
        {
            return Some((slot, lease));
        }
    }
    None
}

fn submit_service_lane_request(entry: BlockingJobEntry) -> Result<u64, BlockingJobEntry> {
    if queued_blocking_jobs() >= BLOCKING_JOB_QUEUE_CAP {
        crate::log_error!(
            target: "service";
            "blocking-job: out of service-lane queue cap={} vm={:?} purpose={}\n",
            BLOCKING_JOB_QUEUE_CAP,
            entry.vm_id,
            entry.purpose
        );
        return Err(entry);
    }

    let Some((slot, lease)) = pick_service_lane_slot() else {
        crate::log_error!(
            target: "service";
            "blocking-job: no service lane available vm={:?} purpose={}\n",
            entry.vm_id,
            entry.purpose
        );
        return Err(entry);
    };

    let id = entry.id;
    let vm_id = entry.vm_id;
    let purpose = entry.purpose;
    let policy_tag = entry.policy_tag;
    let lane_depth = {
        let Some(queue) = SERVICE_LANE_QUEUES.get(slot as usize) else {
            return Err(entry);
        };
        let mut queue = queue.lock();
        queue.push_back(ServiceLaneRequest { entry, lease });
        queue.len()
    };
    let queued = queued_blocking_jobs();
    if queued > BLOCKING_JOB_QUEUE_WARN_DEPTH {
        crate::log_error!(
            target: "service";
            "blocking-job: backlog above safe depth id={} vm={:?} purpose={} tag={} queued={} safe_depth={} cap={} lane_slot={} lane_depth={}\n",
            id,
            vm_id,
            purpose,
            policy_tag,
            queued,
            BLOCKING_JOB_QUEUE_WARN_DEPTH,
            BLOCKING_JOB_QUEUE_CAP,
            slot,
            lane_depth
        );
    }
    crate::log_info!(
        target: "service";
        "blocking-job: queued id={} vm={:?} purpose={} tag={} queued={} cap={} lane_slot={} lane_depth={}\n",
        id,
        vm_id,
        purpose,
        policy_tag,
        queued,
        BLOCKING_JOB_QUEUE_CAP,
        slot,
        lane_depth
    );
    service_lane_wait(slot).notify_one();
    crate::remote_work_wake::wake_cpu_for_remote_work(slot);
    Ok(id)
}

fn enqueue_blocking_job(
    vm_id: Option<u8>,
    purpose: &'static str,
    call: BlockingJobCall,
) -> Result<u64, BlockingJobCall> {
    let id = NEXT_BLOCKING_JOB_ID.fetch_add(1, Ordering::AcqRel);
    let policy_tag = if vm_id.is_some() {
        BLOCKING_JOB_TAG_VMX
    } else {
        BLOCKING_JOB_TAG_HOST
    };
    let entry = BlockingJobEntry {
        id,
        vm_id,
        purpose,
        policy_tag,
        call,
    };
    submit_service_lane_request(entry).map_err(|entry| entry.call)
}

pub fn spawn_blocking_job_with_purpose(job: BlockingJobFn, purpose: &'static str) -> i32 {
    match enqueue_blocking_job(None, purpose, BlockingJobCall::Host(job)) {
        Ok(_) => 0,
        Err(_) => -2,
    }
}

pub unsafe fn submit_guest_service_lane_job_from_raw(
    vm_id: u8,
    data: usize,
    vtable: usize,
    purpose: &'static str,
) -> i32 {
    if data == 0 || vtable == 0 {
        return -5;
    }
    match enqueue_blocking_job(Some(vm_id), purpose, BlockingJobCall::GuestRaw { data, vtable }) {
        Ok(_) => 0,
        Err(_) => -2,
    }
}

pub unsafe fn spawn_vmx_thread_from_raw(
    vm_id: u8,
    data: usize,
    vtable: usize,
    purpose: &'static str,
) -> i32 {
    unsafe { submit_guest_service_lane_job_from_raw(vm_id, data, vtable, purpose) }
}

pub unsafe fn spawn_guest_blocking_job_from_raw(
    vm_id: u8,
    data: usize,
    vtable: usize,
    purpose: &'static str,
) -> i32 {
    unsafe { submit_guest_service_lane_job_from_raw(vm_id, data, vtable, purpose) }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_service_lane_submit_job(job: BlockingJobFn) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let raw = Box::into_raw(job);
        let (data, vtable): (usize, usize) = unsafe { core::mem::transmute(raw) };
        let (status, rc) = crate::hv::vmcall::guest_call(
            crate::hv::vmcall::OP_BP_SERVICE_LANE_SUBMIT,
            data as u64,
            vtable as u64,
        );
        if status == crate::hv::vmcall::STATUS_OK {
            rc as i32
        } else {
            -6
        }
    } else if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        match enqueue_blocking_job(
            Some(vm_id),
            "guest-tokio-blocking-job",
            BlockingJobCall::Host(job),
        ) {
            Ok(_) => 0,
            Err(_) => -2,
        }
    } else {
        spawn_blocking_job_with_purpose(job, "tokio-blocking-job")
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_spawn_blocking_job(job: BlockingJobFn) -> i32 {
    trueos_service_lane_submit_job(job)
}
