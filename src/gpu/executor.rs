//! Async GPU admission and completion executor.
//!
//! GPU programs are still encoded by the hardware backend and scheduled by
//! the physical GPU firmware. This layer owns the host-side async contract:
//! one admitted job per virtual kernel queue, exact timeline retirement, and
//! waking Embassy tasks waiting on a GPU fence.

extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::{Context, Poll, Waker};
use spin::Mutex;

use super::physical::PhysicalContextDescriptor;
use super::vgpu::{self, KernelClient, TimelinePoint, TimelinePointStatus, VgpuError};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum FenceTarget {
    Kernel(KernelClient),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpuFenceError {
    ExecutionFailed,
    DeviceLost,
    InvalidFence,
}

/// One accepted kernel submission. The token is the identity used for exact
/// retirement and can create an awaitable fence without exposing an LRC or a
/// physical scheduler handle to the caller.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct KernelSubmission {
    client: KernelClient,
    point: TimelinePoint,
}

impl KernelSubmission {
    pub(crate) fn fence(self) -> GpuFence {
        GpuFence::new(FenceTarget::Kernel(self.client), self.point)
    }

    /// Backend-defined monotonic position of this request in the physical
    /// scheduler transport. Zero denotes an untracked/control submission.
    pub(crate) const fn physical_publish_sequence(self) -> u64 {
        self.point.physical_publish_sequence
    }
}

/// Future that resolves when one exact vGPU timeline point retires.
///
/// Dropping the future only detaches its waiter. GPU work is never cancelled
/// implicitly and its resources remain owned by the submitting backend until
/// the backend observes retirement.
pub(crate) struct GpuFence {
    waiter_id: u64,
    target: FenceTarget,
    point: TimelinePoint,
}

impl GpuFence {
    fn new(target: FenceTarget, point: TimelinePoint) -> Self {
        Self {
            waiter_id: NEXT_WAITER_ID.fetch_add(1, Ordering::Relaxed).max(1),
            target,
            point,
        }
    }
}

impl Future for GpuFence {
    type Output = Result<TimelinePoint, GpuFenceError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(result) = fence_result(this.target, this.point) {
            remove_waiter(this.waiter_id);
            return Poll::Ready(result);
        }

        register_waiter(this.waiter_id, this.target, this.point, cx.waker());

        // Close the check/register race. Retirement removes and wakes the
        // registered entry; if it happened just before registration, this
        // second query observes it and removes the now-unneeded waiter.
        if let Some(result) = fence_result(this.target, this.point) {
            remove_waiter(this.waiter_id);
            Poll::Ready(result)
        } else {
            Poll::Pending
        }
    }
}

impl Drop for GpuFence {
    fn drop(&mut self) {
        remove_waiter(self.waiter_id);
    }
}

#[derive(Clone)]
struct FenceWaiter {
    id: u64,
    target: FenceTarget,
    point: TimelinePoint,
    waker: Waker,
}

#[derive(Copy, Clone)]
struct InflightKernelSubmission {
    submission: KernelSubmission,
}

struct ExecutorState {
    admitting: Vec<KernelClient>,
    inflight: Vec<InflightKernelSubmission>,
    waiters: Vec<FenceWaiter>,
    submissions: u64,
    completions: u64,
    failures: u64,
}

impl ExecutorState {
    const fn new() -> Self {
        Self {
            admitting: Vec::new(),
            inflight: Vec::new(),
            waiters: Vec::new(),
            submissions: 0,
            completions: 0,
            failures: 0,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuExecutorStatus {
    pub(crate) submissions: u64,
    pub(crate) completions: u64,
    pub(crate) failures: u64,
    pub(crate) admitting: usize,
    pub(crate) inflight: usize,
    pub(crate) waiters: usize,
}

static NEXT_WAITER_ID: AtomicU64 = AtomicU64::new(1);
static EXECUTOR: Mutex<ExecutorState> = Mutex::new(ExecutorState::new());

/// Admit one already-encoded kernel context. A kernel client has one ordered
/// software lane, which gives the first executor version bounded backpressure
/// and unambiguous per-point completion without growing a driver framework.
pub(crate) fn submit_kernel_context(
    client: KernelClient,
    descriptor: PhysicalContextDescriptor,
) -> Result<KernelSubmission, VgpuError> {
    {
        let mut executor = EXECUTOR.lock();
        if executor.admitting.contains(&client)
            || executor
                .inflight
                .iter()
                .any(|entry| entry.submission.client == client)
        {
            return Err(VgpuError::Busy);
        }
        executor.admitting.push(client);
    }

    let submitted = vgpu::submit_kernel_context(client, descriptor);
    let mut executor = EXECUTOR.lock();
    executor.admitting.retain(|admitting| *admitting != client);
    match submitted {
        Ok(point) => {
            let submission = KernelSubmission { client, point };
            executor
                .inflight
                .push(InflightKernelSubmission { submission });
            executor.submissions = executor.submissions.saturating_add(1);
            Ok(submission)
        }
        Err(error) => {
            executor.failures = executor.failures.saturating_add(1);
            Err(error)
        }
    }
}

/// Retire the exact point returned by `submit_kernel_context` and wake every
/// task waiting on that point or an earlier point in the same ordered queue.
pub(crate) fn complete_kernel_submission(
    submission: KernelSubmission,
    completed: bool,
) -> Option<TimelinePoint> {
    let retired = vgpu::complete_kernel_submission(submission.client, submission.point, completed)?;

    let wakers = {
        let mut executor = EXECUTOR.lock();
        executor
            .inflight
            .retain(|entry| entry.submission != submission);
        if completed {
            executor.completions = executor.completions.saturating_add(1);
        } else {
            executor.failures = executor.failures.saturating_add(1);
        }
        take_ready_wakers(&mut executor.waiters, FenceTarget::Kernel(submission.client), retired)
    };
    wake_all(wakers);
    Some(retired)
}

pub(crate) fn status() -> GpuExecutorStatus {
    let executor = EXECUTOR.lock();
    GpuExecutorStatus {
        submissions: executor.submissions,
        completions: executor.completions,
        failures: executor.failures,
        admitting: executor.admitting.len(),
        inflight: executor.inflight.len(),
        waiters: executor.waiters.len(),
    }
}

fn fence_result(
    target: FenceTarget,
    point: TimelinePoint,
) -> Option<Result<TimelinePoint, GpuFenceError>> {
    let FenceTarget::Kernel(client) = target;
    let status = vgpu::kernel_point_status(client, point);
    match status {
        Ok(TimelinePointStatus::Pending) | Err(VgpuError::NotComplete) => None,
        Ok(TimelinePointStatus::Complete) => Some(Ok(point)),
        Ok(TimelinePointStatus::Failed) => Some(Err(GpuFenceError::ExecutionFailed)),
        Err(VgpuError::DeviceLost | VgpuError::NoPhysicalDevice | VgpuError::DeviceNotReady) => {
            Some(Err(GpuFenceError::DeviceLost))
        }
        Err(_) => Some(Err(GpuFenceError::InvalidFence)),
    }
}

fn register_waiter(id: u64, target: FenceTarget, point: TimelinePoint, waker: &Waker) {
    let mut executor = EXECUTOR.lock();
    if let Some(waiter) = executor.waiters.iter_mut().find(|waiter| waiter.id == id) {
        waiter.target = target;
        waiter.point = point;
        if !waiter.waker.will_wake(waker) {
            waiter.waker = waker.clone();
        }
        return;
    }
    executor.waiters.push(FenceWaiter {
        id,
        target,
        point,
        waker: waker.clone(),
    });
}

fn remove_waiter(id: u64) {
    EXECUTOR.lock().waiters.retain(|waiter| waiter.id != id);
}

fn take_ready_wakers(
    waiters: &mut Vec<FenceWaiter>,
    target: FenceTarget,
    retired: TimelinePoint,
) -> Vec<Waker> {
    let mut ready = Vec::new();
    let mut index = 0;
    while index < waiters.len() {
        let waiter = &waiters[index];
        if waiter.target == target
            && waiter.point.queue == retired.queue
            && waiter.point.value <= retired.value
        {
            ready.push(waiters.remove(index).waker);
        } else {
            index += 1;
        }
    }
    ready
}

fn wake_all(wakers: Vec<Waker>) {
    for waker in wakers {
        waker.wake();
    }
}
