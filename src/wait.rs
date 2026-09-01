extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use core::task::{Context, Poll, Waker};
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_time_driver::{TICK_HZ, now};
use spin::Mutex;
use trueos_executor::task;

// A synchronous service-lane wait has no executor waker: its call stack is
// still inside the task currently being polled. Publish the waited-on queue by
// CPU so notify can route an IPI which breaks that CPU's `sti; hlt`. Registering
// happens before the final sequence check, closing the classic lost-wake race.
static BLOCKING_PARKED_WAIT_BY_CPU: [AtomicUsize; crate::percpu::CPU_SLOT_LIMIT] =
    [const { AtomicUsize::new(0) }; crate::percpu::CPU_SLOT_LIMIT];

struct BlockingParkRegistration {
    slot: usize,
    queue: usize,
}

impl BlockingParkRegistration {
    fn publish(queue: &WaitQueue) -> Option<Self> {
        if !crate::r::blocking::current_service_lane_job_active() {
            return None;
        }
        let slot = crate::percpu::current_slot();
        let parked = BLOCKING_PARKED_WAIT_BY_CPU.get(slot)?;
        let queue = queue as *const WaitQueue as usize;
        // SeqCst pairs with notify's sequence increment and parked-slot scan:
        // either the notifier observes this publication and sends an IPI, or
        // the final SeqCst generation check observes the notification.
        parked.store(queue, Ordering::SeqCst);
        Some(Self { slot, queue })
    }
}

impl Drop for BlockingParkRegistration {
    fn drop(&mut self) {
        let _ = BLOCKING_PARKED_WAIT_BY_CPU[self.slot].compare_exchange(
            self.queue,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }
}

fn wake_blocking_parked(queue: &WaitQueue, wake_all: bool) -> usize {
    let queue = queue as *const WaitQueue as usize;
    let mut count = 0usize;
    for (slot, parked) in BLOCKING_PARKED_WAIT_BY_CPU.iter().enumerate() {
        if parked.load(Ordering::SeqCst) != queue {
            continue;
        }
        if crate::remote_work_wake::wake_cpu_for_remote_work(slot as u32) {
            count += 1;
        }
        if !wake_all {
            break;
        }
    }
    count
}

/// Embassy sync primitives are generic over a raw blocking mutex backend.
/// This adapts the kernel's `spin::Mutex` so shared Embassy types like
/// `Watch` can live in static storage without each subsystem redefining the
/// same glue type.
pub struct EmbassySpinRawMutex(Mutex<()>);

unsafe impl RawMutex for EmbassySpinRawMutex {
    const INIT: Self = Self(Mutex::new(()));

    fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.lock();
        f()
    }
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

/// Spin until `condition` is true or the timeout expires.
#[inline]
pub fn spin_until_timeout<F: FnMut() -> bool>(timeout_ms: u64, mut condition: F) -> bool {
    let hz = TICK_HZ;
    let ticks = if hz == 0 {
        0
    } else {
        timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
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
    let hz = TICK_HZ;
    let ticks = if hz == 0 {
        0
    } else {
        timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
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

    /// Observe the current notification generation before checking the state
    /// protected by this wait queue.
    ///
    /// Queue consumers must take this snapshot *before* checking for work and
    /// pass it to [`Self::wait_after`] only when that check is empty. A notify
    /// racing anywhere between those two operations then changes the
    /// generation and prevents a lost wake.
    #[inline]
    pub fn observe(&self) -> u32 {
        self.seq.load(Ordering::Acquire)
    }

    /// Wait until a notification newer than `observed` exists.
    ///
    /// Notifications are generation changes rather than reserved permits, so
    /// callers must always loop and recheck their own queue after this returns.
    #[inline]
    pub async fn wait_after(&self, observed: u32) {
        core::future::poll_fn(|cx: &mut Context<'_>| {
            if self.seq.load(Ordering::Acquire) != observed {
                return Poll::Ready(());
            }

            {
                let mut wakers = self.wakers.lock();
                if self.seq.load(Ordering::Acquire) != observed {
                    return Poll::Ready(());
                }
                register_waker_list(&mut wakers, cx.waker());
            }

            if self.seq.load(Ordering::Acquire) != observed {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await
    }

    /// Wait asynchronously for a generation change or a bounded timeout.
    ///
    /// This is the async counterpart to the parked blocking wait. It is meant
    /// for parent executor tasks (notably VMX runtime lanes) that must suspend
    /// without occupying their AP while a child runtime observes a synchronous
    /// wait contract. The caller still owns the protected state and must
    /// recheck it after a `true` return.
    #[inline]
    pub async fn wait_after_timeout(&self, observed: u32, timeout_ms: u64) -> bool {
        if self.seq.load(Ordering::Acquire) != observed {
            return true;
        }
        if timeout_ms == 0 {
            return false;
        }

        let mut timeout = core::pin::pin!(trueos_time::Timer::after_millis(timeout_ms));
        core::future::poll_fn(|cx: &mut Context<'_>| {
            if self.seq.load(Ordering::Acquire) != observed {
                return Poll::Ready(true);
            }

            {
                let mut wakers = self.wakers.lock();
                if self.seq.load(Ordering::Acquire) != observed {
                    return Poll::Ready(true);
                }
                register_waker_list(&mut wakers, cx.waker());
            }

            if self.seq.load(Ordering::Acquire) != observed {
                return Poll::Ready(true);
            }

            match timeout.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(false),
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }

    #[inline]
    pub fn notify_one(&self) -> bool {
        self.seq.fetch_add(1, Ordering::SeqCst);
        // Prefer the synchronous CPU which may actually be in `hlt`. Wakers
        // left behind by a completed timed future must not consume the only
        // notification while that CPU remains asleep.
        if wake_blocking_parked(self, false) != 0 {
            return true;
        }
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
        self.seq.fetch_add(1, Ordering::SeqCst);
        let wakers = {
            let mut wakers = self.wakers.lock();
            core::mem::take(&mut *wakers)
        };
        let count = wakers.len();
        for waker in wakers {
            waker.wake();
        }
        count + wake_blocking_parked(self, true)
    }

    #[inline]
    pub async fn wait_for_event(&self) {
        let observed = self.observe();
        self.wait_after(observed).await;
    }

    #[inline]
    pub async fn wait_for_event_timeout(&self, timeout_ms: u64) -> bool {
        let hz = TICK_HZ;
        let ticks = if hz == 0 || timeout_ms == 0 {
            0
        } else {
            timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
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
                register_waker_list(&mut wakers, cx.waker());
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
        let hz = TICK_HZ;
        let ticks = if hz == 0 || timeout_ms == 0 {
            0
        } else {
            timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
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

    #[inline]
    pub fn wait_for_event_blocking_parked(&self, timeout_ms: u64) -> bool {
        let observed = self.seq.load(Ordering::Acquire);
        self.wait_for_event_after_blocking_parked(observed, timeout_ms)
    }

    #[inline]
    pub fn wait_for_event_after_blocking_parked(&self, observed: u32, timeout_ms: u64) -> bool {
        let hz = TICK_HZ;
        let ticks = if hz == 0 || timeout_ms == 0 {
            0
        } else {
            timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
        };
        let deadline = if ticks == 0 {
            0
        } else {
            now().saturating_add(ticks)
        };
        loop {
            if ticks != 0 && now() >= deadline {
                return false;
            }

            let current = self.seq.load(Ordering::Acquire);
            if current != observed {
                return true;
            }

            if let Some(_registration) = BlockingParkRegistration::publish(self) {
                // Wake no later than this wait's own timeout, but preserve the
                // executor runtime's earlier global timer budget as well.
                let wait_ticks = if ticks == 0 {
                    u64::MAX
                } else {
                    deadline.saturating_sub(now())
                };
                let sleep_ticks =
                    wait_ticks.min(crate::time::ticks_until_next_wake().unwrap_or(u64::MAX));
                let parked =
                    crate::runtime::park_local_executor_blocking_if_idle(sleep_ticks, || {
                        self.seq.load(Ordering::SeqCst) == observed
                            && (ticks == 0 || now() < deadline)
                    });
                if parked {
                    // Materialize due timer wakers before deciding whether the
                    // next pass may return to `hlt`.
                    crate::time::poll();
                    continue;
                }
            }

            // Parked blocking waits are used by runtime/platform primitives that
            // may already be inside a Tokio enter guard. Polling the local
            // executor here can re-enter another carrier job on the same TLS lane
            // and make Tokio's enter guards unwind out of LIFO order. These
            // waits rely on explicit notify/timeout progress instead.
            spin_step_no_exec();
        }
    }
}

/// Completion cell for TRUEOS scheduled work.
///
/// This is the kernel-side join primitive: spawned work writes exactly one
/// result, and joiners await or poll that result. Dropping a handle is detach;
/// the scheduled work keeps running and any unobserved result is simply dropped.
pub struct CompletionCell<T> {
    value: Mutex<Option<T>>,
    wait: WaitQueue,
}

impl<T> CompletionCell<T> {
    pub const fn new() -> Self {
        Self {
            value: Mutex::new(None),
            wait: WaitQueue::new(),
        }
    }

    pub fn complete(&self, value: T) -> Result<(), T> {
        let mut slot = self.value.lock();
        if slot.is_some() {
            return Err(value);
        }
        *slot = Some(value);
        drop(slot);
        self.wait.notify_all();
        Ok(())
    }

    pub fn try_take(&self) -> Option<T> {
        self.value.lock().take()
    }

    pub fn poll_take(&self, cx: &mut Context<'_>) -> Poll<T> {
        if let Some(value) = self.try_take() {
            return Poll::Ready(value);
        }

        {
            let mut wakers = self.wait.wakers.lock();
            register_waker_list(&mut wakers, cx.waker());
        }

        if let Some(value) = self.try_take() {
            return Poll::Ready(value);
        }

        Poll::Pending
    }

    pub async fn join(&self) -> T {
        core::future::poll_fn(|cx| self.poll_take(cx)).await
    }

    pub fn join_blocking_parked(&self) -> T {
        loop {
            if let Some(value) = self.try_take() {
                return value;
            }
            self.wait.wait_for_event_blocking_parked(0);
        }
    }
}

type JobFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type LocalJobFuture = Pin<Box<dyn Future<Output = ()> + 'static>>;

static JOBS: Mutex<Vec<JobFuture>> = Mutex::new(Vec::new());
static JOBS_WAIT: WaitQueue = WaitQueue::new();
const PLATFORM_WAIT_HOST_SCOPE: u16 = 0;

static PLATFORM_WAIT_QUEUES: Mutex<BTreeMap<(u16, u64), &'static WaitQueue>> =
    Mutex::new(BTreeMap::new());

fn platform_wait_queue(scope: u16, key: u64) -> &'static WaitQueue {
    let scoped_key = (scope, key);
    if let Some(queue) = PLATFORM_WAIT_QUEUES.lock().get(&scoped_key).copied() {
        return queue;
    }

    let queue = Box::leak(Box::new(WaitQueue::new()));
    let mut queues = PLATFORM_WAIT_QUEUES.lock();
    *queues.entry(scoped_key).or_insert(queue)
}

#[inline]
const fn platform_wait_vm_scope(vm_id: u8) -> u16 {
    vm_id as u16 + 1
}

#[inline]
pub fn platform_wait_observe(key: u64) -> u32 {
    platform_wait_queue(PLATFORM_WAIT_HOST_SCOPE, key)
        .seq
        .load(Ordering::Acquire)
}

#[inline]
pub fn platform_wait_after(key: u64, observed: u32, timeout_ms: u64) -> bool {
    platform_wait_queue(PLATFORM_WAIT_HOST_SCOPE, key)
        .wait_for_event_after_blocking_parked(observed, timeout_ms)
}

#[inline]
pub fn platform_wake_one(key: u64) -> bool {
    platform_wait_queue(PLATFORM_WAIT_HOST_SCOPE, key).notify_one()
}

#[inline]
pub fn platform_wake_all(key: u64) -> usize {
    platform_wait_queue(PLATFORM_WAIT_HOST_SCOPE, key).notify_all()
}

#[inline]
pub fn platform_wait_observe_for_vm(vm_id: u8, key: u64) -> u32 {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key)
        .seq
        .load(Ordering::Acquire)
}

#[inline]
pub fn platform_wait_after_for_vm(vm_id: u8, key: u64, observed: u32, timeout_ms: u64) -> bool {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key)
        .wait_for_event_after_blocking_parked(observed, timeout_ms)
}

#[inline]
pub async fn platform_wait_after_for_vm_async(
    vm_id: u8,
    key: u64,
    observed: u32,
    timeout_ms: u64,
) -> bool {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key)
        .wait_after_timeout(observed, timeout_ms)
        .await
}

#[inline]
pub fn platform_wake_one_for_vm(vm_id: u8, key: u64) -> bool {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key).notify_one()
}

#[inline]
pub fn platform_wake_all_for_vm(vm_id: u8, key: u64) -> usize {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key).notify_all()
}

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

/// Enqueue a non-Send future to run on the local executor without observation.
///
/// Detached here means no completion cell is created. It is unrelated to
/// `pthread_detach`; the queued future still runs to completion.
pub fn spawn_local_detached<F>(fut: F)
where
    F: Future<Output = ()> + 'static,
{
    enqueue_local_job(Box::pin(fut));
}
