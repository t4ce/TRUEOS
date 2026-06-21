use core::fmt;

mod bit;

pub use bit::*;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Other,
    UnexpectedEof,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Error {
    kind: ErrorKind,
    desc: &'static str,
}

impl Error {
    pub fn new(kind: ErrorKind, desc: &'static str) -> Self {
        Self { kind, desc }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.desc)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

pub trait ReadBytes {
    fn read_byte(&mut self) -> Result<u8>;

    #[inline(always)]
    fn read_u8(&mut self) -> Result<u8> {
        self.read_byte()
    }
}
