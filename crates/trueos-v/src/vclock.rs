extern crate alloc;

use alloc::{string::String, vec};
use core::fmt;
use core::num::FpCategory;

use crate::vcabi;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct Duration {
    nanos: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Instant {
    nanos: u64,
}

impl Duration {
    #[inline]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self { nanos }
    }

    #[inline]
    pub const fn from_millis(millis: u64) -> Self {
        Self { nanos: millis.saturating_mul(1_000_000) }
    }

    #[inline]
    pub const fn from_secs(seconds: u64) -> Self {
        Self { nanos: seconds.saturating_mul(1_000_000_000) }
    }

    #[inline]
    pub fn try_from_secs_f32(seconds: f32) -> Result<Self, TryFromSecsError> {
        match seconds.classify() {
            FpCategory::Nan | FpCategory::Infinite if seconds.is_sign_negative() => {
                return Err(TryFromSecsError);
            }
            FpCategory::Nan | FpCategory::Infinite => return Err(TryFromSecsError),
            _ => {}
        }
        if seconds < 0.0 {
            return Err(TryFromSecsError);
        }
        let nanos = (seconds as f64) * 1_000_000_000.0;
        if nanos > u64::MAX as f64 {
            return Err(TryFromSecsError);
        }
        Ok(Self { nanos: nanos as u64 })
    }

    #[inline]
    pub const fn as_nanos(self) -> u64 {
        self.nanos
    }

    #[inline]
    pub const fn as_millis(self) -> u64 {
        self.nanos / 1_000_000
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TryFromSecsError;

impl fmt::Display for TryFromSecsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid duration")
    }
}

impl Instant {
    #[inline]
    pub fn now() -> Self {
        Self { nanos: monotonic_nanos() }
    }

    #[inline]
    pub fn elapsed(self) -> Duration {
        Duration::from_nanos(monotonic_nanos().saturating_sub(self.nanos))
    }
}

#[inline]
pub fn monotonic_nanos() -> u64 {
    unsafe { vcabi::trueos_time_monotonic_nanos() }
}

#[inline]
pub fn monotonic_millis() -> u64 {
    monotonic_nanos() / 1_000_000
}

#[inline]
pub fn unix_seconds() -> Option<u64> {
    match unsafe { vcabi::trueos_time_unix_seconds() } {
        0 => None,
        seconds => Some(seconds),
    }
}

#[inline]
pub fn unix_nanos() -> Option<u64> {
    match unsafe { vcabi::trueos_time_unix_nanos() } {
        0 => None,
        nanos => Some(nanos),
    }
}

#[inline]
pub fn ntp_current_unix_seconds() -> u64 {
    unsafe { vcabi::trueos_cabi_ntp_current_unix_seconds() }
}

#[inline]
pub fn kernel_date_day_month_year() -> Option<String> {
    let len =
        unsafe { vcabi::trueos_cabi_ntp_kernel_date_day_month_year(core::ptr::null_mut(), 0) };
    if len == 0 {
        return None;
    }
    let mut bytes = vec![0u8; len];
    let got = unsafe {
        vcabi::trueos_cabi_ntp_kernel_date_day_month_year(bytes.as_mut_ptr(), bytes.len())
    };
    if got == 0 {
        return None;
    }
    bytes.truncate(got);
    String::from_utf8(bytes).ok()
}
