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
static SHARED_TOKIO_RUNNER_STARTED: AtomicBool = AtomicBool::new(false);
static SHARED_TOKIO_UNREADY_LOGGED: AtomicBool = AtomicBool::new(false);

struct SharedTokioResult<T> {
    value: Mutex<Option<T>>,
    wait: WaitQueue,
}

pub fn shared_tokio_runtime_ready() -> bool {
    crate::r::readiness::is_set(crate::r::readiness::TOKIO_RUNTIME_READY)
}

fn pop_shared_tokio_job() -> Option<SharedTokioJob> {
    let mut jobs = SHARED_TOKIO_JOBS.lock();
    if jobs.is_empty() {
        None
    } else {
        Some(jobs.remove(0))
    }
}

fn run_shared_tokio_runtime() {
    crate::log_info!(target: "service"; "shared-tokio: runner start\n");

    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = match builder.build() {
        Ok(runtime) => runtime,
        Err(_) => {
            SHARED_TOKIO_RUNNER_STARTED.store(false, Ordering::Release);
            crate::log_warn!(target: "service"; "shared-tokio: runtime build failed\n");
            return;
        }
    };

    loop {
        while let Some(job) = pop_shared_tokio_job() {
            runtime.block_on(job());
        }

        SHARED_TOKIO_WAIT.wait_for_event_blocking_parked(25);
    }
}

pub fn start_shared_tokio_runtime() -> bool {
    if SHARED_TOKIO_RUNNER_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return true;
    }

    let rc = crate::t::trueos_tokio_worker::spawn_blocking_job_with_purpose(
        Box::new(run_shared_tokio_runtime),
        "shared-tokio-runtime",
    );
    if rc != 0 {
        SHARED_TOKIO_RUNNER_STARTED.store(false, Ordering::Release);
        crate::log_warn!(target: "service"; "shared-tokio: runner submit failed rc={}\n", rc);
        return false;
    }

    true
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
