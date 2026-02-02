use core::future::{poll_fn, Future};
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};

use embassy_executor::{task, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

const TIMEOUT_MS: u64 = 400;
const TICK_MS: u64 = 1;

// Flip these to true to demonstrate the failure modes.
const RUN_RACY_LOST_WAKEUP: bool = false;
const RUN_DEADLOCK: bool = false;

static LOST_WAKEUP_DONE: AtomicBool = AtomicBool::new(false);
static DEADLOCK_DONE: AtomicBool = AtomicBool::new(false);

static DEADLOCK_LOCK: Mutex<LockState> = Mutex::new(LockState {
    ready: false,
    waker: None,
});

#[task]
pub(crate) async fn sched_challenge_task(spawner: Spawner) {
    crate::log!("sched-challenge: starting\n");

    let _ = spawner.spawn(timeout_task(
        &LOST_WAKEUP_DONE,
        "lost-wakeup",
        TIMEOUT_MS,
    ));
    let _ = spawner.spawn(timeout_task(&DEADLOCK_DONE, "deadlock", TIMEOUT_MS));

    run_lost_wakeup().await;
    LOST_WAKEUP_DONE.store(true, Ordering::Release);
    crate::log!("sched-challenge: lost-wakeup ok\n");

    run_deadlock(spawner).await;
    DEADLOCK_DONE.store(true, Ordering::Release);
    crate::log!("sched-challenge: deadlock ok\n");

    crate::log!("sched-challenge: done\n");
}

#[task]
async fn timeout_task(flag: &'static AtomicBool, label: &'static str, timeout_ms: u64) {
    Timer::after(EmbassyDuration::from_millis(timeout_ms)).await;
    if !flag.load(Ordering::Acquire) {
        panic!("sched-challenge: {} timeout", label);
    }
}

async fn run_lost_wakeup() {
    let event = InjectedIrqEvent::new();

    if RUN_RACY_LOST_WAKEUP {
        poll_fn(|cx| event.poll_racy(cx)).await;
        return;
    }

    poll_fn(|cx| event.poll_checked(cx)).await;
}

async fn run_deadlock(spawner: Spawner) {
    {
        let mut state = DEADLOCK_LOCK.lock();
        state.ready = false;
        state.waker = None;
    }

    let _ = spawner.spawn(deadlock_signaler_task());

    if RUN_DEADLOCK {
        // This holds the lock across await and prevents the signaler from ever acquiring it.
        let mut fut = HoldLockWait::new(&DEADLOCK_LOCK);
        fut.await;
        return;
    }

    wait_without_lock().await;
}

#[task]
async fn deadlock_signaler_task() {
    loop {
        if let Some(mut state) = DEADLOCK_LOCK.try_lock() {
            state.ready = true;
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
            break;
        }
        Timer::after(EmbassyDuration::from_millis(TICK_MS)).await;
    }
}

async fn wait_without_lock() {
    poll_fn(|cx| {
        let mut state = DEADLOCK_LOCK.lock();
        if state.ready {
            return Poll::Ready(());
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    })
    .await;
}

struct InjectedIrqEvent {
    ready: AtomicBool,
    inject: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl InjectedIrqEvent {
    fn new() -> Self {
        Self {
            ready: AtomicBool::new(false),
            inject: AtomicBool::new(true),
            waker: Mutex::new(None),
        }
    }

    fn poll_racy(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.ready.load(Ordering::Acquire) {
            return Poll::Ready(());
        }

        if self.inject.swap(false, Ordering::AcqRel) {
            self.ready.store(true, Ordering::Release);
            self.wake();
        }

        *self.waker.lock() = Some(cx.waker().clone());
        Poll::Pending
    }

    fn poll_checked(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.ready.load(Ordering::Acquire) {
            return Poll::Ready(());
        }

        if self.inject.swap(false, Ordering::AcqRel) {
            self.ready.store(true, Ordering::Release);
            self.wake();
        }

        *self.waker.lock() = Some(cx.waker().clone());

        if self.ready.load(Ordering::Acquire) {
            return Poll::Ready(());
        }

        Poll::Pending
    }

    fn wake(&self) {
        if let Some(waker) = self.waker.lock().take() {
            waker.wake();
        }
    }
}

struct LockState {
    ready: bool,
    waker: Option<Waker>,
}

struct HoldLockWait<'a> {
    lock: &'a Mutex<LockState>,
    guard: Option<spin::MutexGuard<'a, LockState>>,
}

impl<'a> HoldLockWait<'a> {
    fn new(lock: &'a Mutex<LockState>) -> Self {
        Self { lock, guard: None }
    }
}

impl Future for HoldLockWait<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.guard.is_none() {
            self.guard = Some(self.lock.lock());
        }

        let state = self.guard.as_mut().expect("lock guard missing");
        if state.ready {
            return Poll::Ready(());
        }

        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}
