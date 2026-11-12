use core::sync::atomic::{AtomicU64, Ordering};

/// Wake notifications are generations, not completion permits. Only the WD
/// status register proves completion for the currently owned capture.
pub(super) struct Generation(AtomicU64);

impl Generation {
    pub(super) const fn new() -> Self {
        Self(AtomicU64::new(0))
    }
    pub(super) fn observe(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
    pub(super) fn notify(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) fn changed_after_register(&self, observed: u64, register: impl FnOnce()) -> bool {
        if self.observe() != observed {
            return true;
        }
        register();
        self.observe() != observed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupt_between_empty_check_and_registration_cannot_be_lost() {
        let generation = Generation::new();
        let observed = generation.observe();
        generation.notify();
        assert!(generation.changed_after_register(observed, || panic!("already changed")));
    }
    #[test]
    fn interrupt_during_registration_cannot_be_lost() {
        let generation = Generation::new();
        let observed = generation.observe();
        assert!(generation.changed_after_register(observed, || generation.notify()));
    }
    #[test]
    fn old_notifications_are_not_permits_for_a_new_capture() {
        let generation = Generation::new();
        generation.notify();
        generation.notify();
        let observed = generation.observe();
        assert!(!generation.changed_after_register(observed, || {}));
        generation.notify();
        assert!(generation.changed_after_register(observed, || {}));
    }
}
