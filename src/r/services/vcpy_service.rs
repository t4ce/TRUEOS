//! Shell controlled Fast Copy Engine bring-up service.
//!
//! The worker polls BCS completion every 16 ms. Demo submissions are 250 ms
//! apart, and this service owns their lifecycle.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use trueos_executor::Spawner;

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
