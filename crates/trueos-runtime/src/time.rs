//! Monotonic time from the native platform clock.
use core::{ops, time::Duration};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self {
        Self(unsafe { crate::abi::trueos_platform_monotonic_nanos() })
    }
    pub fn elapsed(&self) -> Duration {
        Self::now().saturating_duration_since(*self)
    }
    pub fn duration_since(&self, earlier: Self) -> Duration {
        self.saturating_duration_since(earlier)
    }
    pub fn saturating_duration_since(&self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }
    pub fn checked_duration_since(&self, earlier: Self) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }
    pub fn checked_add(&self, duration: Duration) -> Option<Self> {
        self.0
            .checked_add(u64::try_from(duration.as_nanos()).ok()?)
            .map(Self)
    }
    pub fn checked_sub(&self, duration: Duration) -> Option<Self> {
        self.0
            .checked_sub(u64::try_from(duration.as_nanos()).ok()?)
            .map(Self)
    }
}
impl ops::Add<Duration> for Instant {
    type Output = Self;
    fn add(self, d: Duration) -> Self {
        self.checked_add(d).expect("native instant overflow")
    }
}
impl ops::Sub<Duration> for Instant {
    type Output = Self;
    fn sub(self, d: Duration) -> Self {
        self.checked_sub(d).expect("native instant underflow")
    }
}
impl ops::Sub for Instant {
    type Output = Duration;
    fn sub(self, d: Self) -> Duration {
        self.duration_since(d)
    }
}
impl ops::AddAssign<Duration> for Instant {
    fn add_assign(&mut self, d: Duration) {
        *self = *self + d;
    }
}
impl ops::SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, d: Duration) {
        *self = *self - d;
    }
}

/// Round finite waits up; reserve u64::MAX for an infinite generation wait.
pub fn timeout_ms(duration: Duration) -> u64 {
    duration
        .as_nanos()
        .div_ceil(1_000_000)
        .min((u64::MAX - 1) as u128) as u64
}
