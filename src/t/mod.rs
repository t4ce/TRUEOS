//! TRUEOS Tokio integration.
//!
//! This module owns the boundary between Tokio-facing crates and TRUEOS runtime
//! services: time, blocking workers, filesystem shims, and VNet/Mio/socket2.

pub mod io {
    pub use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    pub use trueos_io::*;
}

pub(crate) mod app_exec;
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
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod trueos_tokio_worker;

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
static SHARED_TOKIO_ACTIVE: AtomicU32 = AtomicU32::new(0);

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

fn shared_tokio_queued_jobs() -> usize {
    SHARED_TOKIO_JOBS.lock().len()
}

fn shared_tokio_has_work() -> bool {
    SHARED_TOKIO_ACTIVE.load(Ordering::Acquire) != 0 || shared_tokio_queued_jobs() != 0
}

async fn shared_tokio_job_pump_quantum(quantum_ms: u64) {
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
            SHARED_TOKIO_ACTIVE.fetch_add(1, Ordering::AcqRel);
            detach_tokio_task(tokio::task::spawn_local(async move {
                make_job().await;
                SHARED_TOKIO_ACTIVE.fetch_sub(1, Ordering::AcqRel);
                SHARED_TOKIO_WAIT.notify_one();
            }));
        } else {
            break;
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(quantum_ms)).await;
}

fn build_shared_tokio_runtime() -> Result<tokio::runtime::Runtime, RunError> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    builder.build().map_err(|_| RunError::Build)
}

#[embassy_executor::task]
pub async fn shared_tokio_runtime_service_task() {
    const QUANTUM_MS: u64 = 50;
    const IDLE_WAIT_MS: u64 = 25;

    crate::r::readiness::wait_for(crate::r::readiness::BACKGROUND_AP_WORKER_READY).await;
    crate::log!(
        "t/tokio: launching shared runtime quantum pump after BACKGROUND_AP_WORKER_READY quantum={}ms\n",
        QUANTUM_MS
    );

    let runtime = match build_shared_tokio_runtime() {
        Ok(runtime) => runtime,
        Err(err) => {
            crate::log!("t/tokio: shared runtime failed {:?}\n", err);
            return;
        }
    };
    let local = tokio::task::LocalSet::new();

    loop {
        if !shared_tokio_has_work() && shared_tokio_runtime_ready() {
            SHARED_TOKIO_WAIT.wait_for_event_timeout(IDLE_WAIT_MS).await;
        }

        local.block_on(&runtime, shared_tokio_job_pump_quantum(QUANTUM_MS));
        Timer::after(EmbassyDuration::from_millis(1)).await;
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
