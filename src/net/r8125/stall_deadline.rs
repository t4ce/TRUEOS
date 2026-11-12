//! A stalled interrupt-driven TX ring is measured in time, not IRQ count.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StallAction { None, Kick, Reset }

pub(super) struct StallDeadline {
    started_ms: Option<u64>,
    kicked_ms: Option<u64>,
}

impl StallDeadline {
    pub(super) const fn new() -> Self { Self { started_ms: None, kicked_ms: None } }

    pub(super) fn progress(&mut self) { *self = Self::new(); }

    pub(super) fn pending(&mut self, now_ms: u64) -> StallAction {
        let started = *self.started_ms.get_or_insert(now_ms);
        if now_ms.saturating_sub(started) >= 50_000 { return StallAction::Reset; }
        if now_ms.saturating_sub(self.kicked_ms.unwrap_or(started)) >= 10_000 {
            self.kicked_ms = Some(now_ms);
            StallAction::Kick
        } else { StallAction::None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frequent_irqs_do_not_accelerate_a_stall_deadline() {
        let mut clock = StallDeadline::new();
        for _ in 0..100_000 { assert_eq!(clock.pending(42), StallAction::None); }
        assert_eq!(clock.pending(10_041), StallAction::None);
        assert_eq!(clock.pending(10_042), StallAction::Kick);
        assert_eq!(clock.pending(10_043), StallAction::None);
        assert_eq!(clock.pending(50_042), StallAction::Reset);
    }
    #[test]
    fn sparse_health_wakes_still_detect_stalls_and_progress_starts_a_new_interval() {
        let mut clock = StallDeadline::new();
        assert_eq!(clock.pending(1_000), StallAction::None);
        assert_eq!(clock.pending(21_000), StallAction::Kick);
        assert_eq!(clock.pending(51_000), StallAction::Reset);
        clock.progress();
        assert_eq!(clock.pending(60_000), StallAction::None);
        assert_eq!(clock.pending(69_999), StallAction::None);
    }
}
