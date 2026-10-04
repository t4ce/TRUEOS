//! Stop admission handshake for process-shaped Blueprints with persistent jobs.
//! Registration and stop race through one atomic: a late registration cannot
//! turn an already accepted immediate stop into a cooperative one.
use core::sync::atomic::{AtomicU8, Ordering};

const REGISTERED: u8 = 1;
const REQUESTED: u8 = 2;

pub(crate) struct CooperativeStop(AtomicU8);
impl CooperativeStop {
    pub(crate) const fn new() -> Self {
        Self(AtomicU8::new(0))
    }
    pub(crate) fn reset(&self) {
        self.0.store(0, Ordering::Release);
    }
    pub(crate) fn register(&self) -> bool {
        self.0
            .compare_exchange(0, REGISTERED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    /// True means the guest must retain execution and admission for cleanup.
    pub(crate) fn request(&self) -> bool {
        self.0.fetch_or(REQUESTED, Ordering::AcqRel) & REGISTERED != 0
    }
    pub(crate) fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire) & REQUESTED != 0
    }
    pub(crate) fn registered(&self) -> bool {
        self.0.load(Ordering::Acquire) & REGISTERED != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_stop_retains_execution_until_guest_shutdown() {
        let stop = CooperativeStop::new();
        assert!(stop.register());
        assert!(!stop.requested());
        assert!(stop.request());
        assert!(stop.registered() && stop.requested());
        assert!(stop.request()); // repeated requests preserve the handshake
    }
    #[test]
    fn stop_before_registration_cannot_become_cooperative() {
        let stop = CooperativeStop::new();
        assert!(!stop.request());
        assert!(!stop.register());
        assert!(!stop.registered());
        assert!(stop.requested());
    }
    #[test]
    fn only_one_cleanup_owner_and_reset_only_for_new_incarnation() {
        let stop = CooperativeStop::new();
        assert!(stop.register());
        assert!(!stop.register());
        assert!(stop.request());
        stop.reset();
        assert!(!stop.registered() && !stop.requested());
        assert!(stop.register());
    }
    #[test]
    fn concurrent_registration_and_stop_have_one_consistent_winner() {
        for _ in 0..128 {
            let stop = std::sync::Arc::new(CooperativeStop::new());
            let registration = stop.clone();
            let job = std::thread::spawn(move || registration.register());
            let cooperative = stop.request();
            assert_eq!(job.join().unwrap(), cooperative);
            assert!(stop.requested());
        }
    }
}
