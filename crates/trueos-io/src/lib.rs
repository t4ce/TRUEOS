#![no_std]

//! TRUEOS platform I/O facade.
//!
//! This is the no_std, alloc-backed subset that maps cleanly to the parts of
//! `std::io` used by kernel-side ecosystem crates.

extern crate alloc;

use core::ops::{Deref, DerefMut};

pub use core3::io::{
    BufRead, Bytes, Chain, Cursor, Error, ErrorKind, Read, Result, Seek, SeekFrom, Take, Write,
};

/// Platform I/O prelude, matching the std-facing traits crates usually need.
pub mod prelude {
    pub use super::{BufRead, Read, Seek, Write};
}

/// Borrowed byte buffer for vectored writes.
#[derive(Clone, Copy, Debug)]
pub struct IoSlice<'a>(&'a [u8]);

impl<'a> IoSlice<'a> {
    /// Create an I/O slice from a shared byte slice.
    #[inline]
    pub fn new(buf: &'a [u8]) -> Self {
        Self(buf)
    }
}

impl AsRef<[u8]> for IoSlice<'_> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

impl Deref for IoSlice<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.0
    }
}

/// Borrowed mutable byte buffer for vectored reads.
#[derive(Debug)]
pub struct IoSliceMut<'a>(&'a mut [u8]);

impl<'a> IoSliceMut<'a> {
    /// Create an I/O slice from a mutable byte slice.
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self(buf)
    }
}

impl AsRef<[u8]> for IoSliceMut<'_> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

impl AsMut<[u8]> for IoSliceMut<'_> {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        self.0
    }
}

impl Deref for IoSliceMut<'_> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.0
    }
}

impl DerefMut for IoSliceMut<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        self.0
    }
}

/// Build a platform I/O error.
#[inline]
pub fn error(kind: ErrorKind, detail: &'static str) -> Error {
    Error::new(kind, detail)
}

#[inline]
pub fn other(detail: &'static str) -> Error {
    error(ErrorKind::Other, detail)
}

#[inline]
pub fn invalid_input(detail: &'static str) -> Error {
    error(ErrorKind::InvalidInput, detail)
}

#[inline]
pub fn would_block(detail: &'static str) -> Error {
    error(ErrorKind::WouldBlock, detail)
}

#[inline]
pub fn not_connected(detail: &'static str) -> Error {
    error(ErrorKind::NotConnected, detail)
}

#[inline]
pub fn timed_out(detail: &'static str) -> Error {
    error(ErrorKind::TimedOut, detail)
}

/// Map common POSIX-style errno values to the platform I/O error space.
#[inline]
pub fn errno_kind(errno: i32) -> ErrorKind {
    match errno {
        2 => ErrorKind::NotFound,
        11 | 35 | 115 => ErrorKind::WouldBlock,
        13 => ErrorKind::PermissionDenied,
        17 => ErrorKind::AlreadyExists,
        22 => ErrorKind::InvalidInput,
        32 => ErrorKind::BrokenPipe,
        98 => ErrorKind::AddrInUse,
        99 => ErrorKind::AddrNotAvailable,
        103 => ErrorKind::ConnectionAborted,
        104 => ErrorKind::ConnectionReset,
        106 => ErrorKind::AlreadyExists,
        107 => ErrorKind::NotConnected,
        110 => ErrorKind::TimedOut,
        111 => ErrorKind::ConnectionRefused,
        _ => ErrorKind::Other,
    }
}

/// Map TRUEOS negative socket/status returns to the platform I/O error space.
#[inline]
pub fn status_kind(status: i32) -> ErrorKind {
    match status {
        -2 => ErrorKind::WouldBlock,
        -3 => ErrorKind::NotConnected,
        -4 => ErrorKind::InvalidInput,
        -5 | -8 => ErrorKind::NotFound,
        -6 => ErrorKind::AddrInUse,
        -7 => ErrorKind::TimedOut,
        _ => ErrorKind::Other,
    }
}

#[inline]
pub fn errno_error(errno: i32) -> Error {
    error(errno_kind(errno), "platform errno")
}

#[inline]
pub fn status_error(status: i32) -> Error {
    error(status_kind(status), "platform status")
}
