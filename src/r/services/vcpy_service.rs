//! BCS0 copy service and shell-controlled MicroFont bring-up worker.
//!
//! The worker polls BCS completion every 16 ms. Demo submissions are 250 ms
//! apart, and this service owns their lifecycle.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use trueos_executor::Spawner;

pub(crate) fn queue_marker(
    destination: crate::intel::GucBcs0RgbaSurface,
) -> Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError> {
    crate::intel::queue_guc_bcs0_marker(destination)
}

pub(crate) fn queue_rgba_copies(
    destination: crate::intel::GucBcs0RgbaSurface,
    copies: &[crate::intel::GucBcs0RgbaCopy],
) -> Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError> {
    crate::intel::queue_guc_bcs0_rgba_copies(destination, copies)
}

pub(crate) fn poll_rgba_copies(
    submission: crate::intel::GucBcs0CopySubmission,
) -> crate::intel::GucBcs0CopyCompletion {
    crate::intel::poll_guc_bcs0_rgba_copies(submission)
}

/// Copy an already released, linear RGBA8 source into a destination at (0,0).
/// The caller owns both allocations for this entire synchronous operation.
/// SubmittedIncomplete requires retaining both allocations until reboot; only
/// Unavailable permits the caller to use its reference compute implementation.
/// This path is independent of the MicroFont worker and its 250 ms cadence.
pub(crate) fn copy_rgba8_complete(
    source: crate::intel::gpgpu::GpgpuRgba8Surface,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> crate::intel::gpgpu::GpgpuSubmissionOutcome {
    use crate::intel::{GucBcs0CopyCompletion as Completion, GucBcs0CopySubmitError as SubmitError};
    use crate::intel::gpgpu::GpgpuSubmissionOutcome as Outcome;

    let result = (|| {
        if source.storage_order != destination.storage_order
            || source.width > destination.width || source.height > destination.height
        {
            return Outcome::Unavailable;
        }
        let surface = |s: crate::intel::gpgpu::GpgpuRgba8Surface| crate::intel::GucBcs0RgbaSurface {
            phys: s.phys, gpu: s.gpu, bytes: s.bytes,
            width: s.width, height: s.height, pitch_bytes: s.pitch_bytes,
        };
        let copy = crate::intel::GucBcs0RgbaCopy {
            source: surface(source), source_x: 0, source_y: 0,
            destination_x: 0, destination_y: 0, width: source.width, height: source.height,
        };
        let submission = match queue_rgba_copies(surface(destination), &[copy]) {
            Ok(submission) => submission,
            Err(SubmitError::Busy | SubmitError::Unavailable | SubmitError::InvalidRequest) => {
                return Outcome::Unavailable;
            }
            Err(SubmitError::SubmitFailed) => return Outcome::SubmittedIncomplete,
        };
        loop {
            match poll_rgba_copies(submission) {
                Completion::Pending => core::hint::spin_loop(),
                Completion::Complete => return Outcome::Complete,
                Completion::Failed | Completion::InvalidSubmission => {
                    return Outcome::SubmittedIncomplete;
                }
            }
        }
    })();
    match result {
        Outcome::Complete => {
            CONSUMER_COPIES.fetch_add(1, Ordering::Relaxed);
            CONSUMER_BYTES.fetch_add(u64::from(source.width) * u64::from(source.height) * 4, Ordering::Relaxed);
        }
        Outcome::Unavailable => { CONSUMER_FALLBACKS.fetch_add(1, Ordering::Relaxed); }
        Outcome::SubmittedIncomplete => { CONSUMER_FAILURES.fetch_add(1, Ordering::Relaxed); }
    }
    result
}

static CONSUMER_COPIES: AtomicU64 = AtomicU64::new(0);
static CONSUMER_BYTES: AtomicU64 = AtomicU64::new(0);
static CONSUMER_FALLBACKS: AtomicU64 = AtomicU64::new(0);
static CONSUMER_FAILURES: AtomicU64 = AtomicU64::new(0);

// Poll completion well inside the BCS lane's 250 ms timeout. The demo itself
// admits a new copy only once per 250 ms.
const PERIOD_MS: u64 = 16;
static RUNNING: AtomicBool = AtomicBool::new(false);
static TASK_ACTIVE: AtomicBool = AtomicBool::new(false);
static TICKS: AtomicU64 = AtomicU64::new(0);
static FAILURES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub running: bool,
    pub ticks: u64,
    pub failures: u64,
    pub consumer_copies: u64,
    pub consumer_bytes: u64,
    pub consumer_fallbacks: u64,
    pub consumer_failures: u64,
}

/// Start the resident service. A second start is idempotent.
pub fn start(spawner: &Spawner) -> Result<(), &'static str> {
    if TASK_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return if RUNNING.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("previous vcpy worker is still draining")
        };
    }
    RUNNING.store(true, Ordering::Release);
    match vcpy_service_task() {
        Ok(token) => {
            spawner.spawn(token);
            Ok(())
        }
        Err(_) => {
            RUNNING.store(false, Ordering::Release);
            TASK_ACTIVE.store(false, Ordering::Release);
            Err("executor has no vcpy task slot")
        }
    }
}

/// Request the resident worker to stop at its next cadence boundary.
pub fn stop() {
    RUNNING.store(false, Ordering::Release);
}

pub fn status() -> Status {
    Status {
        running: TASK_ACTIVE.load(Ordering::Acquire),
        ticks: TICKS.load(Ordering::Relaxed),
        failures: FAILURES.load(Ordering::Relaxed),
        consumer_copies: CONSUMER_COPIES.load(Ordering::Relaxed),
        consumer_bytes: CONSUMER_BYTES.load(Ordering::Relaxed),
        consumer_fallbacks: CONSUMER_FALLBACKS.load(Ordering::Relaxed),
        consumer_failures: CONSUMER_FAILURES.load(Ordering::Relaxed),
    }
}

#[trueos_executor::task(pool_size = 1)]
async fn vcpy_service_task() {
    while RUNNING.load(Ordering::Acquire) {
        tick();
        TICKS.fetch_add(1, Ordering::Relaxed);
        trueos_time::Timer::after(trueos_time::Duration::from_millis(PERIOD_MS)).await;
    }
    while !crate::ui4::vcpy_demo::stop() {
        if let Err(reason) = crate::ui4::vcpy_demo::tick() {
            FAILURES.fetch_add(1, Ordering::Relaxed);
            crate::log_warn!(target: "service";
                "vcpy-service: drain failed reason={}\n", reason);
        }
        trueos_time::Timer::after(trueos_time::Duration::from_millis(16)).await;
    }
    TASK_ACTIVE.store(false, Ordering::Release);
}

/// One scheduled service step for completion and new Fast Copy work.
pub(crate) fn tick() {
    if let Err(reason) = crate::ui4::vcpy_demo::tick() {
        FAILURES.fetch_add(1, Ordering::Relaxed);
        crate::log_warn!(target: "service";
            "vcpy-service: demo tick failed reason={} failures={}\n",
            reason,
            FAILURES.load(Ordering::Relaxed));
    }
}
