// SPDX-FileCopyrightText: 2020 Ilaï Deutel & Kibi Contributors
//
// SPDX-License-Identifier: MIT OR Apache-2.0

//! # Kibi

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use trueos_io::{ErrorKind, Read};
use trueos_kibi::{run, Error};
use v::{env, vshell};

struct AttachedStdin;

impl Read for AttachedStdin {
    fn read(&mut self, buf: &mut [u8]) -> trueos_io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        match vshell::attached_read_byte() {
            Some(byte) => {
                buf[0] = byte;
                Ok(1)
            }
            None => Err(trueos_io::Error::new(ErrorKind::WouldBlock, "no attached shell input")),
        }
    }
}

fn is_slot_arg(arg: &str) -> bool {
    arg.starts_with('§') && arg.len() > '§'.len_utf8()
}

/// Load the configuration, initialize the editor and run the program,
/// optionally opening a file if an argument is given.
///
/// # Errors
///
/// Any error that occur during the execution of the program will be returned by
/// this function.
fn main() -> Result<(), Error> {
    let args: Vec<_> = env::args().collect();
    let first = args.get(1).map(String::as_str);
    let second = args.get(2).map(String::as_str);
    let third = args.get(3).map(String::as_str);
    let remaining_args = args.len().saturating_sub(4);

    match (first, second, third, remaining_args) {
        (Some("--version"), None | Some("--"), None, 0) => {
            _ = vshell::line(&format!("kibi {}", env!("CARGO_PKG_VERSION")))
        }
        (Some(slot), file, None, 0) if is_slot_arg(slot) => {
            _ = vshell::attached_retarget_slot(slot);
            run(file, &mut AttachedStdin)?;
        }
        (Some(o), ..) if o.starts_with('-') && o != "--" => {
            return Err(Error::BadOption(o.into()));
        }
        (Some("--"), p, None, 0) | (p, Some("--") | None, None, 0) => {
            run(p, &mut AttachedStdin)?;
        }
        _ => return Err(Error::TooManyArguments(args)),
    }
    Ok(())
}
