// Copyright 2018 Developers of the Rand project.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Error types

use core::fmt;
use core::num::NonZeroU32;

#[cfg(feature = "std")] use std::boxed::Box;

/// Error type of random number generators
///
/// In order to be compatible with `std` and `no_std`, this type has two
/// possible implementations: with `std` a boxed `Error` trait object is stored,
/// while with `no_std` we merely store an error code.
pub struct Error {
    #[cfg(feature = "std")]
    inner: Box<dyn std::error::Error + Send + Sync + 'static>,
    #[cfg(not(feature = "std"))]
    code: NonZeroU32,
}

impl Error {
    /// Codes at or above this point can be used by users to define their own
    /// custom errors.
    ///
    /// This has a fixed value of `(1 << 31) + (1 << 30) = 0xC000_0000`,
    /// therefore the number of values available for custom codes is `1 << 30`.
    ///
    /// This range is retained from `getrandom` 0.2 for compatibility. Newer
    /// versions of `getrandom` use a different, opaque error representation.
    pub const CUSTOM_START: u32 = (1 << 31) + (1 << 30);
    /// Codes below this point represent OS Errors (i.e. positive i32 values).
    /// Codes at or above this point, but below [`Error::CUSTOM_START`] are
    /// reserved for use by the `rand` and `getrandom` crates.
    ///
    /// This range is retained from `getrandom` 0.2 for compatibility.
    pub const INTERNAL_START: u32 = 1 << 31;

    /// Construct from any type supporting `std::error::Error`
    ///
    /// Available only when configured with `std`.
    ///
    /// See also `From<NonZeroU32>`, which is available with and without `std`.
    #[cfg(feature = "std")]
    #[cfg_attr(doc_cfg, doc(cfg(feature = "std")))]
    #[inline]
    pub fn new<E>(err: E) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    {
        Error { inner: err.into() }
    }

    /// Reference the inner error (`std` only)
    ///
    /// When configured with `std`, this is a trivial operation and never
    /// panics. Without `std`, this method is simply unavailable.
    #[cfg(feature = "std")]
    #[cfg_attr(doc_cfg, doc(cfg(feature = "std")))]
    #[inline]
    pub fn inner(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        &*self.inner
    }

    /// Unwrap the inner error (`std` only)
    ///
    /// When configured with `std`, this is a trivial operation and never
    /// panics. Without `std`, this method is simply unavailable.
    #[cfg(feature = "std")]
    #[cfg_attr(doc_cfg, doc(cfg(feature = "std")))]
    #[inline]
    pub fn take_inner(self) -> Box<dyn std::error::Error + Send + Sync + 'static> {
        self.inner
    }

    /// Extract the raw OS error code (if this error came from the OS)
    ///
    /// This method is identical to `std::io::Error::raw_os_error()`, except
    /// that it works in `no_std` contexts. If this method returns `None`, the
    /// error value can still be formatted via the `Display` implementation.
    #[inline]
    pub fn raw_os_error(&self) -> Option<i32> {
        #[cfg(feature = "std")]
        {
            if let Some(e) = self.inner.downcast_ref::<std::io::Error>() {
                return e.raw_os_error();
            }
            #[cfg(feature = "getrandom")]
            if let Some(e) = self.inner.downcast_ref::<getrandom::Error>() {
                return e.raw_os_error().and_then(|code| {
                    core::convert::TryInto::<i32>::try_into(code).ok()
                });
            }
        }
        match self.code() {
            Some(code) if u32::from(code) < Self::INTERNAL_START => Some(u32::from(code) as i32),
            _ => None,
        }
    }

    /// Retrieve the error code, if any.
    ///
    /// If this `Error` was constructed via `From<NonZeroU32>`, then this method
    /// will return this `NonZeroU32` code (for `no_std` this is always the
    /// case). Otherwise, this method will return `None`.
    #[inline]
    pub fn code(&self) -> Option<NonZeroU32> {
        #[cfg(feature = "std")]
        {
            self.inner.downcast_ref::<ErrorCode>().map(|c| c.0)
        }
        #[cfg(not(feature = "std"))]
        {
            Some(self.code)
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        #[cfg(feature = "std")]
        {
            write!(f, "Error {{ inner: {:?} }}", self.inner)
        }
        #[cfg(all(feature = "getrandom", not(feature = "std")))]
        {
            let mut dbg = f.debug_struct("Error");
            if let Some(errno) = self.raw_os_error() {
                dbg.field("os_error", &errno);
            } else if let Some(description) = getrandom_error_description(self.code) {
                dbg.field("internal_code", &self.code.get());
                dbg.field("description", &description);
            } else {
                dbg.field("unknown_code", &self.code.get());
            }
            dbg.finish()
        }
        #[cfg(not(feature = "getrandom"))]
        {
            write!(f, "Error {{ code: {} }}", self.code)
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        #[cfg(feature = "std")]
        {
            write!(f, "{}", self.inner)
        }
        #[cfg(all(feature = "getrandom", not(feature = "std")))]
        {
            if let Some(errno) = self.raw_os_error() {
                write!(f, "OS Error: {}", errno)
            } else if let Some(description) = getrandom_error_description(self.code) {
                f.write_str(description)
            } else {
                write!(f, "Unknown Error: {}", self.code.get())
            }
        }
        #[cfg(not(feature = "getrandom"))]
        {
            write!(f, "error code {}", self.code)
        }
    }
}

impl From<NonZeroU32> for Error {
    #[inline]
    fn from(code: NonZeroU32) -> Self {
        #[cfg(feature = "std")]
        {
            Error {
                inner: Box::new(ErrorCode(code)),
            }
        }
        #[cfg(not(feature = "std"))]
        {
            Error { code }
        }
    }
}

#[cfg(feature = "getrandom")]
/// Preserve the original error with `std`; otherwise translate OS errors and
/// common backend errors into the existing rand_core 0.6 code ranges. Since
/// getrandom 0.4 keeps its other codes private, those map to the reserved
/// internal code `Error::INTERNAL_START + 0xffff`.
impl From<getrandom::Error> for Error {
    #[inline]
    fn from(error: getrandom::Error) -> Self {
        #[cfg(feature = "std")]
        {
            Error {
                inner: Box::new(error),
            }
        }
        #[cfg(not(feature = "std"))]
        {
            Error { code: getrandom_error_code(error) }
        }
    }
}

// getrandom 0.4 no longer exposes its underlying code. Keep rand_core 0.6's
// public code ranges, including TrueOS's existing custom error 1, without
// depending on getrandom's private representation. Other opaque errors use a
// reserved internal code; with std the original error is retained instead.
#[cfg(all(feature = "getrandom", not(feature = "std")))]
fn getrandom_error_code(error: getrandom::Error) -> NonZeroU32 {
    use core::convert::TryFrom;

    if let Some(errno) = error.raw_os_error() {
        if let Ok(code) = u32::try_from(errno) {
            if code < Error::INTERNAL_START {
                if let Some(code) = NonZeroU32::new(code) {
                    return code;
                }
            }
        }
    }
    let code = if error == getrandom::Error::UNSUPPORTED {
        Error::INTERNAL_START
    } else if error == getrandom::Error::ERRNO_NOT_POSITIVE {
        Error::INTERNAL_START + 1
    } else if error == getrandom::Error::UNEXPECTED {
        Error::INTERNAL_START + 2
    } else if error == getrandom::Error::new_custom(1) {
        Error::CUSTOM_START + 1
    } else {
        Error::INTERNAL_START + 0xffff
    };
    NonZeroU32::new(code).unwrap()
}

// Preserve formatting of the legacy public codes, which cannot be converted
// back into a getrandom 0.4 error.
#[cfg(all(feature = "getrandom", not(feature = "std")))]
fn getrandom_error_description(code: NonZeroU32) -> Option<&'static str> {
    if code.get() < Error::INTERNAL_START {
        return None;
    }
    match code.get() - Error::INTERNAL_START {
        0 => Some("getrandom: this target is not supported"),
        1 => Some("errno: did not return a positive value"),
        2 => Some("unexpected situation"),
        3 => Some("SecRandomCopyBytes: iOS Security framework failure"),
        4 => Some("RtlGenRandom: Windows system function failure"),
        5 => Some("RDRAND: failed multiple times: CPU issue likely"),
        6 => Some("RDRAND: instruction not supported"),
        7 => Some("Web Crypto API is unavailable"),
        8 => Some("Calling Web API crypto.getRandomValues failed"),
        11 => Some("randSecure: VxWorks RNG module is not initialized"),
        12 => Some("Node.js crypto CommonJS module is unavailable"),
        13 => Some("Calling Node.js API crypto.randomFillSync failed"),
        14 => Some("Node.js ES modules are not directly supported, see https://docs.rs/getrandom#nodejs-es-module-support"),
        _ => None,
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {
    #[inline]
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.inner.source()
    }
}

#[cfg(feature = "std")]
impl From<Error> for std::io::Error {
    #[inline]
    fn from(error: Error) -> Self {
        if let Some(code) = error.raw_os_error() {
            std::io::Error::from_raw_os_error(code)
        } else {
            std::io::Error::new(std::io::ErrorKind::Other, error)
        }
    }
}

#[cfg(feature = "std")]
#[derive(Debug, Copy, Clone)]
struct ErrorCode(NonZeroU32);

#[cfg(feature = "std")]
impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "error code {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ErrorCode {}

#[cfg(test)]
mod test {
    #[cfg(not(feature = "std"))]
    extern crate std;

    use super::Error;
    use core::num::NonZeroU32;

    #[test]
    fn test_error_codes() {
        assert_eq!(Error::CUSTOM_START, 0xc000_0000);
        assert_eq!(Error::INTERNAL_START, 0x8000_0000);
        for value in [1, 22, Error::INTERNAL_START, Error::CUSTOM_START + 42] {
            let code = NonZeroU32::new(value).unwrap();
            let error = Error::from(code);
            assert_eq!(error.code(), Some(code));
            assert_eq!(error.raw_os_error(), if value < Error::INTERNAL_START {
                Some(value as i32)
            } else {
                None
            });
        }
    }

    #[cfg(all(feature = "getrandom", not(feature = "std")))]
    #[test]
    fn test_getrandom_code_translation() {
        for (error, code) in [
            (getrandom::Error::UNSUPPORTED, Error::INTERNAL_START),
            (getrandom::Error::ERRNO_NOT_POSITIVE, Error::INTERNAL_START + 1),
            (getrandom::Error::UNEXPECTED, Error::INTERNAL_START + 2),
            (getrandom::Error::new_custom(1), Error::CUSTOM_START + 1),
            (getrandom::Error::new_custom(42), Error::INTERNAL_START + 0xffff),
        ] {
            let error = Error::from(error);
            assert_eq!(error.code().unwrap().get(), code);
            assert_eq!(error.raw_os_error(), None);
        }
        assert_eq!(core::mem::size_of::<Error>(), core::mem::size_of::<NonZeroU32>());

        assert_eq!(std::format!("{}", Error::from(getrandom::Error::UNSUPPORTED)),
            "getrandom: this target is not supported");
        assert_eq!(std::format!("{}", Error::from(NonZeroU32::new(22).unwrap())),
            "OS Error: 22");
        assert_eq!(std::format!("{}", Error::from(NonZeroU32::new(Error::CUSTOM_START + 42).unwrap())),
            "Unknown Error: 3221225514");
    }

    #[cfg(all(feature = "getrandom", feature = "std"))]
    #[test]
    fn test_getrandom_error_retained() {
        let original = getrandom::Error::new_custom(42);
        let error = Error::from(original);
        assert_eq!(error.inner().downcast_ref::<getrandom::Error>(), Some(&original));
        assert_eq!(error.code(), None);
        assert_eq!(error.raw_os_error(), None);
        assert_eq!(std::error::Error::source(&error).is_some(),
            std::error::Error::source(&original).is_some());
        assert_eq!(error.take_inner().downcast_ref::<getrandom::Error>(), Some(&original));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_io_error_roundtrip_and_source() {
        let error = Error::new(std::io::Error::from_raw_os_error(22));
        assert_eq!(error.raw_os_error(), Some(22));
        assert_eq!(std::io::Error::from(error).raw_os_error(), Some(22));

        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "outer")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let error = Error::new(Outer(std::io::Error::from_raw_os_error(22)));
        assert!(std::error::Error::source(&error).unwrap().is::<std::io::Error>());
    }
}
