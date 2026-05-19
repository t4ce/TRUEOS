// SPDX-FileCopyrightText: 2020 Ilaï Deutel & Kibi Contributors
//
// SPDX-License-Identifier: MIT OR Apache-2.0

//! # Errors

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::fmt;
use trueos_io as io;

/// Kibi error type.
#[derive(Debug)]
pub enum Error {
    /// Wrapper around `trueos_io::Error`
    Io(io::Error),
    /// Wrapper around `core::fmt::Error`
    Fmt(fmt::Error),
    /// Error returned when the window size obtained through a system call is
    /// invalid.
    InvalidWindowSize,
    /// Error setting or retrieving the cursor position.
    CursorPosition,
    /// Too many arguments given to kibi. The attribute corresponds to the total
    /// number of command line arguments.
    TooManyArguments(Vec<String>),
    /// Unrecognized option given as a command line argument.
    BadOption(String),
}

impl From<io::Error> for Error {
    /// Convert an IO Error into a Kibi Error.
    fn from(err: io::Error) -> Self { Self::Io(err) }
}

impl From<fmt::Error> for Error {
    /// Convert an Fmt Error into a Kibi Error.
    fn from(err: fmt::Error) -> Self { Self::Fmt(err) }
}
