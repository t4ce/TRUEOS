//! Bounded failed-state envelope in the comparison grid's normalized controls.

pub(super) const MIN_DURATION_MS: u64 = 3_000;
pub(super) const MAX_DURATION_MS: u64 = 5_000;

#[derive(Copy, Clone)]
pub(super) struct FailedVfxEnvelope {
    started_ms: Option<u64>,
    duration_ms: u64,
}

impl FailedVfxEnvelope {
    pub(super) const INACTIVE: Self = Self {
        started_ms: None,
        duration_ms: MIN_DURATION_MS,
    };

    /// Only an incoming lifecycle edge arms the burst; sampling never restarts it.
    /// Keep the selected duration fixed for the entire burst, including all fades.
    pub(super) fn start(&mut self, now_ms: u64, duration_ms: u64) {
        self.started_ms = Some(now_ms);
        self.duration_ms = duration_ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS);
    }

    pub(super) fn sample(&mut self, now_ms: u64) -> Option<[f32; 4]> {
        let elapsed_ms = now_ms.saturating_sub(self.started_ms?);
        let duration_ms = self.duration_ms;
        if elapsed_ms >= duration_ms {
            self.started_ms = None;
            return None;
        }
        let progress = elapsed_ms as f32 / duration_ms as f32;
        // Five equal holds keep the final -1 visible too. Pulse speed is a
        // jump control, deliberately excluded from the smooth envelopes.
        let phase = (elapsed_ms.saturating_mul(5) / duration_ms) as usize;
        let pulse_speed = [-1.0, 0.0, 1.0, 0.0, -1.0][phase];
        let rim_progress = (progress * 4.0).min(1.0);
        let rim = 0.20 * (0.5 - 0.5 * libm::cosf(core::f32::consts::PI * rim_progress));
        let shake = -libm::cosf(2.0 * core::f32::consts::PI * progress);
        Some([-0.50, pulse_speed, rim, shake])
    }
}

/// Match the offline grid's `mapped_parameter`: zero is the authored value,
/// either side scales to its own limit, then the native slider step is rounded.
pub(super) fn mapped_parameter(
    normalized: f32,
    minimum: f32,
    authored: f32,
    maximum: f32,
    step: f32,
) -> f32 {
    let position = ((normalized + 1.0) * 0.5).clamp(0.0, 1.0);
    let value = if position < 0.5 {
        minimum + position * 2.0 * (authored - minimum)
    } else {
        authored + (position - 0.5) * 2.0 * (maximum - authored)
    };
    let value = if step > 0.0 {
        libm::roundf(value / step) * step
    } else {
        value
    };
    value.clamp(minimum, maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DURATIONS_MS: [u64; 4] = [MIN_DURATION_MS, 3_001, 4_000, MAX_DURATION_MS];

    fn close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.000_01,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn inactive_including_boot_zero_and_arms_at_zero() {
        let mut state = FailedVfxEnvelope::INACTIVE;
        assert_eq!(state.sample(0), None);
        assert_eq!(state.sample(10_000), None);
        state.start(0, MIN_DURATION_MS);
        assert_eq!(state.sample(0), Some([-0.5, -1.0, 0.0, -1.0]));
    }

    #[test]
    fn pulse_speed_jumps_at_five_equal_holds() {
        for duration in DURATIONS_MS {
            let mut state = FailedVfxEnvelope::INACTIVE;
            state.start(10_000, duration);
            let steps = [-1.0, 0.0, 1.0, 0.0, -1.0];
            assert_eq!(state.sample(10_000).unwrap()[1], steps[0]);
            for phase in 1..steps.len() {
                let boundary = (phase as u64 * duration).div_ceil(5);
                assert_eq!(
                    state.sample(10_000 + boundary - 1).unwrap()[1],
                    steps[phase - 1]
                );
                assert_eq!(state.sample(10_000 + boundary).unwrap()[1], steps[phase]);
            }
            assert_eq!(state.sample(10_000 + duration - 1).unwrap()[1], steps[4]);
            assert_eq!(state.sample(10_000 + duration), None);
        }
    }

    #[test]
    fn rim_ramps_in_first_quarter_then_holds() {
        for duration in [MIN_DURATION_MS, 4_000, MAX_DURATION_MS] {
            let mut state = FailedVfxEnvelope::INACTIVE;
            state.start(0, duration);
            close(state.sample(0).unwrap()[2], 0.0);
            close(state.sample(duration / 8).unwrap()[2], 0.1);
            for elapsed in [duration / 4, duration / 2, duration - 1] {
                close(state.sample(elapsed).unwrap()[2], 0.2);
            }
        }
    }

    #[test]
    fn flash_is_fixed_and_shake_completes_one_smooth_cycle() {
        for duration in [MIN_DURATION_MS, 4_000, MAX_DURATION_MS] {
            let mut state = FailedVfxEnvelope::INACTIVE;
            state.start(0, duration);
            for (quarter, expected_shake) in [(0, -1.0), (1, 0.0), (2, 1.0), (3, 0.0)] {
                let parameters = state.sample(quarter * duration / 4).unwrap();
                assert_eq!(parameters[0], -0.5);
                close(parameters[3], expected_shake);
            }
            assert!(state.sample(duration - 1).unwrap()[3] < -0.999);
        }
    }

    #[test]
    fn repeated_samples_expire_without_rearming() {
        for duration in DURATIONS_MS {
            let mut state = FailedVfxEnvelope::INACTIVE;
            state.start(700, duration);
            for elapsed in 0..duration {
                assert!(state.sample(700 + elapsed).is_some());
            }
            assert_eq!(state.sample(700 + duration), None);
            assert_eq!(state.sample(10_000), None);
        }
    }

    #[test]
    fn new_failure_replaces_the_start_and_duration() {
        let mut state = FailedVfxEnvelope::INACTIVE;
        state.start(0, MIN_DURATION_MS);
        assert!(state.sample(MIN_DURATION_MS - 1).is_some());
        state.start(MIN_DURATION_MS, MAX_DURATION_MS);
        assert_eq!(state.sample(MIN_DURATION_MS), Some([-0.5, -1.0, 0.0, -1.0]));
        assert!(state.sample(MIN_DURATION_MS * 2).is_some());
        assert_eq!(state.sample(MIN_DURATION_MS + MAX_DURATION_MS), None);
    }

    #[test]
    fn duration_is_bounded_to_three_through_five_seconds() {
        for requested in [0, 2_999, 3_000, 4_000, 5_000, 5_001, u64::MAX] {
            let mut state = FailedVfxEnvelope::INACTIVE;
            state.start(0, requested);
            let expected = requested.clamp(MIN_DURATION_MS, MAX_DURATION_MS);
            assert!(state.sample(expected - 1).is_some());
            assert_eq!(state.sample(expected), None);
        }
    }

    #[test]
    fn native_mapping_uses_authored_center_and_slider_steps() {
        // The Hit flash sliders from Spirit_VFX and offline main.c.
        let controls = [
            [0.0, 0.82, 1.0, 0.01],
            [0.0, 2.2, 8.0, 0.01],
            [0.0, 4.5, 14.0, 0.1],
            [0.0, 1.5, 8.0, 0.1],
        ];
        for [min, default, max, step] in controls {
            close(mapped_parameter(-1.0, min, default, max, step), min);
            close(mapped_parameter(0.0, min, default, max, step), default);
            close(mapped_parameter(1.0, min, default, max, step), max);
        }
        close(mapped_parameter(-0.5, 0.0, 0.82, 1.0, 0.01), 0.41);
        close(mapped_parameter(0.2, 0.0, 4.5, 14.0, 0.1), 6.4);
        close(mapped_parameter(0.0, 0.0, 1.25, 8.0, 0.5), 1.5);
        close(mapped_parameter(-2.0, 0.0, 4.5, 14.0, 0.1), 0.0);
        close(mapped_parameter(2.0, 0.0, 4.5, 14.0, 0.1), 14.0);
    }
}
