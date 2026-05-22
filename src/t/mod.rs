//! TRUEOS Tokio integration.
//!
//! This module owns the boundary between Tokio-facing crates and TRUEOS runtime
//! services: time, blocking workers, filesystem shims, and VNet/Mio/socket2.

pub mod io {
    pub use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    pub use trueos_io::*;
}

pub(crate) mod kernel_task_domain;
pub mod net;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod platform;
pub(crate) mod static_map;
pub(crate) mod static_slots;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod th;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod tokio_environment;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub(crate) mod tokio_platform;
pub mod tokio_probe;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod trueos_tokio_worker;

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::wait::WaitQueue;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunError {
    Build,
    NoSharedRuntime,
}

type SharedTokioJob =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + 'static>> + Send + 'static>;

static SHARED_TOKIO_JOBS: Mutex<Vec<SharedTokioJob>> = Mutex::new(Vec::new());
static SHARED_TOKIO_WAIT: WaitQueue = WaitQueue::new();
static SHARED_TOKIO_READY: AtomicBool = AtomicBool::new(false);
static SHARED_TOKIO_PUMP_LOGGED: AtomicBool = AtomicBool::new(false);
static SHARED_TOKIO_UNREADY_LOGGED: AtomicBool = AtomicBool::new(false);

struct SharedTokioResult<T> {
    value: Mutex<Option<T>>,
    wait: WaitQueue,
}

pub fn shared_tokio_runtime_ready() -> bool {
    SHARED_TOKIO_READY.load(Ordering::Acquire)
}

pub fn spawn_on_shared_tokio<F, MakeFuture>(make_future: MakeFuture) -> Result<(), RunError>
where
    F: Future<Output = ()> + 'static,
    MakeFuture: FnOnce() -> F + Send + 'static,
{
    if !shared_tokio_runtime_ready() {
        if !SHARED_TOKIO_UNREADY_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log!("t/tokio: shared runtime unavailable; job rejected\n");
        }
        return Err(RunError::NoSharedRuntime);
    }

    SHARED_TOKIO_JOBS
        .lock()
        .push(Box::new(move || Box::pin(make_future())));
    SHARED_TOKIO_WAIT.notify_one();
    Ok(())
}

/// Drop the Tokio observation handle while leaving the task scheduled.
///
/// This is TRUEOS detach vocabulary for Tokio tasks, not a pthread detach.
pub fn detach_tokio_task<T>(handle: tokio::task::JoinHandle<T>) {
    drop(handle);
}

pub async fn run_on_shared_tokio<F, T, MakeFuture>(make_future: MakeFuture) -> Result<T, RunError>
where
    F: Future<Output = T> + 'static,
    T: Send + 'static,
    MakeFuture: FnOnce() -> F + Send + 'static,
{
    let state = Arc::new(SharedTokioResult {
        value: Mutex::new(None),
        wait: WaitQueue::new(),
    });
    let notify_state = state.clone();

    spawn_on_shared_tokio(move || async move {
        let result = make_future().await;
        *notify_state.value.lock() = Some(result);
        notify_state.wait.notify_all();
    })?;

    loop {
        if let Some(result) = state.value.lock().take() {
            return Ok(result);
        }
        state.wait.wait_for_event().await;
    }
}

pub async fn shared_tokio_job_pump() {
    SHARED_TOKIO_READY.store(true, Ordering::Release);
    crate::r::readiness::set(crate::r::readiness::TOKIO_RUNTIME_READY);
    if !SHARED_TOKIO_PUMP_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log!("t/tokio: shared job pump online\n");
    }

    loop {
        let job = {
            let mut jobs = SHARED_TOKIO_JOBS.lock();
            if jobs.is_empty() {
                None
            } else {
                Some(jobs.remove(0))
            }
        };

        if let Some(make_job) = job {
            detach_tokio_task(tokio::task::spawn_local(make_job()));
        } else {
            SHARED_TOKIO_WAIT.wait_for_event_timeout(25).await;
        }
    }
}

fn run_shared_tokio_runtime() -> Result<(), RunError> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build().map_err(|_| RunError::Build)?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, shared_tokio_job_pump());
    Ok(())
}

#[embassy_executor::task]
pub async fn shared_tokio_runtime_service_task() {
    const RETRY_MS: u64 = 1000;
    const HW_LOGO_DRAIN_TIMEOUT_MS: u64 = 12_000;

    crate::r::readiness::wait_for(crate::r::readiness::BACKGROUND_AP_WORKER_READY).await;
    let hw_logo_done = crate::intel::wait_hw_logo_sequence_done(HW_LOGO_DRAIN_TIMEOUT_MS).await;
    crate::log!(
        "t/tokio: launching shared runtime after BACKGROUND_AP_WORKER_READY hw_logo_done={} timeout_ms={}\n",
        hw_logo_done as u8,
        HW_LOGO_DRAIN_TIMEOUT_MS
    );

    loop {
        let rc = crate::t::trueos_tokio_worker::spawn_blocking_job_with_purpose(
            Box::new(|| {
                if let Err(err) = run_shared_tokio_runtime() {
                    SHARED_TOKIO_READY.store(false, Ordering::Release);
                    crate::log!("t/tokio: shared runtime failed {:?}\n", err);
                }
            }),
            "shared-tokio-runtime",
        );
        if rc == 0 {
            crate::log!("t/tokio: submitted shared runtime to blocking lane\n");
            core::future::pending::<()>().await;
        }
        crate::log!("t/tokio: shared runtime carrier unavailable rc={} retry={}ms\n", rc, RETRY_MS);
        Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
    }
}

pub fn block_on_io<F>(future: F) -> Result<F::Output, RunError>
where
    F: Future,
{
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build().map_err(|_| RunError::Build)?;
    Ok(runtime.block_on(future))
}
