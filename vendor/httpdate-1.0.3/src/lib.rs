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

#[cfg(test)]
mod tests {
    use core::str;
    use std::time::{Duration, UNIX_EPOCH};

    use super::{fmt_http_date, parse_http_date, HttpDate};

    #[test]
    fn test_rfc_example() {
        let d = UNIX_EPOCH + Duration::from_secs(784111777);
        assert_eq!(
            d,
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT").expect("#1")
        );
        assert_eq!(
            d,
            parse_http_date("Sunday, 06-Nov-94 08:49:37 GMT").expect("#2")
        );
        assert_eq!(d, parse_http_date("Sun Nov  6 08:49:37 1994").expect("#3"));
    }

    #[test]
    fn test2() {
        let d = UNIX_EPOCH + Duration::from_secs(1475419451);
        assert_eq!(
            d,
            parse_http_date("Sun, 02 Oct 2016 14:44:11 GMT").expect("#1")
        );
        assert!(parse_http_date("Sun Nov 10 08:00:00 1000").is_err());
        assert!(parse_http_date("Sun Nov 10 08*00:00 2000").is_err());
        assert!(parse_http_date("Sunday, 06-Nov-94 08+49:37 GMT").is_err());
    }

    #[test]
    fn test3() {
        let mut d = UNIX_EPOCH;
        assert_eq!(d, parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT").unwrap());
        d += Duration::from_secs(3600);
        assert_eq!(d, parse_http_date("Thu, 01 Jan 1970 01:00:00 GMT").unwrap());
        d += Duration::from_secs(86400);
        assert_eq!(d, parse_http_date("Fri, 02 Jan 1970 01:00:00 GMT").unwrap());
        d += Duration::from_secs(2592000);
        assert_eq!(d, parse_http_date("Sun, 01 Feb 1970 01:00:00 GMT").unwrap());
        d += Duration::from_secs(2592000);
        assert_eq!(d, parse_http_date("Tue, 03 Mar 1970 01:00:00 GMT").unwrap());
        d += Duration::from_secs(31536005);
        assert_eq!(d, parse_http_date("Wed, 03 Mar 1971 01:00:05 GMT").unwrap());
        d += Duration::from_secs(15552000);
        assert_eq!(d, parse_http_date("Mon, 30 Aug 1971 01:00:05 GMT").unwrap());
        d += Duration::from_secs(6048000);
        assert_eq!(d, parse_http_date("Mon, 08 Nov 1971 01:00:05 GMT").unwrap());
        d += Duration::from_secs(864000000);
        assert_eq!(d, parse_http_date("Fri, 26 Mar 1999 01:00:05 GMT").unwrap());
    }

    #[test]
    fn test_fmt() {
        let d = UNIX_EPOCH;
        assert_eq!(fmt_http_date(d), "Thu, 01 Jan 1970 00:00:00 GMT");
        let d = UNIX_EPOCH + Duration::from_secs(1475419451);
        assert_eq!(fmt_http_date(d), "Sun, 02 Oct 2016 14:44:11 GMT");
    }

    #[allow(dead_code)]
    fn testcase(data: &[u8]) {
        if let Ok(s) = str::from_utf8(data) {
            println!("{:?}", s);
            if let Ok(d) = parse_http_date(s) {
                let o = fmt_http_date(d);
                assert!(!o.is_empty());
            }
        }
    }

    #[test]
    fn size_of() {
        assert_eq!(::core::mem::size_of::<HttpDate>(), 8);
    }

    #[test]
    fn test_date_comparison() {
        let a = UNIX_EPOCH + Duration::from_secs(784111777);
        let b = a + Duration::from_secs(30);
        assert!(a < b);
        let a_date: HttpDate = a.into();
        let b_date: HttpDate = b.into();
        assert!(a_date < b_date);
        assert_eq!(a_date.cmp(&b_date), ::core::cmp::Ordering::Less)
    }

    #[test]
    fn test_parse_bad_date() {
        // 1994-11-07 is actually a Monday
        let parsed = "Sun, 07 Nov 1994 08:48:37 GMT".parse::<HttpDate>();
        assert!(parsed.is_err())
    }
}
