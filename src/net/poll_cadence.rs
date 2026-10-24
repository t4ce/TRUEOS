//! Keep interactive receive traffic responsive while its NIC is active.

// The current 1 kHz timer rounds this request up to one tick (1 ms).
const ACTIVE_POLL_US: u64 = 250;
const ACTIVE_HOLD_US: u64 = 100_000;

pub(super) struct RxPollCadence {
    idle_sleep_us: u64,
    last_frames: (u64, u64),
    last_active_us: Option<u64>,
}

impl RxPollCadence {
    pub(super) fn new(idle_sleep_us: u64, frames: (u64, u64)) -> Self {
        Self {
            idle_sleep_us,
            last_frames: frames,
            last_active_us: None,
        }
    }

    pub(super) fn sleep_us(&mut self, now_us: u64, busy: bool, frames: (u64, u64)) -> u64 {
        // Compare counters instead of subtracting them so wraparound cannot
        // turn a traffic observation into an underflow or a long busy period.
        if busy || frames != self.last_frames {
            self.last_active_us = Some(now_us);
        }
        self.last_frames = frames;

        if busy {
            return 0;
        }
        if self
            .last_active_us
            .is_some_and(|last| now_us.saturating_sub(last) < ACTIVE_HOLD_US)
        {
            ACTIVE_POLL_US.min(self.idle_sleep_us)
        } else {
            self.idle_sleep_us
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RxPollCadence;

    #[test]
    fn outgoing_video_keeps_input_receive_polls_fast_between_frames() {
        let mut cadence = RxPollCadence::new(5_000, (0, 0));
        assert_eq!(cadence.sleep_us(0, false, (0, 0)), 5_000);

        // A stream can have no received packets between control/HID messages.
        // TX alone must retain fast RX polling across each 30 fps frame gap.
        for frame in 1..=300 {
            let at = frame * 33_333;
            assert_eq!(cadence.sleep_us(at, false, (0, frame)), 250);
            assert_eq!(cadence.sleep_us(at + 33_000, false, (0, frame)), 250);
        }
        let last_frame_at = 300 * 33_333;
        assert_eq!(cadence.sleep_us(last_frame_at + 99_999, false, (0, 300)), 250);
        assert_eq!(cadence.sleep_us(last_frame_at + 100_000, false, (0, 300)), 5_000);
    }

    #[test]
    fn receive_bursts_drain_immediately_then_keep_a_bounded_active_window() {
        let mut cadence = RxPollCadence::new(5_000, (0, 0));
        // The driver can be busy before smoltcp increments its RX counters.
        assert_eq!(cadence.sleep_us(10, true, (0, 0)), 0);
        assert_eq!(cadence.sleep_us(20, false, (0, 0)), 250);
        assert_eq!(cadence.sleep_us(100_000, false, (1, 0)), 250);
        assert_eq!(cadence.sleep_us(199_999, false, (1, 0)), 250);
        assert_eq!(cadence.sleep_us(200_000, false, (1, 0)), 5_000);
    }

    #[test]
    fn activity_is_per_nic_and_preserves_a_shorter_idle_cap() {
        let mut streaming = RxPollCadence::new(5_000, (0, 0));
        let mut idle = RxPollCadence::new(5_000, (0, 0));
        let mut already_fast = RxPollCadence::new(100, (0, 0));
        assert_eq!(streaming.sleep_us(1_000, false, (0, 1)), 250);
        assert_eq!(idle.sleep_us(1_000, false, (0, 0)), 5_000);
        assert_eq!(already_fast.sleep_us(1_000, false, (0, 1)), 100);
    }

    #[test]
    fn counter_wrap_is_observed_as_fresh_activity() {
        let mut cadence = RxPollCadence::new(5_000, (u64::MAX, u64::MAX));
        assert_eq!(cadence.sleep_us(1_000, false, (0, u64::MAX)), 250);
        assert_eq!(cadence.sleep_us(100_999, false, (0, 0)), 250);
        assert_eq!(cadence.sleep_us(200_999, false, (0, 0)), 5_000);
    }
}
