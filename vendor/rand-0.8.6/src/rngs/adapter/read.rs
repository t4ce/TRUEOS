// Copyright 2018 Developers of the Rand project.
// Copyright 2013 The Rust Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! A wrapper around any Read to treat it as an RNG.

#![allow(deprecated)]

use ::core::fmt;
use std::io::Read;

use rand_core::{impls, Error, RngCore};


/// An RNG that reads random bytes straight from any type supporting
/// [`std::io::Read`], for example files.
///
/// This will work best with an infinite reader, but that is not required.
///
/// This can be used with `/dev/urandom` on Unix but it is recommended to use
/// [`OsRng`] instead.
///
/// # Panics
///
/// `ReadRng` uses [`std::io::Read::read_exact`], which retries on interrupts.
/// All other errors from the underlying reader, including when it does not
/// have enough data, will only be reported through [`try_fill_bytes`].
/// The other [`RngCore`] methods will panic in case of an error.
///
/// [`OsRng`]: crate::rngs::OsRng
/// [`try_fill_bytes`]: RngCore::try_fill_bytes
#[derive(Debug)]
#[deprecated(since="0.8.4", note="removal due to lack of usage")]
pub struct ReadRng<R> {
    reader: R,
}

impl<R: Read> ReadRng<R> {
    /// Create a new `ReadRng` from a `Read`.
    pub fn new(r: R) -> ReadRng<R> {
        ReadRng { reader: r }
    }
}

impl<R: Read> RngCore for ReadRng<R> {
    fn next_u32(&mut self) -> u32 {
        impls::next_u32_via_fill(self)
    }

    fn next_u64(&mut self) -> u64 {
        impls::next_u64_via_fill(self)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.try_fill_bytes(dest).unwrap_or_else(|err| {
            panic!(
                "reading random bytes from Read implementation failed; error: {}",
                err
            )
        });
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        if dest.is_empty() {
            return Ok(());
        }
        // Use `std::io::read_exact`, which retries on `ErrorKind::Interrupted`.
        self.reader
            .read_exact(dest)
            .map_err(|e| Error::new(ReadError(e)))
    }
}

/// `ReadRng` error type
#[derive(Debug)]
#[deprecated(since="0.8.4")]
pub struct ReadError(std::io::Error);

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ReadError: {}", self.0)
    }
}

impl core::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(&self.0)
    }
}

