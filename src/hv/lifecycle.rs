//! Execution parking is independent of checkpoint serialization and identity.
//! Only the Hull publishes Paused, after VM exit and host I/O quiescence.
use core::sync::atomic::{AtomicU8, Ordering};

const ACTIVE: u8 = 0;
const PAUSING: u8 = 1;
const PAUSED: u8 = 2;

pub(super) struct PauseControl(AtomicU8);

impl PauseControl {
    pub(super) const fn new() -> Self {
        Self(AtomicU8::new(ACTIVE))
    }

    pub(super) fn request(&self) -> bool {
        self.0
            .compare_exchange(ACTIVE, PAUSING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(super) fn acknowledge(&self) -> bool {
        self.0
            .compare_exchange(PAUSING, PAUSED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(super) fn resume(&self) -> bool {
        self.0.swap(ACTIVE, Ordering::AcqRel) != ACTIVE
    }

    pub(super) fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire) != ACTIVE
    }

    pub(super) fn paused(&self) -> bool {
        self.0.load(Ordering::Acquire) == PAUSED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_pause_resume_keeps_the_same_control() {
        let control = PauseControl::new();
        for _ in 0..128 {
            assert!(control.request());
            assert!(control.requested() && !control.paused());
            assert!(!control.request());
            assert!(control.acknowledge());
            assert!(control.paused());
            assert!(!control.acknowledge());
            assert!(control.resume());
            assert!(!control.requested() && !control.paused());
            assert!(!control.resume());
        }
    }

    #[test]
    fn resume_or_stop_before_checkpoint_cannot_publish_paused() {
        let control = PauseControl::new();
        assert!(control.request());
        assert!(control.resume());
        assert!(!control.acknowledge());
        assert!(!control.paused());
    }
}
