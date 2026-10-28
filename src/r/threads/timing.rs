//! Instrument Codex application continuations in either placement mode. The wrapped
//! waker preserves executor identity through will_wake and forwards every wake.
use crate::r::input_timing::{Site, now_ms};
use crate::r::blocking::diagnostics::TimingProbe;
use alloc::{sync::Arc, task::Wake};
use core::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    task::{Context, Waker},
};

static READY: Site = Site::new("codex-ready-to-poll");
static SLICE: Site = Site::new("codex-run-slice");
static SLEEP: Site = Site::new("codex-sleep-overrun");
static WAIT: Site = Site::new("codex-wait-elapsed");

struct Signal {
    ready: AtomicU64,
    executor: spin::Mutex<Waker>,
}
impl Signal {
    fn notify(&self) {
        // Preserve the earliest outstanding wake. Zero means no pending wake.
        let _ = self.ready.compare_exchange(
            0,
            now_ms().saturating_add(1),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        let executor = self.executor.lock().clone();
        executor.wake();
    }
}
impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.notify();
    }
}

pub(super) struct Timing {
    signal: Arc<Signal>,
    waker: Waker,
    wait: Option<(u64, Option<u64>)>,
    first_poll: AtomicBool,
    probe: Option<TimingProbe>,
}
impl Timing {
    pub(super) fn new(cx: &Context<'_>, submitted: u64, probe: Option<TimingProbe>) -> Self {
        let signal = Arc::new(Signal {
            ready: AtomicU64::new(submitted.saturating_add(1)),
            executor: spin::Mutex::new(cx.waker().clone()),
        });
        let waker = Waker::from(signal.clone());
        Self {
            signal,
            waker,
            wait: None,
            first_poll: AtomicBool::new(true),
            probe,
        }
    }
    pub(super) fn begin_poll(&self, cx: &Context<'_>) {
        {
            let mut executor = self.signal.executor.lock();
            if !executor.will_wake(cx.waker()) {
                *executor = cx.waker().clone();
            }
        }
        let ready = self.signal.ready.swap(0, Ordering::Relaxed);
        if ready != 0 {
            let elapsed = now_ms().saturating_sub(ready - 1);
            if let Some(probe) = self.probe() {
                probe.polled(elapsed, self.first_poll.swap(false, Ordering::Relaxed));
            } else {
                READY.record(elapsed);
            }
        }
    }
    pub(super) fn waker(&self) -> Waker {
        self.waker.clone()
    }
    fn probe(&self) -> Option<&TimingProbe> {
        // The independent observer owns the output budget for live probes.
        // Preserve the legacy input-timing site logs when probes are disabled.
        if crate::allcaps::hv::CODEX_PROGRESS_PROBES_ENABLED { self.probe.as_ref() } else { None }
    }
    pub(super) fn begin_wait(&mut self, sleep_ms: Option<u64>) {
        self.wait = Some((now_ms(), sleep_ms));
    }
    pub(super) fn resumed(&mut self) {
        if let Some((since, sleep)) = self.wait.take() {
            let elapsed = now_ms().saturating_sub(since);
            if let Some(probe) = self.probe() {
                probe.parked(elapsed, sleep);
            } else {
                match sleep {
                    Some(requested) => SLEEP.record(elapsed.saturating_sub(requested)),
                    None => WAIT.record(elapsed),
                }
            }
        }
    }
    pub(super) fn ran(&self, since: u64) {
        let elapsed = now_ms().saturating_sub(since);
        if let Some(probe) = self.probe() {
            probe.ran(elapsed);
        } else {
            SLICE.record(elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicUsize;
    struct Counter(AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    #[test]
    fn wrapped_wakes_preserve_earliest_readiness_and_forward_every_notification() {
        let count = Arc::new(Counter(AtomicUsize::new(0)));
        let executor = Waker::from(count.clone());
        let timing = Timing::new(&Context::from_waker(&executor), now_ms(), None);
        let first = timing.signal.ready.load(Ordering::Relaxed);
        let waker = timing.waker();
        assert!(waker.will_wake(&timing.waker()));
        waker.wake_by_ref();
        waker.wake_by_ref();
        assert_eq!(timing.signal.ready.load(Ordering::Relaxed), first);
        assert_eq!(count.0.load(Ordering::Relaxed), 2);
        timing.begin_poll(&Context::from_waker(&executor));
        assert_eq!(timing.signal.ready.load(Ordering::Relaxed), 0);
        waker.wake_by_ref();
        assert_ne!(timing.signal.ready.load(Ordering::Relaxed), 0);
        assert_eq!(count.0.load(Ordering::Relaxed), 3);
        let next_count = Arc::new(Counter(AtomicUsize::new(0)));
        let next_executor = Waker::from(next_count.clone());
        timing.begin_poll(&Context::from_waker(&next_executor));
        waker.wake_by_ref();
        assert_eq!(next_count.0.load(Ordering::Relaxed), 1);
        assert_eq!(count.0.load(Ordering::Relaxed), 3);
    }
}
