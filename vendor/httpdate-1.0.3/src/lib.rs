//! Date and time utils for HTTP.
//!
//! Multiple HTTP header fields store timestamps.
//! For example a response created on May 15, 2015 may contain the header
//! `Date: Fri, 15 May 2015 15:34:21 GMT`. Since the timestamp does not
//! contain any timezone or leap second information it is equvivalent to
//! writing 1431696861 Unix time. Rust’s `SystemTime` is used to store
//! these timestamps.
//!
//! This crate provides two public functions:
//!
//! * `parse_http_date` to parse a HTTP datetime string to a system time
//! * `fmt_http_date` to format a system time to a IMF-fixdate
//!
//! In addition it exposes the `HttpDate` type that can be used to parse
//! and format timestamps. Convert a sytem time to `HttpDate` and vice versa.
//! The `HttpDate` (8 bytes) is smaller than `SystemTime` (16 bytes) and
//! using the display impl avoids a temporary allocation.
#![forbid(unsafe_code)]
#![cfg_attr(any(target_os = "trueos", target_os = "zkvm"), no_std)]

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate alloc;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
extern crate self as std;

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use alloc::{format, string::String};
use core::fmt::{Display, Formatter};
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::{format, string::String};
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::io;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use crate::time::SystemTime;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use std::time::SystemTime;

pub use date::HttpDate;

mod date;

/// An opaque error type for all parsing errors.
#[derive(Debug)]
pub struct Error(());

impl core::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result<(), core::fmt::Error> {
        f.write_str("string contains no or an invalid date")
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> io::Error {
        io::Error::new(io::ErrorKind::Other, e)
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod cmp {
    pub use core::cmp::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod error {
    pub use core::error::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod fmt {
    pub use core::fmt::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod io {
    use core::fmt;

    #[derive(Debug)]
    pub struct Error {
        kind: ErrorKind,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub enum ErrorKind {
        Other,
    }

    impl Error {
        pub fn new<E>(kind: ErrorKind, _error: E) -> Self {
            Self { kind }
        }

        pub fn kind(&self) -> ErrorKind {
            self.kind
        }
    }

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("httpdate error")
        }
    }

    impl core::error::Error for Error {}
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod str {
    pub use core::str::*;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub mod time {
    pub use core::time::Duration;

    use core::ops::{Add, AddAssign, Sub};

    #[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct SystemTime {
        duration_since_epoch: Duration,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub struct SystemTimeError {
        duration: Duration,
    }

    pub const UNIX_EPOCH: SystemTime = SystemTime {
        duration_since_epoch: Duration::ZERO,
    };

    impl SystemTime {
        pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
            self.duration_since_epoch
                .checked_sub(earlier.duration_since_epoch)
                .ok_or_else(|| SystemTimeError {
                    duration: earlier.duration_since_epoch - self.duration_since_epoch,
                })
        }
    }

    impl SystemTimeError {
        pub fn duration(&self) -> Duration {
            self.duration
        }
    }

    impl Add<Duration> for SystemTime {
        type Output = SystemTime;

        fn add(self, duration: Duration) -> SystemTime {
            SystemTime {
                duration_since_epoch: self.duration_since_epoch + duration,
            }
        }
    }

    impl AddAssign<Duration> for SystemTime {
        fn add_assign(&mut self, duration: Duration) {
            *self = *self + duration;
        }
    }

    impl Sub<Duration> for SystemTime {
        type Output = SystemTime;

        fn sub(self, duration: Duration) -> SystemTime {
            SystemTime {
                duration_since_epoch: self.duration_since_epoch - duration,
            }
        }
    }
}

/// Parse a date from an HTTP header field.
///
/// Supports the preferred IMF-fixdate and the legacy RFC 805 and
/// ascdate formats. Two digit years are mapped to dates between
/// 1970 and 2069.
pub fn parse_http_date(s: &str) -> Result<SystemTime, Error> {
    s.parse::<HttpDate>().map(|d| d.into())
}

/// Format a date to be used in a HTTP header field.
///
/// Dates are formatted as IMF-fixdate: `Fri, 15 May 2015 15:34:21 GMT`.
pub fn fmt_http_date(d: SystemTime) -> String {
    format!("{}", HttpDate::from(d))
}
