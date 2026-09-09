//! A short-lived reorder guard, not a persistent sender identity.

pub(crate) const IDLE_RESET_MS: u64 = 2_000;

#[derive(Copy, Clone, Debug)]
pub(crate) struct SequenceWindow {
    last_seq: u32,
    last_accepted_ms: u64,
}

impl SequenceWindow {
    pub(crate) fn new(seq: u32, now_ms: u64) -> Self {
        Self {
            last_seq: seq,
            last_accepted_ms: now_ms,
        }
    }

    pub(crate) fn expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_accepted_ms) >= IDLE_RESET_MS
    }

    pub(crate) fn accept(&mut self, seq: u32, now_ms: u64) -> bool {
        // Rejected packets do not extend this window: a restarted sender must
        // recover even when it immediately resumes sending a lower counter.
        let forward = seq.wrapping_sub(self.last_seq);
        if !self.expired(now_ms) && (forward == 0 || forward >= 1 << 31) {
            return false;
        }
        *self = Self::new(seq, now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_and_reordering_are_dropped_within_the_window() {
        let mut state = SequenceWindow::new(100, 0);
        assert!(!state.accept(100, 1));
        assert!(!state.accept(99, 2));
        assert!(state.accept(101, 3));
    }

    #[test]
    fn sequence_one_has_no_special_power() {
        let mut state = SequenceWindow::new(100, 0);
        assert!(!state.accept(1, 1));
        assert!(state.accept(1, IDLE_RESET_MS));
        assert!(!state.accept(1, IDLE_RESET_MS + 1));
        assert!(state.accept(2, IDLE_RESET_MS + 2));
    }

    #[test]
    fn any_counter_can_resume_after_two_seconds() {
        for seq in [0, 1, 25, 100, u32::MAX] {
            let mut state = SequenceWindow::new(100, 10);
            assert!(state.accept(seq, 10 + IDLE_RESET_MS));
        }
    }

    #[test]
    fn continuous_restart_packets_cannot_keep_the_sender_stale_forever() {
        let mut state = SequenceWindow::new(500_000, 0);
        for tick in 1..20 {
            assert!(!state.accept(tick, u64::from(tick) * 100));
        }
        assert!(state.accept(20, 2_000));
        assert!(state.accept(21, 2_001));
    }

    #[test]
    fn counter_wrap_does_not_require_a_restart() {
        let mut state = SequenceWindow::new(u32::MAX, 0);
        assert!(state.accept(0, 1));
        assert!(state.accept(1, 2));
        assert!(!state.accept(u32::MAX, 3));
    }

    #[test]
    fn independent_streams_do_not_keep_each_other_alive() {
        let mut pointer = SequenceWindow::new(500, 0);
        let mut control = SequenceWindow::new(500, 0);
        assert!(control.accept(501, 1_999));
        assert!(pointer.accept(3, 2_000));
        assert!(!control.accept(3, 2_000));
    }
}
