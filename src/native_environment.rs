//! Byte-oriented access to the existing realm-owned native environment.
//!
//! Buffers are copied while the environment lock is held. No C strings,
//! descriptor state, borrowed kernel pointers, or secondary environment exists.

use crate::native_abi::{abi_read_bytes, abi_write_bytes};
use crate::r::native_process;

const EINVAL: i32 = 22;
const ENOENT: i32 = 2;
const MAX_STRING: usize = 64 * 1024;

fn key_bytes<'a>(key: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 || len > MAX_STRING {
        return None;
    }
    let bytes = abi_read_bytes(key, len)?;
    if bytes.contains(&0) || bytes.contains(&b'=') {
        return None;
    }
    Some(bytes)
}

fn result_code(result: Result<Result<(), i32>, i32>) -> i32 {
    match result {
        Ok(Ok(())) => 0,
        Ok(Err(error)) | Err(error) => -error,
    }
}

/// Read one value as bytes, preserving empty and non-UTF-8 values. A null or
/// short output queries the required length without writing any bytes. Missing
/// keys return -2; other failures return a negative native error code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_env_get_v1(
    key: *const u8,
    key_len: usize,
    out: *mut u8,
    capacity: usize,
) -> isize {
    let Some(key) = key_bytes(key, key_len) else {
        return -(EINVAL as isize);
    };
    match native_process::with_environment(native_process::current_vm(), |environment| {
        let Some(value) = environment.get(key) else {
            return -(ENOENT as isize);
        };
        if out.is_null() || capacity < value.len() {
            return value.len() as isize;
        }
        let Some(target) = abi_write_bytes(out, value.len()) else {
            return -(EINVAL as isize);
        };
        target.copy_from_slice(value);
        value.len() as isize
    }) {
        Ok(result) => result,
        Err(error) => -(error as isize),
    }
}

/// Replace one mutable variable. Reserved TRUEOS_* keys, invalid keys/values,
/// and exhausted environment capacity fail without changing the old value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_env_set_v1(
    key: *const u8,
    key_len: usize,
    value: *const u8,
    value_len: usize,
) -> i32 {
    let Some(key) = key_bytes(key, key_len) else {
        return -EINVAL;
    };
    if value_len > MAX_STRING {
        return -12;
    }
    let Some(value) = abi_read_bytes(value, value_len) else {
        return -EINVAL;
    };
    result_code(native_process::with_environment(
        native_process::current_vm(),
        |environment| environment.set(key, value, true),
    ))
}

/// Remove one mutable variable. Removing an absent valid key succeeds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_env_remove_v1(key: *const u8, key_len: usize) -> i32 {
    let Some(key) = key_bytes(key, key_len) else {
        return -EINVAL;
    };
    result_code(native_process::with_environment(
        native_process::current_vm(),
        |environment| environment.unset(key),
    ))
}

/// Copy a consistent snapshot: LE u32 pair count, followed by each pair's LE
/// u32 key length, LE u32 value length, key bytes, and value bytes. A null or
/// short output returns the required length and writes nothing. Each successful
/// copy represents one instant even when other logical threads mutate values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_env_snapshot_v1(out: *mut u8, capacity: usize) -> isize {
    match native_process::with_environment(native_process::current_vm(), |environment| {
        let length = 4 + environment
            .pairs()
            .map(|(key, value)| 8 + key.len() + value.len())
            .sum::<usize>();
        if out.is_null() || capacity < length {
            return length as isize;
        }
        let Some(target) = abi_write_bytes(out, length) else {
            return -(EINVAL as isize);
        };
        target[..4].copy_from_slice(&(environment.pairs().count() as u32).to_le_bytes());
        let mut offset = 4;
        for (key, value) in environment.pairs() {
            target[offset..offset + 4].copy_from_slice(&(key.len() as u32).to_le_bytes());
            target[offset + 4..offset + 8].copy_from_slice(&(value.len() as u32).to_le_bytes());
            offset += 8;
            target[offset..offset + key.len()].copy_from_slice(key);
            offset += key.len();
            target[offset..offset + value.len()].copy_from_slice(value);
            offset += value.len();
        }
        length as isize
    }) {
        Ok(result) => result,
        Err(error) => -(error as isize),
    }
}
