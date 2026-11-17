//! Media-clock policy shared by window and retained-texture playback.
//! Time starts when the first converted picture is ready, and pauses rebase
//! future deadlines instead of making the queued pictures late on resume.

#[derive(Copy, Clone, Debug, Default)]
pub(super) struct PresentationClock {
    origin: Option<u64>,
    paused_at: Option<u64>,
}

impl PresentationClock {
    pub(super) const fn new() -> Self {
        Self {
            origin: None,
            paused_at: None,
        }
    }

    pub(super) fn set_paused(&mut self, paused: bool, now: u64) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(now),
            (false, Some(started)) => {
                if let Some(origin) = &mut self.origin {
                    *origin = origin.saturating_add(now.saturating_sub(started));
                }
                self.paused_at = None;
            }
            _ => {}
        }
    }

    /// A pending prior publication gates the next one until the compositor
    /// confirms its exact frame/buffer at SURFLIVE. Textures use their own
    /// Ready/Reading ring and therefore supply `false` for that gate.
    pub(super) fn deadline(
        &mut self,
        now: u64,
        media_ticks: u64,
        prior_pending: bool,
    ) -> Option<u64> {
        if self.paused_at.is_some() || prior_pending {
            return None;
        }
        let origin = *self.origin.get_or_insert(now);
        Some(origin.saturating_add(media_ticks))
    }
}

/// Convert relative container PTS without wrapping on malformed timestamps.
pub(crate) fn pts_ticks(pts: i64, base_pts: i64, timescale: u32, tick_hz: u64) -> u64 {
    let relative = pts.saturating_sub(base_pts).max(0) as u64;
    let ticks =
        u128::from(relative).saturating_mul(u128::from(tick_hz)) / u128::from(timescale.max(1));
    ticks.min(u128::from(u64::MAX)) as u64
}

pub(super) fn publication_deadline(media_deadline: u64, lead_ticks: u64) -> u64 {
    media_deadline.saturating_sub(lead_ticks)
}

pub(super) fn latch_wait_expired(now: u64, published: u64, limit: u64) -> bool {
    now.saturating_sub(published) >= limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_does_not_advance_media_clock() {
        let mut clock = PresentationClock::new();
        assert_eq!(clock.deadline(9_000, 0, false), Some(9_000));
        assert_eq!(clock.deadline(9_005, 1_000, false), Some(10_000));
    }

    #[test]
    fn pause_rebases_all_future_deadlines_once() {
        let mut clock = PresentationClock::new();
        assert_eq!(clock.deadline(100, 0, false), Some(100));
        clock.set_paused(true, 120);
        clock.set_paused(true, 160);
        assert_eq!(clock.deadline(190, 30, false), None);
        clock.set_paused(false, 220);
        clock.set_paused(false, 250);
        assert_eq!(clock.deadline(220, 30, false), Some(230));
    }

    #[test]
    fn paused_start_and_prior_latch_do_not_anchor_clock() {
        let mut clock = PresentationClock::new();
        clock.set_paused(true, 10);
        assert_eq!(clock.deadline(20, 0, false), None);
        clock.set_paused(false, 30);
        assert_eq!(clock.deadline(40, 0, true), None);
        assert_eq!(clock.deadline(50, 0, false), Some(50));
    }

    #[test]
    fn container_time_conversion_is_bounded_and_preserves_b_frame_pts_order() {
        assert_eq!(pts_ticks(9_000, 6_000, 90_000, 1_000_000), 33_333);
        assert_eq!(pts_ticks(-1, 0, 1, 1_000), 0);
        assert_eq!(pts_ticks(i64::MAX, i64::MIN, 0, u64::MAX), u64::MAX);
        let mut order = [(9_000, 6_000), (6_000, 9_000), (12_000, 12_000)];
        order.sort_by_key(|&(pts, dts)| (pts, dts));
        assert_eq!(
            order.map(|(pts, _)| pts_ticks(pts, 6_000, 90_000, 1_000_000)),
            [0, 33_333, 66_666]
        );
    }

    #[test]
    fn publication_lead_and_missing_display_ack_are_bounded() {
        assert_eq!(publication_deadline(1_000, 20), 980);
        assert_eq!(publication_deadline(10, 20), 0);
        assert!(!latch_wait_expired(1_099, 1_000, 100));
        assert!(latch_wait_expired(1_100, 1_000, 100));
        assert!(!latch_wait_expired(900, 1_000, 100));
    }
}
