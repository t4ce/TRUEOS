//! Unix pipe entry points on the native socket provider.
//!
//! A native socket ID is not a Unix descriptor. No pipe handle can be created
//! until TRUEOS has an actual pipe provider; no file capability is reinterpreted.
use crate::{Interest, Registry, Token, event};
use std::io::{self, Read, Write};

/// Return `Unsupported` before allocating a pipe or acquiring a descriptor.
pub fn new() -> io::Result<(Sender, Receiver)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "TRUEOS has no Unix pipe provider",
    ))
}

/// Sending end of a Unix pipe. Native TRUEOS cannot construct one.
#[derive(Debug)]
pub enum Sender {}
/// Receiving end of a Unix pipe. Native TRUEOS cannot construct one.
#[derive(Debug)]
pub enum Receiver {}

macro_rules! pipe_end {
    ($end:ty) => {
        impl $end {
            /// No native pipe handle exists.
            pub fn set_nonblocking(&self, _: bool) -> io::Result<()> {
                match *self {}
            }
            /// No native pipe handle exists; the closure is never called.
            pub fn try_io<F, T>(&self, _: F) -> io::Result<T>
            where
                F: FnOnce() -> io::Result<T>,
            {
                match *self {}
            }
        }
        impl event::Source for $end {
            fn register(&mut self, _: &Registry, _: Token, _: Interest) -> io::Result<()> {
                match *self {}
            }
            fn reregister(&mut self, _: &Registry, _: Token, _: Interest) -> io::Result<()> {
                match *self {}
            }
            fn deregister(&mut self, _: &Registry) -> io::Result<()> {
                match *self {}
            }
        }
    };
}
pipe_end!(Sender);
pipe_end!(Receiver);
impl Write for Sender {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        match *self {}
    }
    fn flush(&mut self) -> io::Result<()> {
        match *self {}
    }
}
impl Write for &Sender {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        match **self {}
    }
    fn flush(&mut self) -> io::Result<()> {
        match **self {}
    }
}
impl Read for Receiver {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        match *self {}
    }
}
impl Read for &Receiver {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        match **self {}
    }
}
