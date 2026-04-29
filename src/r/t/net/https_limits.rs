/// Operational guardrails for HTTPS client behavior under real-world traffic.
///
/// Keep this focused on "limit and stability" policy knobs so they are easy
/// to reason about and tune without touching request parsing logic.
pub struct HttpsLimits;

impl HttpsLimits {
    pub const KEEPALIVE_ENABLE: bool = true;
    pub const KEEPALIVE_IDLE_CLOSE_MS: u64 = 10_000;

    pub const CONNECT_FAIL_BACKOFF_BASE_MS: u64 = 250;
    pub const CONNECT_FAIL_BACKOFF_MAX_MS: u64 = 4_000;
    pub const CONNECT_FAIL_BACKOFF_START_STREAK: u8 = 2;
    pub const CONNECT_FAIL_HARD_STOP_STREAK: u8 = 8;
    pub const CONNECT_FAIL_HARD_STOP_MS: u64 = 30_000;

    /// Exponential connect-failure backoff by consecutive failure streak.
    /// Returns `None` until the streak reaches `CONNECT_FAIL_BACKOFF_START_STREAK`.
    pub fn connect_backoff_ms(streak: u8) -> Option<u64> {
        if streak < Self::CONNECT_FAIL_BACKOFF_START_STREAK {
            return None;
        }

        let shifts = (streak - Self::CONNECT_FAIL_BACKOFF_START_STREAK).min(4) as u32;
        let delay = Self::CONNECT_FAIL_BACKOFF_BASE_MS.saturating_mul(1u64 << shifts);
        Some(delay.min(Self::CONNECT_FAIL_BACKOFF_MAX_MS))
    }

    pub fn connect_hard_stop_ms(streak: u8) -> Option<u64> {
        if streak < Self::CONNECT_FAIL_HARD_STOP_STREAK {
            return None;
        }
        Some(Self::CONNECT_FAIL_HARD_STOP_MS)
    }
}
