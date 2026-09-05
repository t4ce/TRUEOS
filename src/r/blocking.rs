extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use heapless::String as HString;
use spin::Mutex;
use trueos_time::{Duration as EmbassyDuration, Timer};

pub type BlockingJobFn = Box<dyn FnOnce() + Send + 'static>;

mod lifetime;
pub(crate) use lifetime::{close_guest_jobs, drain_guest_jobs, guest_jobs_in_flight, open_guest_jobs};

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
    let started_ms = now_ms();
    service_lane_activity_begin(slot, id, vm_id, purpose);
    crate::log_info!(
        target: "service";
        "blocking-job: run begin id={} vm={:?} purpose={} tag={}\n",
        id,
        vm_id,
        purpose,
        policy_tag
    );
    if let Some(vm_id) = vm_id {
        crate::log_os::log_with_area_purpose(
            crate::log_os::flags::LogArea::Blueprint,
            log_os_core::LogLevel::Info,
            Some("multi-rt-alloc"),
            format_args!(
                "guest service job begin id={} vm={} purpose={} alloc_domain=hv-guest\n",
                id, vm_id, purpose
            ),
        );
        let mut pending_call = Some(call);
        let ran = crate::r::kernel_task_domain::with(
            crate::r::kernel_task_domain::KernelTaskDomain::TokioCarrier,
            Some(vm_id),
            || {
                crate::allocators::with_hv_guest_alloc_domain(vm_id, || {
                    run_blocking_job_call(pending_call.take().expect("native job consumed once"))
                }).is_some()
            },
        );
        if !ran {
            // Do not invoke a guest destructor in the host allocation realm,
            // or publish its executable memory as reusable after losing the
            // realm. Retain the reservation for diagnosis/recovery.
            core::mem::forget(pending_call);
            core::mem::forget(owner);
            crate::log_error!(target: "service";
                "blocking-job: guest allocation domain unavailable id={} vm={} resources=retained\n", id, vm_id);
            service_lane_activity_finish(slot);
            return;
        }
        crate::log_os::log_with_area_purpose(
            crate::log_os::flags::LogArea::Blueprint,
            log_os_core::LogLevel::Info,
            Some("multi-rt-alloc"),
            format_args!("guest service job done id={} vm={} purpose={}\n", id, vm_id, purpose),
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
        let Some(owner) = lifetime::reserve(vm_id) else { return Err(call) };
        Some(owner)
    } else {
        None
    };
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

/// Advisory native capacity, independent of std thread counts and archive names.
#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_service_lane_available_capacity() -> usize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, count) = crate::hv::vmcall::guest_call(
            crate::hv::vmcall::OP_BP_SERVICE_LANE_CAPACITY, 0, 0);
        return if status == crate::hv::vmcall::STATUS_OK { count as usize } else { 0 };
    }
    service_lane_available_capacity_for_vm(crate::hv::current_guest_execution_context_vm_id())
}

pub(crate) fn service_lane_available_capacity_for_vm(vm_id: Option<u8>) -> usize {
    if vm_id.is_some_and(|id| !lifetime::accepts_guest_jobs(id)) {
        return 0;
    }
    crate::workers::background_worker_slots().into_iter()
        .filter(|slot| crate::workers::is_general_background_worker_slot(*slot)
            && crate::workers::spawner_for_slot(*slot).is_some()
            && service_lane_started_for_slot(*slot as usize)
            && crate::hv::lane::is_carrier_lane_free(*slot))
        .count()
        .min(crate::wls::available_worker_identities())
}
