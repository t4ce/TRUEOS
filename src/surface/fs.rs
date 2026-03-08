extern crate alloc;

use alloc::{string::String, vec::Vec};

use crate::surface::io::kfs;
use crate::surface::path::Path;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NoRoot,
    BadPath,
    NoSpace,
    NotFound,
    AlreadyExists,
    Io,
    Utf8,
}

impl From<kfs::FsError> for Error {
    fn from(value: kfs::FsError) -> Self {
        match value {
            kfs::FsError::NoRoot => Error::NoRoot,
            kfs::FsError::BadPath => Error::BadPath,
            kfs::FsError::NoSpace => Error::NoSpace,
            kfs::FsError::NotFound => Error::NotFound,
            kfs::FsError::AlreadyExists => Error::AlreadyExists,
            kfs::FsError::Device(_) => Error::Io,
        }
    }
}

#[inline]
pub fn exists<P: AsRef<Path>>(path: P) -> Result<bool> {
    kfs::exists(path.as_ref().as_str()).map_err(Into::into)
}

#[inline]
pub fn read<P: AsRef<Path>>(path: P) -> Result<Vec<u8>> {
    kfs::read_file(path.as_ref().as_str()).map_err(Into::into)
}

#[inline]
pub fn read_to_string<P: AsRef<Path>>(path: P) -> Result<String> {
    let bytes = read(path)?;
    String::from_utf8(bytes).map_err(|_| Error::Utf8)
}

#[inline]
pub fn write<P: AsRef<Path>>(path: P, bytes: &[u8]) -> Result<()> {
    let p = path.as_ref().as_str();
    let h = kfs::write_file_begin(p, bytes.len() as u64).map_err(Error::from)?;
    if let Err(e) = kfs::write_file_chunk(h, bytes).map_err(Error::from) {
        let _ = kfs::write_file_abort(h);
        return Err(e);
    }
    kfs::write_file_finish(h).map_err(Error::from)
}

#[inline]
pub fn remove_file<P: AsRef<Path>>(path: P) -> Result<()> {
    kfs::remove(path.as_ref().as_str()).map_err(Into::into)
}

#[inline]
pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> Result<()> {
    kfs::rename(from.as_ref().as_str(), to.as_ref().as_str()).map_err(Into::into)
}
