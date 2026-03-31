extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::IntoIter;
use alloc::vec::Vec;

use crate::vcabi;

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
    let count = unsafe { vcabi::trueos_cabi_env_args_count() };
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let len = unsafe { vcabi::trueos_cabi_env_arg(index, core::ptr::null_mut(), 0) };
        if len <= 0 {
            continue;
        }
        let mut bytes = vec![0u8; len as usize];
        let got = unsafe { vcabi::trueos_cabi_env_arg(index, bytes.as_mut_ptr(), bytes.len()) };
        if got <= 0 {
            continue;
        }
        bytes.truncate(got as usize);
        if let Ok(arg) = String::from_utf8(bytes) {
            values.push(arg);
        }
    }

    Args {
        inner: values.into_iter(),
    }
}

pub fn var<K: AsRef<str>>(key: K) -> Result<String, VarError> {
    let key = key.as_ref();
    let len =
        unsafe { vcabi::trueos_cabi_env_var(key.as_ptr(), key.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(VarError::NotPresent);
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_env_var(key.as_ptr(), key.len(), bytes.as_mut_ptr(), bytes.len())
    };
    if got < 0 {
        return Err(VarError::NotPresent);
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).map_err(|_| VarError::NotUnicode)
}
