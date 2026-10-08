extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll, Waker};
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_time_driver::{TICK_HZ, now};
use spin::Mutex;
use trueos_executor::task;

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
    // Kernel queues outlive the caller's Blueprint realm. Native guest
    // continuations can reach this helper with guest allocation forced.
    crate::allocators::with_host_alloc_domain_strong(|| list.push(waker.clone()));
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
    if crate::r::threads::yield_now() {
        return;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        crate::hv::vmcall::guest_yield();
        return;
    }
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
    // Hull-private timer state must never wake host executor pointers, and
    // this critical-section path must not schedule while its lock is held.
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        crate::time::poll();
    }
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
    next_waiter: core::sync::atomic::AtomicU64,
    wakers: Mutex<Vec<QueuedWaker>>,
}

struct QueuedWaker {
    waiter: u64,
    waker: Waker,
}

/// Own exactly this future's registration, including cancellation. Waker
/// identity is insufficient: several waits can belong to the same task, and
/// a completed old future must not remove a newer registration for that task.
struct WaitRegistration<'a> {
    queue: &'a WaitQueue,
    waiter: u64,
}

impl<'a> WaitRegistration<'a> {
    fn new(queue: &'a WaitQueue) -> Self {
        let waiter = queue.next_waiter.fetch_add(1, Ordering::Relaxed);
        assert_ne!(waiter, 0, "wait registration identity exhausted");
        Self { queue, waiter }
    }

    fn register(&mut self, waker: &Waker) {
        let mut wakers = self.queue.wakers.lock();
        if let Some(entry) = wakers.iter_mut().find(|entry| entry.waiter == self.waiter) {
            if !entry.waker.will_wake(waker) { entry.waker = waker.clone(); }
        } else {
            crate::allocators::with_host_alloc_domain_strong(|| {
                wakers.push(QueuedWaker { waiter: self.waiter, waker: waker.clone() });
            });
        }
    }
}

impl Drop for WaitRegistration<'_> {
    fn drop(&mut self) {
        self.queue.wakers.lock().retain(|entry| entry.waiter != self.waiter);
    }
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            seq: AtomicU32::new(0),
            next_waiter: core::sync::atomic::AtomicU64::new(1),
            wakers: Mutex::new(Vec::new()),
        }
    }

    // Compatibility for a manually polled completion cell. Its completion
    // broadcasts to all registrations; asynchronous joins use owned waits.
    fn register_poll_waker(&self, waker: &Waker) {
        let mut wakers = self.wakers.lock();
        if !wakers.iter().any(|entry| entry.waiter == 0 && entry.waker.will_wake(waker)) {
            crate::allocators::with_host_alloc_domain_strong(|| {
                wakers.push(QueuedWaker { waiter: 0, waker: waker.clone() });
            });
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
        let mut registration = WaitRegistration::new(self);
        core::future::poll_fn(|cx: &mut Context<'_>| {
            if self.seq.load(Ordering::Acquire) != observed {
                return Poll::Ready(());
            }

            {
                registration.register(cx.waker());
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
        let mut registration = WaitRegistration::new(self);
        core::future::poll_fn(|cx: &mut Context<'_>| {
            if self.seq.load(Ordering::Acquire) != observed {
                return Poll::Ready(true);
            }

            {
                registration.register(cx.waker());
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
            waker.waker.wake();
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
            waker.waker.wake();
        }
        count
    }

    #[inline]
    pub async fn wait_for_event(&self) {
        let observed = self.observe();
        self.wait_after(observed).await;
    }

    #[inline]
    pub async fn wait_for_event_timeout(&self, timeout_ms: u64) -> bool {
        let observed = self.observe();
        // Preserve this API's zero-as-unbounded contract. Bounded waits must
        // actually poll a timer; checking now() alone supplies no wakeup.
        if timeout_ms == 0 {
            self.wait_after(observed).await;
            true
        } else {
            self.wait_after_timeout(observed, timeout_ms).await
        }
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
        if let Some(woke) = crate::r::threads::wait(self, observed, timeout_ms) {
            return woke;
        }
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
            self.wait.register_poll_waker(cx.waker());
        }

        if let Some(value) = self.try_take() {
            return Poll::Ready(value);
        }

        Poll::Pending
    }

    pub async fn join(&self) -> T {
        loop {
            let observed = self.wait.observe();
            if let Some(value) = self.try_take() { return value; }
            self.wait.wait_after(observed).await;
        }
    }

    pub fn join_blocking_parked(&self) -> T {
        loop {
            // Observe before testing the predicate: a completion between the
            // test and parking must change the generation we wait after.
            let observed = self.wait.seq.load(Ordering::Acquire);
            if let Some(value) = self.try_take() {
                return value;
            }
            self.wait.wait_for_event_after_blocking_parked(observed, 0);
        }
    }
}

type JobFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type LocalJobFuture = Pin<Box<dyn Future<Output = ()> + 'static>>;

static JOBS: Mutex<Vec<JobFuture>> = Mutex::new(Vec::new());
static JOBS_WAIT: WaitQueue = WaitQueue::new();
const PLATFORM_WAIT_HOST_SCOPE: u16 = 0;
pub(crate) const BLUEPRINT_IO_WAIT_KEY: u64 = 0x4250_494f_0000_0001;

static PLATFORM_WAIT_QUEUES: Mutex<BTreeMap<(u16, u64), Arc<WaitQueue>>> =
    Mutex::new(BTreeMap::new());

fn platform_wait_queue(scope: u16, key: u64) -> Arc<WaitQueue> {
    let scoped_key = (scope, key);
    if let Some(queue) = PLATFORM_WAIT_QUEUES.lock().get(&scoped_key).cloned() {
        return queue;
    }

    // Both the registry's BTree nodes and queue storage belong to the host.
    // An Arc also protects snapshots collected by a concurrent network wake
    // while teardown removes this VM's registry entries.
    crate::allocators::with_host_alloc_domain_strong(|| {
        let mut queues = PLATFORM_WAIT_QUEUES.lock();
        queues.entry(scoped_key).or_insert_with(|| Arc::new(WaitQueue::new())).clone()
    })
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
    platform_wait_after_parked(
        &platform_wait_queue(PLATFORM_WAIT_HOST_SCOPE, key),
        observed,
        timeout_ms,
    )
}

fn platform_wait_after_parked(queue: &WaitQueue, observed: u32, timeout_ms: u64) -> bool {
    // The exported platform contract uses zero for a nonblocking probe and
    // MAX for infinity. Internal WaitQueue users retain their zero=infinite API.
    if timeout_ms == 0 {
        return queue.seq.load(Ordering::Acquire) != observed;
    }
    queue.wait_for_event_after_blocking_parked(
        observed,
        if timeout_ms == u64::MAX {
            0
        } else {
            timeout_ms
        },
    )
}

#[cfg(test)]
mod native_completion_tests {
    extern crate std;
    use super::*;

    #[derive(Default)]
    struct WakeCount(core::sync::atomic::AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::Relaxed); }
        fn wake_by_ref(self: &Arc<Self>) { self.0.fetch_add(1, Ordering::Relaxed); }
    }

    #[test]
    fn cancelled_wait_does_not_steal_a_live_waiters_notification() {
        let queue = WaitQueue::new();
        let old = Arc::new(WakeCount::default());
        let live = Arc::new(WakeCount::default());
        let old_waker = Waker::from(old.clone());
        let live_waker = Waker::from(live.clone());
        let mut cancelled = Box::pin(queue.wait_after(queue.observe()));
        let mut waiting = Box::pin(queue.wait_after(queue.observe()));
        assert!(cancelled.as_mut().poll(&mut Context::from_waker(&old_waker)).is_pending());
        assert!(waiting.as_mut().poll(&mut Context::from_waker(&live_waker)).is_pending());
        drop(cancelled);
        assert!(queue.notify_one());
        assert_eq!(old.0.load(Ordering::Relaxed), 0, "cancelled waiter was still registered");
        assert_eq!(live.0.load(Ordering::Relaxed), 1, "live waiter never scheduled");
        assert!(waiting.as_mut().poll(&mut Context::from_waker(&live_waker)).is_ready());
    }

    #[test]
    fn generation_completion_unregisters_the_waiter_not_chosen_by_notify_one() {
        let queue = WaitQueue::new();
        let first = Arc::new(WakeCount::default());
        let second = Arc::new(WakeCount::default());
        let third = Arc::new(WakeCount::default());
        let a = Waker::from(first.clone());
        let b = Waker::from(second.clone());
        let c = Waker::from(third.clone());
        let mut one = Box::pin(queue.wait_after(queue.observe()));
        let mut two = Box::pin(queue.wait_after_timeout(queue.observe(), 60_000));
        assert!(one.as_mut().poll(&mut Context::from_waker(&a)).is_pending());
        assert!(two.as_mut().poll(&mut Context::from_waker(&b)).is_pending());
        let second_wakes_before_notify = second.0.load(Ordering::Relaxed);
        queue.notify_one();
        // A timer/spurious poll can observe the new generation even when
        // another waiter received the notification's actual scheduling wake.
        assert!(two.as_mut().poll(&mut Context::from_waker(&b)).is_ready());
        drop(two);
        let mut three = Box::pin(queue.wait_after(queue.observe()));
        assert!(three.as_mut().poll(&mut Context::from_waker(&c)).is_pending());
        queue.notify_one();
        assert_eq!(second.0.load(Ordering::Relaxed), second_wakes_before_notify);
        assert_eq!(third.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn completed_registration_cannot_remove_a_new_wait_with_the_same_task_waker() {
        let queue = WaitQueue::new();
        let count = Arc::new(WakeCount::default());
        let waker = Waker::from(count.clone());
        let mut old = Box::pin(queue.wait_after(queue.observe()));
        assert!(old.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        queue.notify_one();
        let mut new = Box::pin(queue.wait_after(queue.observe()));
        assert!(new.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        drop(old);
        queue.notify_one();
        assert_eq!(count.0.load(Ordering::Relaxed), 2);
        assert!(new.as_mut().poll(&mut Context::from_waker(&waker)).is_ready());
    }

    #[test]
    fn event_timeout_has_a_timer_wake_and_zero_remains_unbounded() {
        let queue = WaitQueue::new();
        let count = Arc::new(WakeCount::default());
        let waker = Waker::from(count.clone());
        let mut bounded = Box::pin(queue.wait_for_event_timeout(3));
        assert!(bounded.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        // The host timer supplies a scheduling wake on poll. The old wrapper
        // merely checked now(), with no deadline registered to wake it again.
        #[cfg(thread_scheduler_harness)]
        assert!(count.0.load(Ordering::Relaxed) > 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(bounded.as_mut().poll(&mut Context::from_waker(&waker)), Poll::Ready(false));
        drop(bounded);
        assert!(!queue.notify_one(), "completed timeout left a stale registration");
        let mut unbounded = Box::pin(queue.wait_for_event_timeout(0));
        assert!(unbounded.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        queue.notify_one();
        assert_eq!(unbounded.as_mut().poll(&mut Context::from_waker(&waker)), Poll::Ready(true));
    }

    #[test]
    fn completion_before_join_is_visible_without_parking() {
        let cell = CompletionCell::new();
        assert!(cell.complete(7).is_ok());
        assert_eq!(cell.join_blocking_parked(), 7);
    }

    #[test]
    fn completion_between_predicate_and_park_changes_observed_generation() {
        let cell = CompletionCell::new();
        let observed = cell.wait.observe();
        assert_eq!(cell.try_take(), None);
        assert!(cell.complete(9).is_ok());
        assert!(cell.wait.wait_for_event_after_blocking_parked(observed, 0));
        assert_eq!(cell.try_take(), Some(9));
    }

    #[test]
    fn exported_zero_timeout_probes_and_max_timeout_observes_a_prior_wake() {
        let queue = WaitQueue::new();
        let observed = queue.observe();
        assert!(!platform_wait_after_parked(&queue, observed, 0));
        queue.notify_all();
        assert!(platform_wait_after_parked(&queue, observed, 0));
        assert!(platform_wait_after_parked(&queue, observed, u64::MAX));
    }

    #[test]
    fn completion_after_async_registration_is_observed() {
        let cell = CompletionCell::new();
        let mut cx = Context::from_waker(Waker::noop());
        assert!(cell.poll_take(&mut cx).is_pending());
        assert!(cell.complete(11).is_ok());
        assert_eq!(cell.poll_take(&mut cx), Poll::Ready(11));
    }
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
    platform_wait_after_parked(
        &platform_wait_queue(platform_wait_vm_scope(vm_id), key),
        observed,
        timeout_ms,
    )
}

#[inline]
pub async fn platform_wait_after_for_vm_async(
    vm_id: u8,
    key: u64,
    observed: u32,
    timeout_ms: u64,
) -> bool {
    let queue = platform_wait_queue(platform_wait_vm_scope(vm_id), key);
    if timeout_ms == u64::MAX {
        queue.wait_after(observed).await;
        true
    } else {
        queue.wait_after_timeout(observed, timeout_ms).await
    }
}

#[inline]
pub fn platform_wake_one_for_vm(vm_id: u8, key: u64) -> bool {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key).notify_one()
}

#[inline]
pub fn platform_wake_all_for_vm(vm_id: u8, key: u64) -> usize {
    platform_wait_queue(platform_wait_vm_scope(vm_id), key).notify_all()
}

/// Advance every keyed wait generation already owned by one Blueprint VM.
/// Lifecycle control uses this when the Hull may be outside VMX in a platform wait.
pub fn platform_wake_vm_scope(vm_id: u8) -> usize {
    let scope = platform_wait_vm_scope(vm_id);
    let queues = crate::allocators::with_host_alloc_domain_strong(|| {
        let queues = PLATFORM_WAIT_QUEUES.lock();
        queues
            .iter()
            .filter_map(|(&(queue_scope, _), queue)| (queue_scope == scope).then(|| queue.clone()))
            .collect::<Vec<_>>()
    });
    let count = queues.len();
    for queue in queues {
        queue.notify_all();
    }
    count
}

/// Wake only existing Blueprint I/O queues. Network producers use this as a
/// coarse readiness edge; userspace poll/Mio re-probes exact descriptors.
pub fn platform_wake_all_blueprint_io_waiters() -> usize {
    let queues = crate::allocators::with_host_alloc_domain_strong(|| {
        let queues = PLATFORM_WAIT_QUEUES.lock();
        queues
            .iter()
            .filter_map(|(&(scope, key), queue)| {
                (scope != PLATFORM_WAIT_HOST_SCOPE && key == BLUEPRINT_IO_WAIT_KEY)
                    .then(|| queue.clone())
            })
            .collect::<Vec<_>>()
    });
    let mut woke = 0usize;
    for queue in queues {
        woke = woke.saturating_add(queue.notify_all());
    }
    woke
}

/// Call only after the Hull and all native guest jobs have finished. Existing
/// wake snapshots retain their own Arc, so removal cannot invalidate a scan.
/// Warm pause/preserve continuations keep their existing queue generations.
pub(crate) fn retire_platform_vm_waits(vm_id: u8) -> usize {
    crate::allocators::with_host_alloc_domain_strong(|| {
        let scope = platform_wait_vm_scope(vm_id);
        let mut queues = PLATFORM_WAIT_QUEUES.lock();
        let before = queues.len();
        queues.retain(|&(queue_scope, _), _| queue_scope != scope);
        before - queues.len()
    })
}

#[inline]
pub fn platform_wake_blueprint_io_for_vm(vm_id: u8) -> usize {
    platform_wake_all_for_vm(vm_id, BLUEPRINT_IO_WAIT_KEY)
}

#[cfg(test)]
mod platform_retirement_tests {
    use super::*;

    #[test]
    fn retirement_preserves_wake_snapshots_and_other_scopes() {
        const KEY: u64 = 0x7265_7469_7265_0001;
        let old = platform_wait_queue(platform_wait_vm_scope(210), KEY);
        let weak = Arc::downgrade(&old);
        let host = platform_wait_queue(PLATFORM_WAIT_HOST_SCOPE, KEY);
        let other = platform_wait_queue(platform_wait_vm_scope(211), KEY);
        let snapshot = old.clone();
        old.notify_all();
        assert_eq!(retire_platform_vm_waits(210), 1);
        snapshot.notify_all(); // racing network wake still owns valid storage
        let fresh = platform_wait_queue(platform_wait_vm_scope(210), KEY);
        assert!(!Arc::ptr_eq(&old, &fresh));
        assert_eq!(fresh.observe(), 0);
        assert!(Arc::ptr_eq(&host, &platform_wait_queue(PLATFORM_WAIT_HOST_SCOPE, KEY)));
        assert!(Arc::ptr_eq(&other, &platform_wait_queue(platform_wait_vm_scope(211), KEY)));
        drop(snapshot);
        drop(old);
        assert!(weak.upgrade().is_none());
        retire_platform_vm_waits(210);
        retire_platform_vm_waits(211);
    }
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
