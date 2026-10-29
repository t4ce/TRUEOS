//! Execution hints are not timed sleeps. Keep a timer-backed escape from
//! native spin loops, and use an executor turn for explicit cooperative yield.

/// Select PAUSE interception only when it is needed as the fallback host
/// scheduling boundary. Hardware-required control bits are applied afterward.
pub(super) const fn intercept_pause(preemption_timer_enabled: bool, native_pause: bool) -> bool {
    !preemption_timer_enabled || !native_pause
}

/// Bound one executor turn across VM exits that immediately resume the guest.
/// The VMX timer is reloaded on entry, so frequent short VMCALLs alone cannot
/// enforce this bound. Only a host executor yield restarts this budget.
pub(super) struct ExecutorBudget {
    started_ms: u64,
    quantum_ms: u64,
}

impl ExecutorBudget {
    pub(super) fn new(now_ms: u64, quantum_ms: u64) -> Self {
        Self { started_ms: now_ms, quantum_ms: quantum_ms.max(1) }
    }

    pub(super) fn expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.started_ms) >= self.quantum_ms
    }

    pub(super) fn restart(&mut self, now_ms: u64) {
        self.started_ms = now_ms;
    }
}

/// Give other ready tasks a turn without imposing a minimum wall-clock delay.
/// The caller must clear its current-VM identity before awaiting this future.
pub(crate) async fn yield_executor_turn() {
    let mut yielded = false;
    core::future::poll_fn(|cx| {
        if yielded {
            core::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Context, Poll, Wake, Waker};

    #[test]
    fn pause_stays_intercepted_without_a_timer_or_with_replay_enabled() {
        assert!(!intercept_pause(true, true));
        assert!(intercept_pause(false, true));
        assert!(intercept_pause(true, false));
        assert!(intercept_pause(false, false));
    }

    struct WakeCount(AtomicUsize);
    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn yield_suspends_once_and_wakes_itself_without_a_timer() {
        use core::future::Future;
        let count = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let mut cx = Context::from_waker(&waker);
        let mut future = core::pin::pin!(yield_executor_turn());
        assert_eq!(future.as_mut().poll(&mut cx), Poll::Pending);
        assert_eq!(count.0.load(Ordering::Relaxed), 1);
        assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(()));
        assert_eq!(count.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn frequent_immediate_vm_exits_cannot_extend_the_executor_turn() {
        let mut budget = ExecutorBudget::new(0, 125);
        let mut host_turns = Vec::new();
        // Model 100 short VMCALLs per millisecond. Checking admission on every
        // resume must still give the host eight turns during this second.
        for now_ms in 0..=1_000 {
            for _ in 0..100 {
                if budget.expired(now_ms) {
                    host_turns.push(now_ms);
                    budget.restart(now_ms);
                }
            }
        }
        assert_eq!(host_turns, [125, 250, 375, 500, 625, 750, 875, 1_000]);
    }

    #[test]
    fn resuming_after_a_host_turn_gets_one_fresh_quantum() {
        let mut budget = ExecutorBudget::new(0, 125);
        assert!(budget.expired(125));
        // Other executor work can take longer than a quantum. Restart at the
        // actual resumption time; there is no debt requiring catch-up yields.
        budget.restart(1_000);
        assert!(!budget.expired(1_000));
        assert!(!budget.expired(1_124));
        assert!(budget.expired(1_125));
    }

    #[test]
    fn executor_budgets_are_local_to_each_vm_run() {
        let first = ExecutorBudget::new(10, 125);
        let next = ExecutorBudget::new(130, 125);
        assert!(first.expired(135));
        assert!(!next.expired(135));
    }
}
