extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll, Waker};
use embassy_executor::task;
use embassy_time_driver::{now, TICK_HZ};
use spin::Mutex;

/// Update a stored waker if it differs from the current one.
#[inline]
pub fn register_waker_slot(slot: &mut Option<Waker>, waker: &Waker) -> bool {
    let should_replace = match slot.as_ref() {
        Some(existing) => !existing.will_wake(waker),
        None => true,
    };
    if should_replace {
        *slot = Some(waker.clone());
    }
    should_replace
}

/// Register a waker into a list if it is not already present.
#[inline]
pub fn register_waker_list(list: &mut Vec<Waker>, waker: &Waker) -> bool {
    if list.iter().any(|existing| existing.will_wake(waker)) {
        return false;
    }
    list.push(waker.clone());
    true
}

/// Single spin step for polling loops.
///
/// Important: this must not execute `hlt`.
/// Many low-level drivers use polling (e.g. virtio queue progress by observing
/// shared memory updated by the device). If we `hlt` here we may never observe
/// the condition becoming true, which can present as a hard freeze (notably from
/// synchronous shell commands like `gfx sw`).
#[inline]
pub fn spin_step() {
    crate::time::poll();
    crate::runtime::poll_local_executor();
    core::hint::spin_loop();
}

/// Spin step that does **not** poll the async executor.
///
/// Use this inside low-level driver critical sections / global locks where
/// polling the executor could re-enter unrelated subsystems and deadlock
/// (e.g. shell invoking `gfx` while the gfx SYSTEM mutex is held).
#[inline]
pub fn spin_step_no_exec() {
    crate::time::poll();
    core::hint::spin_loop();
}

/// Single parking step that drives async work and may idle the BSP.
#[inline]
pub fn park_step() {
    crate::time::poll();
    crate::runtime::poll_local_executor();
    crate::power::idle_hint();
}

/// Spin until `condition` is true or the timeout expires.
#[inline]
pub fn spin_until_timeout<F: FnMut() -> bool>(timeout_ms: u64, mut condition: F) -> bool {
    let hz = TICK_HZ as u64;
    let ticks = if hz == 0 {
        0
    } else {
        ((timeout_ms.saturating_mul(hz) + 999) / 1000).max(1)
    };
    let deadline = now().saturating_add(ticks);

    loop {
        if condition() {
            return true;
        }
        if now() >= deadline {
            return false;
        }
        spin_step();
    }
}

/// Spin until `condition` is true or the timeout expires, without polling the executor.
#[inline]
pub fn spin_until_timeout_no_exec<F: FnMut() -> bool>(timeout_ms: u64, mut condition: F) -> bool {
    let hz = TICK_HZ as u64;
    let ticks = if hz == 0 {
        0
    } else {
        ((timeout_ms.saturating_mul(hz) + 999) / 1000).max(1)
    };
    let deadline = now().saturating_add(ticks);

    loop {
        if condition() {
            return true;
        }
        if now() >= deadline {
            return false;
        }
        spin_step_no_exec();
    }
}

/// Take a waker from a slot and wake it.
#[inline]
pub fn take_and_wake(slot: &mut Option<Waker>) -> bool {
    if let Some(waker) = slot.take() {
        waker.wake();
        return true;
    }
    false
}

/// A minimal wait-queue for task-context wakeups.
pub struct WaitQueue {
    seq: AtomicU32,
    wakers: Mutex<Vec<Waker>>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
            wakers: Mutex::new(Vec::new()),
        }
    }

    #[inline]
    pub fn notify_one(&self) -> bool {
        self.seq.fetch_add(1, Ordering::Release);
        let waker = {
            let mut wakers = self.wakers.lock();
            if wakers.is_empty() {
                None
            } else {
                Some(wakers.remove(0))
            }
        };
        if let Some(waker) = waker {
            waker.wake();
            return true;
        }
        false
    }

    #[inline]
    pub fn notify_all(&self) -> usize {
        self.seq.fetch_add(1, Ordering::Release);
        let wakers = {
            let mut wakers = self.wakers.lock();
            core::mem::take(&mut *wakers)
        };
        let count = wakers.len();
        for waker in wakers {
            waker.wake();
        }
        count
    }

    #[inline]
    pub async fn wait_for_event(&self) {
        let _ = self.wait_for_event_timeout(0).await;
    }

    #[inline]
    pub async fn wait_for_event_timeout(&self, timeout_ms: u64) -> bool {
        let hz = TICK_HZ as u64;
        let ticks = if hz == 0 || timeout_ms == 0 {
            0
        } else {
            ((timeout_ms.saturating_mul(hz) + 999) / 1000).max(1)
        };
        let deadline = if ticks == 0 {
            0
        } else {
            now().saturating_add(ticks)
        };
        let mut observed = self.seq.load(Ordering::Acquire);

        core::future::poll_fn(|cx: &mut Context<'_>| {
            if ticks != 0 && now() >= deadline {
                return Poll::Ready(false);
            }

            let current = self.seq.load(Ordering::Acquire);
            if current != observed {
                observed = current;
                return Poll::Ready(true);
            }

            {
                let mut wakers = self.wakers.lock();
                register_waker_list(&mut *wakers, cx.waker());
            }

            let current = self.seq.load(Ordering::Acquire);
            if current != observed {
                observed = current;
                return Poll::Ready(true);
            }

            Poll::Pending
        })
        .await
    }

    #[inline]
    pub fn wait_for_event_blocking(&self, timeout_ms: u64) -> bool {
        let hz = TICK_HZ as u64;
        let ticks = if hz == 0 || timeout_ms == 0 {
            0
        } else {
            ((timeout_ms.saturating_mul(hz) + 999) / 1000).max(1)
        };
        let deadline = if ticks == 0 {
            0
        } else {
            now().saturating_add(ticks)
        };
        let observed = self.seq.load(Ordering::Acquire);

        loop {
            if ticks != 0 && now() >= deadline {
                return false;
            }

            let current = self.seq.load(Ordering::Acquire);
            if current != observed {
                return true;
            }

            // Blocking waits must not `hlt`.
            // Many subsystems (net fetch, module loader, sync wrappers) depend on polling-driven
            // progress where there may be no periodic interrupt to wake a halted CPU.
            spin_step();
        }
    }
}

type JobFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type LocalJobFuture = Pin<Box<dyn Future<Output = ()> + 'static>>;

static JOBS: Mutex<Vec<JobFuture>> = Mutex::new(Vec::new());
static JOBS_WAIT: WaitQueue = WaitQueue::new();

struct LocalJobQueue {
    jobs: Mutex<Vec<LocalJobFuture>>,
}

unsafe impl Sync for LocalJobQueue {}

static LOCAL_JOBS: LocalJobQueue = LocalJobQueue {
    jobs: Mutex::new(Vec::new()),
};

#[task]
pub async fn job_runner_task() {
    async move {
        loop {
            let job = {
                let mut jobs = LOCAL_JOBS.jobs.lock();
                if jobs.is_empty() {
                    None
                } else {
                    Some(jobs.remove(0))
                }
            };

            match job {
                Some(job) => job.await,
                None => {
                    let job = {
                        let mut jobs = JOBS.lock();
                        if jobs.is_empty() {
                            None
                        } else {
                            Some(jobs.remove(0))
                        }
                    };

                    match job {
                        Some(job) => job.await,
                        None => JOBS_WAIT.wait_for_event().await,
                    }
                }
            }
        }
    }
    .await;
}

fn enqueue_local_job(job: LocalJobFuture) {
    LOCAL_JOBS.jobs.lock().push(job);
    JOBS_WAIT.notify_one();
}

/// Enqueue a non-Send future to run on the local executor without waiting.
pub fn spawn_local_detached<F>(fut: F)
where
    F: Future<Output = ()> + 'static,
{
    enqueue_local_job(Box::pin(fut));
}

struct WaitState<T> {
    value: Mutex<Option<T>>,
    wait: WaitQueue,
}

/// Run a future on the async executor and wait synchronously for its result.
///
/// This accepts non-Send futures and must only be used on the single executor thread.
pub fn spawn_and_wait_local<F, T>(fut: F) -> T
where
    F: Future<Output = T> + 'static,
    T: 'static,
{
    let state = Arc::new(WaitState {
        value: Mutex::new(None),
        wait: WaitQueue::new(),
    });
    let state_task = state.clone();

    enqueue_local_job(Box::pin(async move {
        let out = fut.await;
        *state_task.value.lock() = Some(out);
        state_task.wait.notify_all();
    }));

    loop {
        if let Some(out) = state.value.lock().take() {
            return out;
        }
        state.wait.wait_for_event_blocking(0);
    }
}

pub enum Either<A, B> {
    First(A),
    Second(B),
}

/// Race two futures and resolve with whichever completes first.
///
/// If both are ready in the same poll, `a` wins.
pub async fn select2<A, B>(a: A, b: B) -> Either<A::Output, B::Output>
where
    A: Future,
    B: Future,
{
    let mut a = core::pin::pin!(a);
    let mut b = core::pin::pin!(b);

    core::future::poll_fn(|cx: &mut Context<'_>| {
        if let Poll::Ready(out) = a.as_mut().poll(cx) {
            return Poll::Ready(Either::First(out));
        }
        if let Poll::Ready(out) = b.as_mut().poll(cx) {
            return Poll::Ready(Either::Second(out));
        }
        Poll::Pending
    })
    .await
}
