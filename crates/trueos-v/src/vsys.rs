use alloc::string::String;
use core::fmt;
use core::fmt::Write as _;

use crate::vcabi;

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleStream {
    Out = 1,
    Err = 2,
}

impl ConsoleStream {
    #[inline]
    pub const fn raw(self) -> u32 {
        self as u32
    }

    #[inline]
    pub const fn from_raw(stream: u32) -> Self {
        match stream {
            2 => Self::Err,
            _ => Self::Out,
        }
    }
}

#[inline]
pub fn poll_once() {
    unsafe { vcabi::trueos_cabi_poll_once() }
}

#[inline]
pub fn sleep_ms(ms: u64) {
    unsafe { vcabi::trueos_cabi_sleep_ms(ms) }
}

#[inline]
pub fn write_console_stream(stream: ConsoleStream, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { vcabi::trueos_cabi_write(stream.raw(), bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn write_out(bytes: &[u8]) {
    write_console_stream(ConsoleStream::Out, bytes);
}

#[inline]
pub fn write_err(bytes: &[u8]) {
    write_console_stream(ConsoleStream::Err, bytes);
}

#[inline]
pub fn write_stream(stream: u32, bytes: &[u8]) {
    write_console_stream(ConsoleStream::from_raw(stream), bytes);
}

#[inline]
pub fn write_console_log_stream(stream: ConsoleStream, s: &str) {
    write_console_stream(stream, s.as_bytes());
}

#[inline]
pub fn write_log_stream(stream: u32, s: &str) {
    write_console_log_stream(ConsoleStream::from_raw(stream), s);
}

#[inline]
pub fn log_info(s: &str) {
    write_console_log_stream(ConsoleStream::Out, s);
}

#[inline]
pub fn log_error(s: &str) {
    write_console_log_stream(ConsoleStream::Err, s);
}

#[inline]
pub fn log_infof(args: fmt::Arguments<'_>) {
    logf(ConsoleStream::Out, args);
}

#[inline]
pub fn log_errorf(args: fmt::Arguments<'_>) {
    logf(ConsoleStream::Err, args);
}

#[inline]
pub fn log_info_with_args(prefix: &str, args: &[&str]) {
    log_with_args(ConsoleStream::Out, prefix, args);
}

#[inline]
pub fn log_error_with_args(prefix: &str, args: &[&str]) {
    log_with_args(ConsoleStream::Err, prefix, args);
}

fn log_with_args(stream: ConsoleStream, prefix: &str, args: &[&str]) {
    let mut line = String::from(prefix);
    if args.is_empty() {
        line.push_str(" args=(none)\n");
    } else {
        line.push_str(" args=");
        for (idx, arg) in args.iter().enumerate() {
            if idx != 0 {
                line.push(' ');
            }
            line.push_str(arg);
        }
        line.push('\n');
    }

    write_console_log_stream(stream, line.as_str());
}

fn logf(stream: ConsoleStream, args: fmt::Arguments<'_>) {
    let mut line = String::new();
    let _ = line.write_fmt(args);
    if !line.ends_with('\n') {
        line.push('\n');
    }
    write_console_log_stream(stream, line.as_str());
}
