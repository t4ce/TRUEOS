// Reference source for library/std/src/sys/paths/trueos.rs.
// Native launch scope and byte environment supply paths. There is no process
// cwd mutation, passwd database, or /proc executable link on this platform.

use crate::ffi::{OsStr, OsString};
use crate::io;
use crate::os::unix::ffi::{OsStrExt, OsStringExt};
use crate::path::{Path, PathBuf};
use crate::{fmt, slice};

unsafe extern "C" {
    fn trueos_cabi_env_current_dir(out: *mut u8, capacity: usize) -> isize;
}

pub fn getcwd() -> io::Result<PathBuf> {
    let mut bytes = Vec::new();
    loop {
        let count = unsafe { trueos_cabi_env_current_dir(bytes.as_mut_ptr(), bytes.len()) };
        let count = usize::try_from(count)
            .map_err(|_| io::Error::other("native launch scope query failed"))?;
        if count > 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native launch scope is too long",
            ));
        }
        if count <= bytes.len() {
            bytes.truncate(count);
            return Ok(PathBuf::from(OsString::from_vec(bytes)));
        }
        bytes.resize(count, 0);
    }
}

pub fn chdir(_: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "TRUEOS uses an immutable launch filesystem scope",
    ))
}

pub fn current_exe() -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "a Blueprint module has no native executable filesystem path",
    ))
}

pub fn temp_dir() -> PathBuf {
    crate::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn home_dir() -> Option<PathBuf> {
    crate::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

#[derive(Clone, Debug)]
pub struct SplitPaths<'a> {
    inner: slice::Split<'a, u8, fn(&u8) -> bool>,
}

pub fn split_paths(value: &OsStr) -> SplitPaths<'_> {
    fn separator(byte: &u8) -> bool {
        *byte == b':'
    }
    SplitPaths {
        inner: value.as_bytes().split(separator),
    }
}

impl Iterator for SplitPaths<'_> {
    type Item = PathBuf;
    fn next(&mut self) -> Option<PathBuf> {
        self.inner
            .next()
            .map(|part| PathBuf::from(OsStr::from_bytes(part)))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl DoubleEndedIterator for SplitPaths<'_> {
    fn next_back(&mut self) -> Option<PathBuf> {
        self.inner
            .next_back()
            .map(|part| PathBuf::from(OsStr::from_bytes(part)))
    }
}

#[derive(Debug)]
pub struct JoinPathsError;

pub fn join_paths<I, T>(paths: I) -> Result<OsString, JoinPathsError>
where
    I: Iterator<Item = T>,
    T: AsRef<OsStr>,
{
    let mut bytes = Vec::new();
    for (index, path) in paths.enumerate() {
        if index != 0 {
            bytes.push(b':');
        }
        let path = path.as_ref().as_bytes();
        if path.contains(&b':') {
            return Err(JoinPathsError);
        }
        bytes.extend_from_slice(path);
    }
    Ok(OsString::from_vec(bytes))
}
impl fmt::Display for JoinPathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "path segment contains separator `:`")
    }
}
impl crate::error::Error for JoinPathsError {}
