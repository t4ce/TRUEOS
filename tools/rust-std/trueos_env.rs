// Reference source for library/std/src/sys/env/trueos.rs.
// The kernel's existing native process environment owns all values and mutations.

pub use super::common::Env;
use crate::ffi::{OsStr, OsString};
use crate::io;
use crate::os::unix::ffi::{OsStrExt, OsStringExt};

unsafe extern "C" {
    fn trueos_cabi_env_get_v1(
        key: *const u8,
        key_len: usize,
        out: *mut u8,
        capacity: usize,
    ) -> isize;
    fn trueos_cabi_env_set_v1(
        key: *const u8,
        key_len: usize,
        value: *const u8,
        value_len: usize,
    ) -> i32;
    fn trueos_cabi_env_remove_v1(key: *const u8, key_len: usize) -> i32;
    fn trueos_cabi_env_snapshot_v1(out: *mut u8, capacity: usize) -> isize;
}

const MAX_VALUE: usize = 64 * 1024;
const MAX_SNAPSHOT: usize = 1024 * 1024 + 8 * 1024 + 4;

fn copied_bytes(
    maximum: usize,
    mut read: impl FnMut(*mut u8, usize) -> isize,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let count = read(bytes.as_mut_ptr(), bytes.len());
        let count = usize::try_from(count).map_err(|_| {
            io::Error::from_raw_os_error(i32::try_from(count.saturating_neg()).unwrap_or(5))
        })?;
        if count > maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "native environment length exceeds its contract",
            ));
        }
        if count <= bytes.len() {
            bytes.truncate(count);
            return Ok(bytes);
        }
        // The read reports the current required capacity without writing when
        // a concurrent mutation grew the value/snapshot between the probes.
        bytes.try_reserve_exact(count - bytes.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "native environment allocation failed",
            )
        })?;
        bytes.resize(count, 0);
    }
}

fn pairs(bytes: &[u8]) -> io::Result<Vec<(OsString, OsString)>> {
    fn invalid() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid native environment snapshot",
        )
    }
    fn word(bytes: &[u8], offset: &mut usize) -> io::Result<usize> {
        let end = offset.checked_add(4).ok_or_else(invalid)?;
        let value = u32::from_le_bytes(
            bytes
                .get(*offset..end)
                .ok_or_else(invalid)?
                .try_into()
                .unwrap(),
        );
        *offset = end;
        Ok(value as usize)
    }
    let mut offset = 0;
    let count = word(bytes, &mut offset)?;
    if count > 1024 {
        return Err(invalid());
    }
    let mut pairs = Vec::with_capacity(count);
    for _ in 0..count {
        let key_len = word(bytes, &mut offset)?;
        let value_len = word(bytes, &mut offset)?;
        let key_end = offset.checked_add(key_len).ok_or_else(invalid)?;
        let value_end = key_end.checked_add(value_len).ok_or_else(invalid)?;
        let key = bytes.get(offset..key_end).ok_or_else(invalid)?;
        let value = bytes.get(key_end..value_end).ok_or_else(invalid)?;
        if key.is_empty()
            || key_len > MAX_VALUE
            || value_len > MAX_VALUE
            || key.contains(&0)
            || key.contains(&b'=')
            || value.contains(&0)
        {
            return Err(invalid());
        }
        pairs.push((
            OsString::from_vec(key.to_vec()),
            OsString::from_vec(value.to_vec()),
        ));
        offset = value_end;
    }
    if offset != bytes.len() {
        return Err(invalid());
    }
    Ok(pairs)
}

pub fn env() -> Env {
    let bytes = copied_bytes(MAX_SNAPSHOT, |out, len| unsafe {
        trueos_cabi_env_snapshot_v1(out, len)
    })
    .unwrap_or_else(|error| panic!("cannot snapshot native environment: {error}"));
    Env::new(
        pairs(&bytes).unwrap_or_else(|error| panic!("cannot decode native environment: {error}")),
    )
}

pub fn getenv(key: &OsStr) -> Option<OsString> {
    let key = key.as_bytes();
    if key.is_empty() || key.contains(&0) || key.contains(&b'=') {
        return None;
    }
    copied_bytes(MAX_VALUE, |out, len| unsafe {
        trueos_cabi_env_get_v1(key.as_ptr(), key.len(), out, len)
    })
    .ok()
    .map(OsString::from_vec)
}

pub unsafe fn setenv(key: &OsStr, value: &OsStr) -> io::Result<()> {
    let (key, value) = (key.as_bytes(), value.as_bytes());
    let status =
        unsafe { trueos_cabi_env_set_v1(key.as_ptr(), key.len(), value.as_ptr(), value.len()) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.saturating_neg()))
    }
}

pub unsafe fn unsetenv(key: &OsStr) -> io::Result<()> {
    let key = key.as_bytes();
    let status = unsafe { trueos_cabi_env_remove_v1(key.as_ptr(), key.len()) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.saturating_neg()))
    }
}
