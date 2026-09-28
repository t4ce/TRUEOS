//! Execution hints are not timed sleeps. Keep a timer-backed escape from
//! native spin loops, and use an executor turn for explicit cooperative yield.

/// Select PAUSE interception only when it is needed as the fallback host
/// scheduling boundary. Hardware-required control bits are applied afterward.
pub(super) const fn intercept_pause(preemption_timer_enabled: bool, native_pause: bool) -> bool {
    !preemption_timer_enabled || !native_pause
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
}
