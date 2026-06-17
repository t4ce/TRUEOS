extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub type BlockingJobFn = Box<dyn FnOnce() + Send + 'static>;

const BLOCKING_JOB_QUEUE_WARN_DEPTH: usize = 100;
const BLOCKING_JOB_QUEUE_CAP: usize = 4094;
const BLOCKING_JOB_DISPATCH_IDLE_MS: u64 = 5;
const BLOCKING_JOB_DISPATCH_BUSY_MS: u64 = 1;
const BLOCKING_JOB_TAG_HOST: &str = "host-blocking-job";
const BLOCKING_JOB_TAG_VMX: &str = "vmx-respect-architecture";
static NEXT_BLOCKING_JOB_ID: AtomicU64 = AtomicU64::new(1);
static BLOCKING_JOB_QUEUE: Mutex<VecDeque<BlockingJobEntry>> = Mutex::new(VecDeque::new());

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

pub fn queued_blocking_jobs() -> usize {
    BLOCKING_JOB_QUEUE.lock().len()
}

pub fn pop_blocking_job() -> Option<BlockingJobEntry> {
    BLOCKING_JOB_QUEUE.lock().pop_front()
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
    crate::log_trace!(
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
    crate::log_trace!(
        target: "service";
        "blocking-job: run done id={} vm={:?} purpose={} tag={} elapsed_ms={}\n",
        id,
        vm_id,
        purpose,
        policy_tag,
        now_ms().saturating_sub(started_ms)
    );
}

#[embassy_executor::task(pool_size = 64)]
async fn blocking_job_execute_task(
    entry: BlockingJobEntry,
    _lease: crate::hv::lane::LaneLease,
    slot: u32,
    core_kind: u8,
) {
    crate::log_trace!(
        target: "service";
        "blocking-job: carrier start slot={} core_kind={}\n",
        slot,
        core_kind
    );
    run_blocking_job_entry(entry);
}

#[embassy_executor::task]
pub async fn blocking_job_dispatcher_task() {
    loop {
        if queued_blocking_jobs() == 0 {
            Timer::after(EmbassyDuration::from_millis(BLOCKING_JOB_DISPATCH_IDLE_MS)).await;
            continue;
        }

        let target = match crate::hv::guest_work::pick_tokio_blocking_lane() {
            Ok(target) => target,
            Err(err) => {
                crate::log_trace!(
                    target: "service";
                    "blocking-job: no carrier lane queued={} reason={}\n",
                    queued_blocking_jobs(),
                    err.as_str()
                );
                Timer::after(EmbassyDuration::from_millis(BLOCKING_JOB_DISPATCH_BUSY_MS)).await;
                continue;
            }
        };

        let Some(entry) = pop_blocking_job() else {
            continue;
        };
        let slot = target.slot;
        let core_kind = target.core_kind;
        match blocking_job_execute_task(entry, target.lease, slot, core_kind) {
            Ok(token) => target.spawner.spawn(token),
            Err(err) => {
                crate::log_error!(
                    target: "service";
                    "blocking-job: carrier spawn failed slot={} core_kind={} err={:?}\n",
                    slot,
                    core_kind,
                    err
                );
            }
        }
    }
}

fn enqueue_blocking_job(
    vm_id: Option<u8>,
    purpose: &'static str,
    call: BlockingJobCall,
) -> Result<u64, BlockingJobCall> {
    let mut queue = BLOCKING_JOB_QUEUE.lock();
    if queue.len() >= BLOCKING_JOB_QUEUE_CAP {
        crate::log_error!(
            target: "service";
            "blocking-job: out of blocking_jobs cap={} vm={:?} purpose={}\n",
            BLOCKING_JOB_QUEUE_CAP,
            vm_id,
            purpose
        );
        return Err(call);
    }

    let id = NEXT_BLOCKING_JOB_ID.fetch_add(1, Ordering::AcqRel);
    let policy_tag = if vm_id.is_some() {
        BLOCKING_JOB_TAG_VMX
    } else {
        BLOCKING_JOB_TAG_HOST
    };
    queue.push_back(BlockingJobEntry {
        id,
        vm_id,
        purpose,
        policy_tag,
        call,
    });
    let queued = queue.len();
    if queued > BLOCKING_JOB_QUEUE_WARN_DEPTH {
        crate::log_error!(
            target: "service";
            "blocking-job: backlog above safe depth id={} vm={:?} purpose={} tag={} queued={} safe_depth={} cap={}\n",
            id,
            vm_id,
            purpose,
            policy_tag,
            queued,
            BLOCKING_JOB_QUEUE_WARN_DEPTH,
            BLOCKING_JOB_QUEUE_CAP
        );
    }
    crate::log_trace!(
        target: "service";
        "blocking-job: queued id={} vm={:?} purpose={} tag={} queued={} cap={}\n",
        id,
        vm_id,
        purpose,
        policy_tag,
        queued,
        BLOCKING_JOB_QUEUE_CAP
    );
    Ok(id)
}

pub fn spawn_blocking_job_with_purpose(job: BlockingJobFn, purpose: &'static str) -> i32 {
    match enqueue_blocking_job(None, purpose, BlockingJobCall::Host(job)) {
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
    if data == 0 || vtable == 0 {
        return -5;
    }
    match enqueue_blocking_job(Some(vm_id), purpose, BlockingJobCall::GuestRaw { data, vtable }) {
        Ok(_) => 0,
        Err(_) => -2,
    }
}

pub unsafe fn spawn_guest_blocking_job_from_raw(
    vm_id: u8,
    data: usize,
    vtable: usize,
    purpose: &'static str,
) -> i32 {
    unsafe { spawn_vmx_thread_from_raw(vm_id, data, vtable, purpose) }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn trueos_tokio_spawn_blocking_job(job: BlockingJobFn) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let raw = Box::into_raw(job);
        let (data, vtable): (usize, usize) = unsafe { core::mem::transmute(raw) };
        let (status, rc) = crate::hv::vmcall::guest_call(
            crate::hv::vmcall::OP_BP_TOKIO_BLOCKING_SPAWN,
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
