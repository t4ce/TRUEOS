//! Lead time for the resident, single-target WD preparation worker.
//!
//! Observe trigger-to-Ready time, excluding target allocation and setup. The
//! recent peak covers normal capture variation; two milliseconds leave room
//! for clock rounding and task dispatch. A slow screenshot can temporarily
//! move capture earlier without permanently increasing every frame's age.

const RECENT_CAPTURES: usize = 8;
const DISPATCH_MARGIN_NS: u64 = 2_000_000;
const NANOS_PER_SECOND: u128 = 1_000_000_000;

pub(super) struct CaptureTiming {
    recent_ns: [u64; RECENT_CAPTURES],
    next: usize,
    count: usize,
}

impl CaptureTiming {
    pub(super) const fn new() -> Self {
        Self {
            recent_ns: [0; RECENT_CAPTURES],
            next: 0,
            count: 0,
        }
    }

    pub(super) fn observe(&mut self, duration_ns: u64) {
        self.recent_ns[self.next] = duration_ns;
        self.next = (self.next + 1) % RECENT_CAPTURES;
        self.count = (self.count + 1).min(RECENT_CAPTURES);
    }

    /// Subtract this many ticks from the next encode deadline. A cold worker
    /// starts as soon as its target becomes free. A budget larger than a frame
    /// also falls back toward immediate preparation, preserving serialization.
    pub(super) fn lead_ticks(&self, timer_hz: u64, target_hz: usize) -> u64 {
        if timer_hz == 0 || target_hz == 0 {
            return 1;
        }
        // Fractional cadence uses floor/ceil periods. Bound lookahead by its
        // shortest period, and support timer rates below the requested rate.
        let frame_ticks = ((timer_hz as u128 / target_hz as u128).max(1)) as u64;
        if self.count == 0 {
            return frame_ticks;
        }
        let peak_ns = self.recent_ns[..self.count]
            .iter()
            .copied()
            .max()
            .unwrap_or(0);
        let budget_ns = peak_ns.saturating_add(DISPATCH_MARGIN_NS);
        // Two u64 factors fit in u128; clamp before narrowing back to ticks.
        let ticks = (budget_ns as u128 * timer_hz as u128).div_ceil(NANOS_PER_SECOND);
        ticks.max(1).min(frame_ticks as u128) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_worker_keeps_the_full_frame_available_for_unknown_capture_cost() {
        let timing = CaptureTiming::new();
        assert_eq!(timing.lead_ticks(1000, 33), 30);
        assert_eq!(timing.lead_ticks(1000, 60), 16);
    }

    #[test]
    fn measured_four_millisecond_capture_reserves_six_milliseconds() {
        let mut timing = CaptureTiming::new();
        timing.observe(4_000_000);
        assert_eq!(timing.lead_ticks(1000, 33), 6);
    }

    #[test]
    fn fractional_tick_conversion_rounds_up_instead_of_starting_capture_late() {
        let mut timing = CaptureTiming::new();
        timing.observe(4_000_001);
        assert_eq!(timing.lead_ticks(1000, 33), 7);
        timing.observe(4_000_000);
        assert_eq!(timing.lead_ticks(1024, 33), 7);
    }

    #[test]
    fn recent_peak_covers_variation_then_expires_after_eight_new_captures() {
        let mut timing = CaptureTiming::new();
        timing.observe(5_000_000);
        for _ in 0..7 {
            timing.observe(4_000_000);
            assert_eq!(timing.lead_ticks(1000, 33), 7);
        }
        timing.observe(4_000_000);
        assert_eq!(timing.lead_ticks(1000, 33), 6);
    }

    #[test]
    fn slow_snapshot_uses_available_budget_without_permanent_early_capture() {
        let mut timing = CaptureTiming::new();
        timing.observe(80_000_000);
        assert_eq!(timing.lead_ticks(1000, 33), 30);
        for _ in 0..8 {
            timing.observe(4_000_000);
        }
        assert_eq!(timing.lead_ticks(1000, 33), 6);
    }

    #[test]
    fn extreme_clock_and_capture_values_do_not_overflow() {
        let mut timing = CaptureTiming::new();
        timing.observe(u64::MAX);
        assert_eq!(timing.lead_ticks(u64::MAX, 1), u64::MAX);
        assert_eq!(timing.lead_ticks(u64::MAX, usize::MAX), 1);
        assert_eq!(timing.lead_ticks(1000, 33), 30);
    }

    #[test]
    fn zero_duration_is_a_valid_quantized_sample_with_dispatch_margin() {
        let mut timing = CaptureTiming::new();
        timing.observe(0);
        assert_eq!(timing.lead_ticks(1000, 33), 2);
        assert_eq!(timing.lead_ticks(1000, 10_000), 1);
    }

    #[test]
    fn invalid_rates_remain_bounded_and_never_request_zero_ticks() {
        let timing = CaptureTiming::new();
        assert_eq!(timing.lead_ticks(0, 33), 1);
        assert_eq!(timing.lead_ticks(1000, 0), 1);
        assert_eq!(timing.lead_ticks(0, 0), 1);
    }

    #[test]
    fn new_lifetime_discards_the_previous_capture_budget() {
        let mut timing = CaptureTiming::new();
        timing.observe(4_000_000);
        assert_eq!(timing.lead_ticks(1000, 33), 6);
        timing = CaptureTiming::new();
        assert_eq!(timing.lead_ticks(1000, 33), 30);
    }
}
