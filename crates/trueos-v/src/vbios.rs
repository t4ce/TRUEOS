extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

type BiosReadFn = unsafe extern "C" fn(offset: usize, out_ptr: *mut u8, out_cap: usize) -> isize;

unsafe extern "C" {
    fn trueos_vlayer_bios_schema_snapshot_read(
        offset: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
}

#[inline]
pub fn snapshot_len() -> Result<usize, i32> {
    read_len(trueos_vlayer_bios_schema_snapshot_read)
}

#[inline]
pub fn snapshot_bytes() -> Result<Vec<u8>, i32> {
    read_all(trueos_vlayer_bios_schema_snapshot_read)
}

#[inline]
pub fn snapshot_text() -> Result<String, i32> {
    String::from_utf8(snapshot_bytes()?).map_err(|_| -1)
}

fn read_all(read_fn: BiosReadFn) -> Result<Vec<u8>, i32> {
    let len = read_len(read_fn)?;
    let mut bytes = vec![0u8; len];
    if len == 0 {
        return Ok(bytes);
    }

    let got = unsafe { read_fn(0, bytes.as_mut_ptr(), bytes.len()) };
    if got < 0 {
        return Err(got as i32);
    }

    bytes.truncate((got as usize).min(len));
    Ok(bytes)
}

fn read_len(read_fn: BiosReadFn) -> Result<usize, i32> {
    let len = unsafe { read_fn(0, core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    Ok(len as usize)
}
