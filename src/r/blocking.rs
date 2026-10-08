extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::r::compute_budget::{self, CooldownDebt};
use crate::workers::ComputeWorkerPolicy;
use heapless::String as HString;
use spin::Mutex;
use trueos_time::{Duration as EmbassyDuration, Timer};

pub type BlockingJobFn = Box<dyn FnOnce() + Send + 'static>;
type GuestComputeJob = Box<dyn FnMut() -> bool + Send + 'static>;

mod lifetime;
pub(crate) mod diagnostics;
pub(crate) use lifetime::reserve as reserve_guest_job;
pub(crate) use lifetime::{
    GuestJobOwner, close_guest_jobs, drain_guest_jobs, guest_jobs_in_flight, open_guest_jobs,
};

const BLOCKING_JOB_QUEUE_WARN_DEPTH: usize = 100;
const BLOCKING_JOB_QUEUE_CAP: usize = 4094;
const SERVICE_LANE_IDLE_POLL_MS: u64 = 10;
const SERVICE_LANE_BUSY_RETRY_MS: u64 = 1;
const SERVICE_LANE_SUPERVISOR_MS: u64 = 250;
const SERVICE_LANE_QUEUE_WAIT_WARN_MS: u64 = 100;
const SERVICE_LANE_SOFT_THROTTLE_DEPTH: usize = 64;
const SERVICE_LANE_TASK_POOL: usize = crate::allcaps::hv::VM_CPU_SLOT_LIMIT;
const BLOCKING_JOB_TAG_HOST: &str = "host-blocking-job";
const BLOCKING_JOB_TAG_VMX: &str = "vmx-respect-architecture";
const SERVICE_LANE_PTHREAD_NAME_CAPACITY: usize = 15;
static NEXT_BLOCKING_JOB_ID: AtomicU64 = AtomicU64::new(1);
static SERVICE_LANE_RR: AtomicU64 = AtomicU64::new(0);
static SERVICE_LANE_STARTED: [AtomicBool; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { AtomicBool::new(false) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
static SERVICE_LANE_QUEUES: [Mutex<VecDeque<ServiceLaneRequest>>;
    crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { Mutex::new(VecDeque::new()) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
static SERVICE_LANE_WAITS: [crate::wait::WaitQueue; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { crate::wait::WaitQueue::new() }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
static SERVICE_LANE_ACTIVITY: [Mutex<ServiceLaneActivity>; crate::allcaps::hv::VM_CPU_SLOT_LIMIT] =
    [const { Mutex::new(ServiceLaneActivity::new()) }; crate::allcaps::hv::VM_CPU_SLOT_LIMIT];

const XPAPP_COMPUTE_WORKERS: usize = 2;
const XPAPP_COMPUTE_QUEUE_CAP: usize = 64;
const XPAPP_COMPUTE_IDLE_GRACE_MS: u64 = 250;
const XPAPP_COMPUTE_NO_SLOT: u32 = u32::MAX;

struct XpappComputeEntry {
    vm_id: u8,
    // This is `None` before the entry itself drops: guest code and its
    // allocation must be destroyed while the VM allocation realm is live.
    job: Option<GuestComputeJob>,
    owner: GuestJobOwner,
}

struct XpappComputeQueue {
    slot: AtomicU32,
    started: AtomicBool,
    in_flight: AtomicU32,
    jobs: Mutex<VecDeque<XpappComputeEntry>>,
    wait: crate::wait::WaitQueue,
}

impl XpappComputeQueue {
    const fn new() -> Self {
        Self {
            slot: AtomicU32::new(XPAPP_COMPUTE_NO_SLOT),
            started: AtomicBool::new(false),
            in_flight: AtomicU32::new(0),
            jobs: Mutex::new(VecDeque::new()),
            wait: crate::wait::WaitQueue::new(),
        }
    }
}

// Serializes worker start/idle-retire with enqueue. It prevents a band from
// being added just after the worker decides its queue is empty.
static XPAPP_COMPUTE_CONTROL: Mutex<()> = Mutex::new(());
static XPAPP_COMPUTE_QUEUE_RR: AtomicU32 = AtomicU32::new(0);
static XPAPP_COMPUTE_UNAVAILABLE_LOGGED: AtomicBool = AtomicBool::new(false);
static XPAPP_COMPUTE_QUEUES: [XpappComputeQueue; XPAPP_COMPUTE_WORKERS] =
    [const { XpappComputeQueue::new() }; XPAPP_COMPUTE_WORKERS];

struct ServiceLaneActivity {
    active_id: u64,
    active_vm_id: Option<u8>,
    active_purpose: &'static str,
    active_pthread_name: HString<SERVICE_LANE_PTHREAD_NAME_CAPACITY>,
    recent_id: u64,
    recent_vm_id: Option<u8>,
    recent_purpose: &'static str,
    recent_pthread_name: HString<SERVICE_LANE_PTHREAD_NAME_CAPACITY>,
    recent_completed_ms: u64,
}

impl ServiceLaneActivity {
    const fn new() -> Self {
        Self {
            active_id: 0,
            active_vm_id: None,
            active_purpose: "",
            active_pthread_name: HString::new(),
            recent_id: 0,
            recent_vm_id: None,
            recent_purpose: "",
            recent_pthread_name: HString::new(),
            recent_completed_ms: 0,
        }
    }
}

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
    // Dropped only after the closure has finished (or enqueue was rejected).
    owner: Option<lifetime::GuestJobOwner>,
}

struct ServiceLaneRequest {
    entry: BlockingJobEntry,
    lease: crate::hv::lane::LaneLease,
    enqueued_ms: u64,
    lane_depth_at_enqueue: usize,
}

pub fn queued_blocking_jobs() -> usize {
    SERVICE_LANE_QUEUES
        .iter()
        .map(|queue| queue.lock().len())
        .sum()
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

fn service_lane_executor_counts(slot: u32) -> (usize, usize) {
    crate::workers::spawner_for_slot(slot)
        .map(|spawner| (spawner.spawned_task_count(), spawner.ready_task_count()))
        .unwrap_or((0, 0))
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

fn run_blocking_job_entry(slot: u32, entry: BlockingJobEntry) {
    let BlockingJobEntry {
        id,
        vm_id,
        purpose,
        policy_tag,
        call,
        owner,
    } = entry;
    if vm_id.is_some_and(crate::hv::guest_kill_requested) {
        core::mem::forget(call);
        drop(owner);
        return;
    }
    let started_ms = now_ms();
    if let Some(owner) = &owner {
        owner.diagnostic.identify(purpose, id as usize, slot as usize);
        owner.diagnostic.phase(diagnostics::RUNNING, 0, 0);
    }
    let trace_each_job = purpose != "vmx-service-lane";
    service_lane_activity_begin(slot, id, vm_id, purpose);
    if trace_each_job {
        crate::log_info!(
            target: "service";
            "blocking-job: run begin id={} vm={:?} purpose={} tag={}\n",
            id,
            vm_id,
            purpose,
            policy_tag
        );
    }
    if let Some(vm_id) = vm_id {
        if trace_each_job {
            crate::log_os::log_with_area_purpose(
                crate::log_os::flags::LogArea::Blueprint,
                log_os_core::LogLevel::Info,
                Some("multi-rt-alloc"),
                format_args!(
                    "guest service job begin id={} vm={} purpose={} alloc_domain=hv-guest\n",
                    id, vm_id, purpose
                ),
            );
        }
        let mut pending_call = Some(call);
        let ran = crate::r::kernel_task_domain::with(
            crate::r::kernel_task_domain::KernelTaskDomain::TokioCarrier,
            Some(vm_id),
            || {
                crate::allocators::with_hv_guest_alloc_domain(vm_id, || {
                    run_blocking_job_call(pending_call.take().expect("native job consumed once"))
                })
                .is_some()
            },
        );
        if !ran {
            // Do not invoke a guest destructor in the host allocation realm,
            // or publish its executable memory as reusable after losing the
            // realm. Retain the reservation for diagnosis/recovery.
            if let Some(owner) = &owner {
                owner.diagnostic.phase(diagnostics::RETAINED, 0, 0);
            }
            core::mem::forget(pending_call);
            core::mem::forget(owner);
            crate::log_error!(target: "service";
                "blocking-job: guest allocation domain unavailable id={} vm={} resources=retained\n", id, vm_id);
            service_lane_activity_finish(slot);
            return;
        }
        if trace_each_job {
            crate::log_os::log_with_area_purpose(
                crate::log_os::flags::LogArea::Blueprint,
                log_os_core::LogLevel::Info,
                Some("multi-rt-alloc"),
                format_args!("guest service job done id={} vm={} purpose={}\n", id, vm_id, purpose),
            );
        }
    } else {
        crate::r::kernel_task_domain::with(
            crate::r::kernel_task_domain::KernelTaskDomain::HostService,
            None,
            || run_blocking_job_call(call),
        );
    }
    if trace_each_job {
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
    service_lane_activity_finish(slot);
    drop(owner);
}

#[trueos_executor::task(pool_size = SERVICE_LANE_TASK_POOL)]
async fn service_lane_executor_task(slot: u32, core_kind: u8) {
    crate::log_info!(
        target: "service";
        "service-lane: TRUEOS executor task start slot={} core_kind={}\n",
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

        let Some(request) = pop_service_lane_request(slot) else {
            Timer::after(EmbassyDuration::from_millis(SERVICE_LANE_BUSY_RETRY_MS)).await;
            continue;
        };
        let ServiceLaneRequest {
            entry,
            mut lease,
            enqueued_ms,
            lane_depth_at_enqueue,
        } = request;
        let purpose = entry.purpose;
        let queue_wait_ms = now_ms().saturating_sub(enqueued_ms);
        let (spawned_tasks, ready_tasks) = service_lane_executor_counts(slot);
        if queue_wait_ms >= SERVICE_LANE_QUEUE_WAIT_WARN_MS {
            crate::log_warn!(target: "service";
                "service-lane: queued job waited id={} vm={:?} purpose={} lane_slot={} wait_ms={} lane_depth_at_enqueue={} spawned_tasks={} ready_tasks={}\n",
                entry.id,
                entry.vm_id,
                purpose,
                slot,
                queue_wait_ms,
                lane_depth_at_enqueue,
                spawned_tasks,
                ready_tasks
            );
        }
        if let Some(vm_id) = entry.vm_id {
            lease.set_vm_owner(vm_id);
        } else {
            lease.clear_vm_owner();
        }
        let wls_guard = lease.enter_wls();
        run_blocking_job_entry(slot, entry);
        drop(wls_guard);
        lease.clear_vm_owner();
    }
}

#[trueos_executor::task]
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
    if !crate::workers::is_general_background_worker_slot(slot) {
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
    match service_lane_executor_task(slot, core_kind) {
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

fn copy_pthread_name(destination: &mut HString<SERVICE_LANE_PTHREAD_NAME_CAPACITY>, name: &str) {
    destination.clear();
    for ch in name.chars() {
        if destination.push(ch).is_err() {
            break;
        }
    }
}

fn service_lane_activity_begin(slot: u32, id: u64, vm_id: Option<u8>, purpose: &'static str) {
    let Some(activity) = SERVICE_LANE_ACTIVITY.get(slot as usize) else {
        return;
    };
    let mut activity = activity.lock();
    activity.active_id = id;
    activity.active_vm_id = vm_id;
    activity.active_purpose = purpose;
    activity.active_pthread_name.clear();
}

fn service_lane_activity_finish(slot: u32) {
    let Some(activity) = SERVICE_LANE_ACTIVITY.get(slot as usize) else {
        return;
    };
    let mut activity = activity.lock();
    activity.recent_id = activity.active_id;
    activity.recent_vm_id = activity.active_vm_id;
    activity.recent_purpose = activity.active_purpose;
    activity.recent_pthread_name = activity.active_pthread_name.clone();
    activity.recent_completed_ms = now_ms();
    activity.active_id = 0;
    activity.active_vm_id = None;
    activity.active_purpose = "";
    activity.active_pthread_name.clear();
}

pub fn set_current_service_lane_pthread_name(name: &str) {
    crate::r::threads::set_current_name(name);
    let slot = crate::percpu::current_slot();
    let Some(activity) = SERVICE_LANE_ACTIVITY.get(slot) else {
        return;
    };
    let mut activity = activity.lock();
    if activity.active_id != 0 {
        copy_pthread_name(&mut activity.active_pthread_name, name);
    }
}

pub fn service_lane_activity_text(slot: usize) -> Option<alloc::string::String> {
    if !service_lane_started_for_slot(slot) {
        return None;
    }
    let queue_depth = service_lane_queue_depth(slot as u32);
    let activity = SERVICE_LANE_ACTIVITY.get(slot)?.lock();
    if activity.active_id != 0 {
        let identity = if activity.active_pthread_name.is_empty() {
            alloc::format!("purpose={}", activity.active_purpose)
        } else if activity.active_purpose.contains("tokio")
            || activity.active_pthread_name.starts_with("tokio-")
        {
            alloc::format!("Tokio worker={}", activity.active_pthread_name)
        } else {
            alloc::format!("logical std thread={}", activity.active_pthread_name)
        };
        return Some(alloc::format!(
            "active#{} vm={} {} q={}",
            activity.active_id,
            activity
                .active_vm_id
                .map(|vm_id| alloc::format!("vm{vm_id}"))
                .unwrap_or_else(|| alloc::string::String::from("host")),
            identity,
            queue_depth,
        ));
    }
    if activity.recent_id != 0 {
        let identity = if activity.recent_pthread_name.is_empty() {
            alloc::format!("purpose={}", activity.recent_purpose)
        } else if activity.recent_purpose.contains("tokio")
            || activity.recent_pthread_name.starts_with("tokio-")
        {
            alloc::format!("Tokio worker={}", activity.recent_pthread_name)
        } else {
            alloc::format!("logical std thread={}", activity.recent_pthread_name)
        };
        return Some(alloc::format!(
            "idle q={} recent#{} vm={} {} age={}ms",
            queue_depth,
            activity.recent_id,
            activity
                .recent_vm_id
                .map(|vm_id| alloc::format!("vm{vm_id}"))
                .unwrap_or_else(|| alloc::string::String::from("host")),
            identity,
            now_ms().saturating_sub(activity.recent_completed_ms),
        ));
    }
    Some(alloc::format!("idle q={queue_depth}"))
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
    // A leased lane is not necessarily an executor that can run promptly: a
    // cooperative workload may already have several ready tasks on that AP.
    // Prefer a started executor with no ready work, then retain the original
    // round-robin scan as a capacity fallback when every executor is busy.
    for idle_executor_only in [true, false] {
        for offset in 0..slots.len() {
            let slot = slots[(start + offset) % slots.len()];
            if !SERVICE_LANE_STARTED
                .get(slot as usize)
                .map(|started| started.load(Ordering::Acquire))
                .unwrap_or(false)
                || (idle_executor_only && service_lane_executor_counts(slot).1 != 0)
            {
                continue;
            }
            if let Some(lease) = crate::hv::lane::try_lease_tokio_blocking_lane_for_slot(slot) {
                return Some((slot, lease));
            }
        }
    }
    None
}

fn submit_service_lane_request(
    entry: BlockingJobEntry,
    log_rejection: bool,
) -> Result<u64, BlockingJobEntry> {
    if queued_blocking_jobs() >= BLOCKING_JOB_QUEUE_CAP {
        if log_rejection {
            crate::log_error!(
                target: "service";
                "blocking-job: out of service-lane queue cap={} vm={:?} purpose={}\n",
                BLOCKING_JOB_QUEUE_CAP,
                entry.vm_id,
                entry.purpose
            );
        }
        return Err(entry);
    }

    let Some((slot, lease)) = pick_service_lane_slot() else {
        if log_rejection {
            crate::log_error!(
                target: "service";
                "blocking-job: no service lane available vm={:?} purpose={}\n",
                entry.vm_id,
                entry.purpose
            );
        }
        return Err(entry);
    };

    let id = entry.id;
    let vm_id = entry.vm_id;
    let purpose = entry.purpose;
    let policy_tag = entry.policy_tag;
    let enqueued_ms = now_ms();
    let lane_depth = {
        let Some(queue) = SERVICE_LANE_QUEUES.get(slot as usize) else {
            return Err(entry);
        };
        let mut queue = queue.lock();
        let lane_depth_at_enqueue = queue.len().saturating_add(1);
        queue.push_back(ServiceLaneRequest {
            entry,
            lease,
            enqueued_ms,
            lane_depth_at_enqueue,
        });
        queue.len()
    };
    let queued = queued_blocking_jobs();
    let (spawned_tasks, ready_tasks) = service_lane_executor_counts(slot);
    if lane_depth >= SERVICE_LANE_SOFT_THROTTLE_DEPTH {
        crate::log_warn!(
            target: "service";
            "blocking-job: service-lane soft throttle id={} vm={:?} purpose={} tag={} lane_slot={} lane_depth={} soft_depth={} queued={} spawned_tasks={} ready_tasks={}\n",
            id,
            vm_id,
            purpose,
            policy_tag,
            slot,
            lane_depth,
            SERVICE_LANE_SOFT_THROTTLE_DEPTH,
            queued,
            spawned_tasks,
            ready_tasks
        );
    } else if queued > BLOCKING_JOB_QUEUE_WARN_DEPTH {
        crate::log_error!(
            target: "service";
            "blocking-job: backlog above safe depth id={} vm={:?} purpose={} tag={} queued={} safe_depth={} cap={} lane_slot={} lane_depth={} spawned_tasks={} ready_tasks={}\n",
            id,
            vm_id,
            purpose,
            policy_tag,
            queued,
            BLOCKING_JOB_QUEUE_WARN_DEPTH,
            BLOCKING_JOB_QUEUE_CAP,
            slot,
            lane_depth,
            spawned_tasks,
            ready_tasks
        );
    }
    crate::log_info!(
        target: "service";
        "blocking-job: queued id={} vm={:?} purpose={} tag={} queued={} cap={} lane_slot={} lane_depth={} spawned_tasks={} ready_tasks={}\n",
        id,
        vm_id,
        purpose,
        policy_tag,
        queued,
        BLOCKING_JOB_QUEUE_CAP,
        slot,
        lane_depth,
        spawned_tasks,
        ready_tasks
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
    enqueue_blocking_job_with_rejection_policy(vm_id, purpose, call, true)
}

fn enqueue_blocking_job_with_rejection_policy(
    vm_id: Option<u8>,
    purpose: &'static str,
    call: BlockingJobCall,
    log_rejection: bool,
) -> Result<u64, BlockingJobCall> {
    let owner = if let Some(vm_id) = vm_id {
        let Some(owner) = lifetime::reserve(vm_id) else {
            return Err(call);
        };
        Some(owner)
    } else {
        None
    };
    let id = NEXT_BLOCKING_JOB_ID.fetch_add(1, Ordering::AcqRel);
    if let Some(owner) = &owner {
        owner.diagnostic.identify(purpose, id as usize, usize::MAX);
    }
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
        owner,
    };
    submit_service_lane_request(entry, log_rejection).map_err(|entry| entry.call)
}

pub fn spawn_blocking_job_with_purpose(job: BlockingJobFn, purpose: &'static str) -> i32 {
    match enqueue_blocking_job(None, purpose, BlockingJobCall::Host(job)) {
        Ok(_) => 0,
        Err(_) => -2,
    }
}

/// Submit host work while preserving ownership when no leased service lane is
/// currently available. Kernel service controllers use this to park and retry
/// instead of dropping an accepted request.
pub fn try_spawn_blocking_job_with_purpose(
    job: BlockingJobFn,
    purpose: &'static str,
) -> Result<(), BlockingJobFn> {
    enqueue_blocking_job_with_rejection_policy(None, purpose, BlockingJobCall::Host(job), false)
        .map(|_| ())
        .map_err(|call| match call {
            BlockingJobCall::Host(job) => job,
            BlockingJobCall::GuestRaw { .. } => unreachable!(),
        })
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub unsafe fn spawn_vmx_thread_from_raw(
    vm_id: u8,
    data: usize,
    vtable: usize,
    purpose: &'static str,
) -> i32 {
    unsafe { submit_guest_service_lane_job_from_raw(vm_id, data, vtable, purpose) }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
        let result = if status == crate::hv::vmcall::STATUS_OK {
            rc as i32
        } else {
            -6
        };
        if result != 0 {
            // Ownership crosses to the service lane only after a successful
            // enqueue. Reclaim a rejected raw guest closure in its original
            // allocation realm.
            unsafe { drop(Box::from_raw(raw)) };
        }
        result
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

fn xpapp_compute_claim_slot(
    slot: u32,
) -> Option<(
    crate::workers::ComputeWorkerLease,
    crate::hv::lane::LaneLease,
    crate::wls::WorkerIdentityLease,
)> {
    let worker = crate::workers::try_claim_compute_worker_on_slot(
        slot,
        ComputeWorkerPolicy::PerformanceOnly,
    )?;
    let lane = crate::hv::lane::try_lease_guest_compute_lane_for_slot(slot)?;
    let identity = crate::wls::try_lease_worker_identity(slot)?;
    Some((worker, lane, identity))
}

fn xpapp_compute_start_worker(index: usize) -> bool {
    let Some(queue) = XPAPP_COMPUTE_QUEUES.get(index) else {
        return false;
    };
    if queue.started.load(Ordering::Acquire) {
        return true;
    }
    let remembered = queue.slot.load(Ordering::Acquire);
    // Preserve the previous slot when it is usable. If another persistent
    // carrier owns it after idle retirement, scan the remaining P workers
    // before declaring the pool unavailable.
    let claimed = (remembered != XPAPP_COMPUTE_NO_SLOT)
        .then(|| xpapp_compute_claim_slot(remembered))
        .flatten()
        .or_else(|| {
            crate::workers::background_worker_slots()
                .into_iter()
                .filter(|slot| *slot != remembered)
                .filter(|slot| {
                    crate::workers::core_kind_for_slot(*slot) == crate::workers::CORE_KIND_PERF
                })
                .find_map(xpapp_compute_claim_slot)
        });
    let Some((worker, lane, identity)) = claimed else {
        return false;
    };
    let slot = worker.cpu_slot();
    let spawner = worker.spawner();
    match xpapp_compute_worker(index, worker, lane, identity) {
        Ok(task) => {
            queue.slot.store(slot, Ordering::Release);
            queue.started.store(true, Ordering::Release);
            spawner.spawn(task);
            crate::log_important!(target: "service";
                "xpapp-compute: worker={} slot={} core_kind=perf duty_percent={} burst_ms={} idle_grace_ms={} policy=sticky-shared-executor\n",
                index, slot, crate::allcaps::cpu_task_pool::GUEST_COMPUTE_DUTY_PERCENT,
                crate::allcaps::cpu_task_pool::GUEST_COMPUTE_BURST_TICKS,
                XPAPP_COMPUTE_IDLE_GRACE_MS);
            true
        }
        Err(_) => false,
    }
}

fn xpapp_compute_ensure_workers() -> usize {
    (0..XPAPP_COMPUTE_WORKERS)
        .filter(|index| xpapp_compute_start_worker(*index))
        .count()
}

fn xpapp_compute_enqueue(vm_id: u8, job: GuestComputeJob) -> Result<(), GuestComputeJob> {
    let _control = XPAPP_COMPUTE_CONTROL.lock();
    if xpapp_compute_ensure_workers() == 0 {
        if XPAPP_COMPUTE_UNAVAILABLE_LOGGED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            crate::log_important!(target: "service";
                "xpapp-compute: renderer_fallback=scalar reason=no-compatible-performance-worker requested_workers={}\n",
                XPAPP_COMPUTE_WORKERS);
        }
        return Err(job);
    }
    let start =
        XPAPP_COMPUTE_QUEUE_RR.fetch_add(1, Ordering::Relaxed) as usize % XPAPP_COMPUTE_WORKERS;
    let mut target = None;
    let mut best_cost = usize::MAX;
    for offset in 0..XPAPP_COMPUTE_WORKERS {
        let index = (start + offset) % XPAPP_COMPUTE_WORKERS;
        let queue = &XPAPP_COMPUTE_QUEUES[index];
        if !queue.started.load(Ordering::Acquire) {
            continue;
        }
        let cost = compute_budget::queue_cost(
            queue.jobs.lock().len(),
            queue.in_flight.load(Ordering::Acquire) as usize,
        );
        if cost < best_cost {
            best_cost = cost;
            target = Some(index);
        }
    }
    let Some(index) = target else {
        return Err(job);
    };
    let Some(owner) = reserve_guest_job(vm_id) else {
        return Err(job);
    };
    owner.diagnostic.identify("raster-compute", 0, XPAPP_COMPUTE_QUEUES[index].slot.load(Ordering::Acquire) as usize);
    let queue = &XPAPP_COMPUTE_QUEUES[index];
    let mut jobs = queue.jobs.lock();
    if jobs.len() >= XPAPP_COMPUTE_QUEUE_CAP {
        drop(owner);
        return Err(job);
    }
    jobs.push_back(XpappComputeEntry {
        vm_id,
        job: Some(job),
        owner,
    });
    drop(jobs);
    queue.wait.notify_one();
    crate::remote_work_wake::wake_cpu_for_remote_work(queue.slot.load(Ordering::Acquire));
    Ok(())
}

#[inline]
fn xpapp_compute_cycles() -> u64 {
    unsafe {
        core::arch::x86_64::_mm_lfence();
        core::arch::x86_64::_rdtsc()
    }
}

#[inline]
fn xpapp_compute_burst_cycles() -> u64 {
    compute_budget::burst_cycles(
        crate::time::tsc_hz(),
        crate::allcaps::cpu_task_pool::GUEST_COMPUTE_BURST_TICKS,
    )
}

#[trueos_executor::task(pool_size = XPAPP_COMPUTE_WORKERS)]
async fn xpapp_compute_worker(
    index: usize,
    _worker: crate::workers::ComputeWorkerLease,
    mut lane: crate::hv::lane::LaneLease,
    identity: crate::wls::WorkerIdentityLease,
) {
    let queue = &XPAPP_COMPUTE_QUEUES[index];
    let mut burst_cycles = 0u64;
    let mut cooldown_debt = CooldownDebt::default();
    loop {
        // Take the generation before examining the queue. A producer that
        // races this empty check will then make `wait_after_timeout` return
        // immediately rather than losing its wake for the idle grace period.
        let observed = queue.wait.observe();
        let entry = queue.jobs.lock().pop_front();
        let Some(mut entry) = entry else {
            queue
                .wait
                .wait_after_timeout(observed, XPAPP_COMPUTE_IDLE_GRACE_MS)
                .await;
            let _control = XPAPP_COMPUTE_CONTROL.lock();
            if queue.jobs.lock().is_empty() {
                queue.started.store(false, Ordering::Release);
                return;
            }
            continue;
        };
        queue.in_flight.fetch_add(1, Ordering::AcqRel);
        entry.owner.diagnostic.phase(diagnostics::RUNNING, 0, 0);
        lane.set_vm_owner(entry.vm_id);
        let started = xpapp_compute_cycles();
        let slice_cycles = xpapp_compute_burst_cycles()
            .saturating_sub(burst_cycles)
            .max(1);
        let result = {
            let _wls = identity.enter();
            crate::r::kernel_task_domain::with(
                crate::r::kernel_task_domain::KernelTaskDomain::TokioCarrier,
                Some(entry.vm_id),
                || {
                    crate::allocators::with_hv_guest_alloc_domain(entry.vm_id, || {
                        // Keep the guest realm and WLS identity across a full
                        // bounded slice. An eight-row raster step is only the
                        // preemption granularity; rebuilding those identities
                        // for every step would put the old call-chain overhead
                        // straight back on the frame path.
                        if crate::hv::guest_kill_requested(entry.vm_id) {
                            core::mem::forget(entry.job.take());
                            return false;
                        }
                        let cancelled = guest_job_cancellation_requested(entry.vm_id);
                        let mut more = !cancelled;
                        while more {
                            more = (entry.job.as_mut().expect("queued compute job"))();
                            if !more || xpapp_compute_cycles().wrapping_sub(started) >= slice_cycles
                            {
                                break;
                            }
                        }
                        // A closed VM, or a completed job, must release the
                        // closure in its guest realm so lifetime teardown can
                        // observe the owner after this scope exits.
                        if !more {
                            drop(entry.job.take());
                        }
                        more
                    })
                },
            )
        };
        lane.clear_vm_owner();
        let Some(more) = result else {
            let vm_id = entry.vm_id;
            entry.owner.diagnostic.phase(diagnostics::RETAINED, 0, 0);
            core::mem::forget(entry);
            queue.in_flight.fetch_sub(1, Ordering::AcqRel);
            crate::log_error!(target: "service";
                "xpapp-compute: guest allocation domain unavailable vm={} resources=retained\n", vm_id);
            return;
        };
        burst_cycles = burst_cycles.saturating_add(xpapp_compute_cycles().wrapping_sub(started));
        queue.in_flight.fetch_sub(1, Ordering::AcqRel);
        if more {
            entry.owner.diagnostic.phase(diagnostics::QUEUED, 0, 0);
            queue.jobs.lock().push_back(entry);
        }
        if burst_cycles >= xpapp_compute_burst_cycles() {
            let cooldown = cooldown_debt.request_timer_ticks(
                burst_cycles,
                crate::allcaps::cpu_task_pool::GUEST_COMPUTE_DUTY_PERCENT,
                crate::time::tsc_hz(),
                embassy_time_driver::TICK_HZ,
            );
            burst_cycles = 0;
            if cooldown != 0 {
                let started = xpapp_compute_cycles();
                Timer::after(EmbassyDuration::from_ticks(cooldown)).await;
                cooldown_debt.settle_paid_cycles(xpapp_compute_cycles().wrapping_sub(started));
                cooldown_debt.cap_credit_cycles(compute_budget::cooldown_cycles(
                    xpapp_compute_burst_cycles(),
                    crate::allcaps::cpu_task_pool::GUEST_COMPUTE_DUTY_PERCENT,
                ));
            }
            // Credit can skip a timer wait, never this cooperative executor
            // boundary. Other ready work gets a turn after every ~4 ms burst.
            crate::hv::execution_policy::yield_executor_turn().await;
        }
    }
}

fn submit_guest_compute_job(vm_id: u8, job: GuestComputeJob) -> i32 {
    // The persistent scheduler queue belongs to the kernel.  Its VecDeque,
    // worker-start scan, and task storage must therefore be allocated in the
    // host realm; only the submitted closure itself belongs to the guest.
    let raw = Box::into_raw(job);
    let rejected = crate::allocators::with_host_alloc_domain_strong(|| unsafe {
        let job = Box::from_raw(raw);
        match xpapp_compute_enqueue(vm_id, job) {
            Ok(()) => None,
            Err(job) => Some(Box::into_raw(job)),
        }
    });
    let Some(raw) = rejected else {
        return 0;
    };
    // A rejection must still run the guest closure's destructor in the guest
    // realm. If that realm is already gone, retain it rather than corrupting
    // the host allocator; it cannot be safely destroyed after this point.
    match crate::allocators::with_hv_guest_alloc_domain(vm_id, || unsafe {
        drop(Box::from_raw(raw));
    }) {
        Some(()) => -2,
        None => {
            crate::log_error!(target: "service";
                "xpapp-compute: submission allocation domain unavailable vm={} resources=retained\n", vm_id);
            -2
        }
    }
}

/// Host-side half of the Hull VMCALL path.  This function owns the raw closure
/// on every result so a rejection drops it under the VM allocation domain;
/// callers must never reconstruct it after this call returns.
pub unsafe fn submit_guest_compute_job_from_raw(vm_id: u8, data: usize, vtable: usize) -> i32 {
    if data == 0 || vtable == 0 {
        return -5;
    }
    let raw: *mut (dyn FnMut() -> bool + Send + 'static) =
        unsafe { core::mem::transmute((data, vtable)) };
    let job: GuestComputeJob = unsafe { Box::from_raw(raw) };
    submit_guest_compute_job(vm_id, job)
}

/// Submit one finite CPU band to XPAPP's two-worker strict-P-core pool.
/// A rejection consumes and drops the closure; callers retain the serial
/// fallback input separately and must not retry the same owned closure.
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_guest_compute_submit_job(job: GuestComputeJob) -> i32 {
    if let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() {
        let raw = Box::into_raw(job);
        let (data, vtable): (usize, usize) = unsafe { core::mem::transmute(raw) };
        let (status, rc) = crate::hv::vmcall::guest_call(
            crate::hv::vmcall::OP_BP_GUEST_COMPUTE_SUBMIT,
            data as u64,
            vtable as u64,
        );
        if status == crate::hv::vmcall::STATUS_OK {
            return rc as i32;
        }
        // The host did not receive ownership on a transport failure; this is
        // still the Hull's guest allocation realm.
        unsafe { drop(Box::from_raw(raw)) };
        return -6;
    }
    let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() else {
        return -2;
    };
    submit_guest_compute_job(vm_id, job)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_guest_compute_capacity() -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, count) =
            crate::hv::vmcall::guest_call(crate::hv::vmcall::OP_BP_GUEST_COMPUTE_CAPACITY, 0, 0);
        return if status == crate::hv::vmcall::STATUS_OK {
            count as usize
        } else {
            0
        };
    }
    guest_compute_capacity()
}

pub(crate) fn guest_compute_capacity() -> usize {
    crate::allocators::with_host_alloc_domain_strong(|| {
        let _control = XPAPP_COMPUTE_CONTROL.lock();
        xpapp_compute_ensure_workers();
        XPAPP_COMPUTE_QUEUES
            .iter()
            .filter(|queue| queue.started.load(Ordering::Acquire))
            .count()
    })
}

/// Advisory native capacity, independent of std thread counts and archive names.
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_service_lane_available_capacity() -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, count) =
            crate::hv::vmcall::guest_call(crate::hv::vmcall::OP_BP_SERVICE_LANE_CAPACITY, 0, 0);
        return if status == crate::hv::vmcall::STATUS_OK {
            count as usize
        } else {
            0
        };
    }
    service_lane_available_capacity_for_vm(crate::hv::current_guest_execution_context_vm_id())
}

/// Cooperative cancellation for long-lived native jobs. VM exit cannot run
/// the application's Rust destructors; jobs must observe closed admission and
/// return before teardown can release their executable pages and heap.
pub(crate) fn guest_job_cancellation_requested(vm_id: u8) -> bool {
    !lifetime::accepts_guest_jobs(vm_id)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_service_lane_cancellation_requested() -> bool {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, cancelled) =
            crate::hv::vmcall::guest_call(crate::hv::vmcall::OP_BP_SERVICE_LANE_CANCELLED, 0, 0);
        return status != crate::hv::vmcall::STATUS_OK || cancelled != 0;
    }
    crate::hv::current_guest_execution_context_vm_id().is_some_and(guest_job_cancellation_requested)
}

pub(crate) fn service_lane_available_capacity_for_vm(vm_id: Option<u8>) -> usize {
    if vm_id.is_some_and(|id| !lifetime::accepts_guest_jobs(id)) {
        return 0;
    }
    crate::workers::background_worker_slots()
        .into_iter()
        .filter(|slot| {
            crate::workers::is_general_background_worker_slot(*slot)
                && crate::workers::spawner_for_slot(*slot).is_some()
                && service_lane_started_for_slot(*slot as usize)
                && crate::hv::lane::is_carrier_lane_free(*slot)
        })
        .count()
        .min(crate::wls::available_worker_identities())
}
