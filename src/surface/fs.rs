use core::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => f.write_str("fs unsupported"),
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct File;

impl File {
    #[inline]
    pub fn open(_path: &str) -> Result<Self> {
        Err(Error::Unsupported)
    }

    #[inline]
    pub fn create(_path: &str) -> Result<Self> {
        Err(Error::Unsupported)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenOptions;

impl OpenOptions {
    #[inline]
    pub fn new() -> Self {
        Self
    }

    #[inline]
    pub fn read(&mut self, _read: bool) -> &mut Self {
        self
    }

    #[inline]
    pub fn write(&mut self, _write: bool) -> &mut Self {
        self
    }

    #[inline]
    pub fn create(&mut self, _create: bool) -> &mut Self {
        self
    }

    #[inline]
    pub fn truncate(&mut self, _truncate: bool) -> &mut Self {
        self
    }

    #[inline]
    pub fn open(&self, _path: &str) -> Result<File> {
        Err(Error::Unsupported)
    }
}
