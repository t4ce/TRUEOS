extern crate alloc;

use alloc::string::String;
use alloc::vec::IntoIter;
use alloc::vec::Vec;

/// Iterator over process arguments.
///
/// TRUEOS does not currently expose process argv through the stable v ABI,
/// so this yields no items for now.
pub struct Args {
    inner: IntoIter<String>,
}

impl Iterator for Args {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarError {
    NotPresent,
    NotUnicode,
}

pub fn args() -> Args {
    Args {
        inner: Vec::new().into_iter(),
    }
}

pub fn var<K: AsRef<str>>(_key: K) -> Result<String, VarError> {
    Err(VarError::NotPresent)
}