// SPDX-FileCopyrightText: 2020 Ilaï Deutel & Kibi Contributors
//
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate alloc;

use alloc::vec::Vec;
use core::str::FromStr;
use trueos_io as io;
use v::{vcabi, vsys};

use crate::{Error, ansi_escape::*};

/// Obtain the window size using the cursor position.
///
/// This function moves the cursor to the bottom-right using ANSI escape
/// sequence `\x1b[999C\x1b[999B`, then requests the cursor position using ANSI
/// escape sequence `\x1b[6n`. After this sequence is sent, the next characters
/// on stdin should be `\x1b[{row};{column}R`.
///
/// It is used as an alternative method if `sys::get_window_size()` returns an
/// error.
pub fn get_window_size_using_cursor() -> Result<(usize, usize), Error> {
    vsys::write_out(REPOSITION_CURSOR_END.as_bytes());
    vsys::write_out(DEVICE_STATUS_REPORT.as_bytes());
    let mut prefix_buffer = [0u8; 2];
    read_exact(&mut prefix_buffer)?;
    if prefix_buffer != *b"\x1b[" {
        return Err(Error::CursorPosition);
    }
    Ok((read_value_until(b';')?, read_value_until(b'R')?))
}

/// Read value until a certain stop byte is reached, and parse the result
/// (pre-stop byte).
fn read_value_until<T: FromStr>(stop_byte: u8) -> Result<T, Error> {
    let mut buf = Vec::new();
    read_until(stop_byte, &mut buf)?;
    // Check that we have reached `stop_byte`, not EOF.
    buf.pop().filter(|u| *u == stop_byte).ok_or(Error::CursorPosition)?;
    // TODO: https://github.com/rust-lang/rust/issues/134821 - Use from_ascii when stabilized
    core::str::from_utf8(&buf).or(Err(Error::CursorPosition))?.parse().or(Err(Error::CursorPosition))
}

#[cfg_attr(any(windows, target_os = "wasi"), expect(clippy::trivially_copy_pass_by_ref))]
pub fn restore_terminal<T>(_orig_term_mode: &T) -> io::Result<()> {
    vsys::write_out(USE_MAIN_SCREEN.as_bytes());
    Ok(())
}

fn read_exact(buf: &mut [u8]) -> Result<(), Error> {
    for byte in buf.iter_mut() {
        *byte = read_byte()?;
    }
    Ok(())
}

fn read_until(stop_byte: u8, buf: &mut Vec<u8>) -> Result<(), Error> {
    loop {
        let byte = read_byte()?;
        buf.push(byte);
        if byte == stop_byte {
            return Ok(());
        }
    }
}

fn read_byte() -> Result<u8, Error> {
    let byte = unsafe { vcabi::trueos_cabi_shell_attached_read_byte() };
    u8::try_from(byte).map_err(|_| Error::CursorPosition)
}
