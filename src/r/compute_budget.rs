//! Pure accounting for bounded cooperative compute workers.
//!
//! The worker supplies measured TSC work time. This module converts it into
//! burst/cooldown decisions without depending on a timer, executor, or VM
//! state, so the policy has host-executable coverage.

#[inline]
pub const fn queue_cost(queued: usize, in_flight: usize) -> usize {
    queued.saturating_add(in_flight)
}

#[inline]
pub fn burst_cycles(tsc_hz: u64, burst_ms: u64) -> u64 {
    ((tsc_hz as u128).saturating_mul(burst_ms as u128) / 1_000)
        .max(1)
        .min(u64::MAX as u128) as u64
}

#[inline]
pub fn cooldown_cycles(work_cycles: u64, duty_percent: u64) -> u64 {
    let duty = duty_percent.clamp(1, 99);
    (work_cycles as u128)
        .saturating_mul(100u64.saturating_sub(duty) as u128)
        .saturating_add(duty.saturating_sub(1) as u128)
        .checked_div(duty as u128)
        .unwrap_or(0)
        .min(u64::MAX as u128) as u64
}

/// Carries measured cooldown over/undersleep into later bursts. This avoids
/// turning a nominal 66% duty policy into 57% on a 1 kHz timer (4 ms work +
/// 3 ms sleep at every burst), while also charging an early timer wake.
#[derive(Default)]
pub struct CooldownDebt {
    balance_cycles: i128,
}

impl CooldownDebt {
    pub fn request_timer_ticks(
        &mut self,
        work_cycles: u64,
        duty_percent: u64,
        tsc_hz: u64,
        tick_hz: u64,
    ) -> u64 {
        self.balance_cycles = self
            .balance_cycles
            .saturating_add(cooldown_cycles(work_cycles, duty_percent) as i128);
        if self.balance_cycles <= 0 {
            return 0;
        }
        let owed = self.balance_cycles.min(u64::MAX as i128) as u64;
        let tick_hz = tick_hz.max(1);
        let tsc_hz = tsc_hz.max(1);
        let ticks = (owed as u128)
            .saturating_mul(tick_hz as u128)
            .saturating_add(tsc_hz.saturating_sub(1) as u128)
            / tsc_hz as u128;
        ticks.max(1).min(u64::MAX as u128) as u64
    }

    /// Call with the real elapsed TSC cycles after the timer wakes. A late
    /// executor poll becomes credit; an early wake remains debt.
    pub fn settle_paid_cycles(&mut self, elapsed_cycles: u64) {
        self.balance_cycles = self.balance_cycles.saturating_sub(elapsed_cycles as i128);
    }

    /// Do not bank an unbounded executor stall as future uninterrupted work.
    /// The caller still yields at every burst boundary even while credit exists.
    pub fn cap_credit_cycles(&mut self, maximum_credit_cycles: u64) {
        self.balance_cycles = self.balance_cycles.max(-(maximum_credit_cycles as i128));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_steps_accumulate_until_the_four_millisecond_burst() {
        let threshold = burst_cycles(1_000_000_000, 4);
        assert_eq!(threshold, 4_000_000);
        assert!(999 * 4_000 < threshold);
        assert!(1_000 * 4_000 >= threshold);
    }

    #[test]
    fn sustained_timer_rounding_converges_to_requested_duty() {
        let mut debt = CooldownDebt::default();
        let work = 4_000_000u64;
        let mut total_sleep = 0u64;
        for _ in 0..1_000 {
            let ticks = debt.request_timer_ticks(work, 66, 1_000_000_000, 1_000);
            let elapsed = ticks * 1_000_000;
            total_sleep += elapsed;
            debt.settle_paid_cycles(elapsed);
        }
        // Expected 34/66 * 4_000_000 * 1000 = 2.0606e9 cycles. A one-tick
        // bound covers the final carried credit while proving we do not charge
        // every 4 ms burst as a fixed 3 ms sleep.
        let expected = cooldown_cycles(work, 66) * 1_000;
        assert!(total_sleep >= expected);
        assert!(total_sleep - expected <= 1_000_000);
    }

    #[test]
    fn queue_cost_includes_currently_running_band() {
        assert_eq!(queue_cost(0, 0), 0);
        assert_eq!(queue_cost(0, 1), 1);
        assert_eq!(queue_cost(3, 2), 5);
    }

    #[test]
    fn early_timer_wake_keeps_remaining_debt() {
        let mut debt = CooldownDebt::default();
        assert_eq!(debt.request_timer_ticks(4_000_000, 66, 1_000_000_000, 1_000), 3);
        debt.settle_paid_cycles(1_000_000);
        // One millisecond did not cover the requested 2.06 ms cooldown.
        assert_eq!(debt.request_timer_ticks(0, 66, 1_000_000_000, 1_000), 2);
    }

    #[test]
    fn late_wake_credit_is_limited_to_one_future_burst() {
        let mut debt = CooldownDebt::default();
        let burst = 4_000_000;
        assert_eq!(debt.request_timer_ticks(burst, 66, 1_000_000_000, 1_000), 3);
        debt.settle_paid_cycles(100_000_000);
        debt.cap_credit_cycles(cooldown_cycles(burst, 66));
        // One burst can use the late-wake credit, but a second requires a
        // normal cooldown again; no giant scheduler stall becomes a long run.
        assert_eq!(debt.request_timer_ticks(burst, 66, 1_000_000_000, 1_000), 0);
        assert_eq!(debt.request_timer_ticks(burst, 66, 1_000_000_000, 1_000), 3);
    }
}
