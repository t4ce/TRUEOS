extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

use crate::vcabi;

#[inline]
pub fn read_file(path: &[u8]) -> Result<Vec<u8>, i32> {
    let len = unsafe {
        vcabi::trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0)
    };
    if len < 0 {
        return Err(len as i32);
    }
    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_fs_read_file(path.as_ptr(), path.len(), bytes.as_mut_ptr(), bytes.len())
    };
    if got < 0 {
        return Err(got as i32);
    }
    bytes.truncate(got as usize);
    Ok(bytes)
}

#[inline]
pub fn read_file_utf8(path: &[u8]) -> Result<String, i32> {
    let bytes = read_file(path)?;
    String::from_utf8(bytes).map_err(|_| -1)
}

#[inline]
pub fn write_begin(path: &[u8], total_len: u64) -> Result<u32, i32> {
    let mut handle = 0u32;
    let rc = unsafe {
        vcabi::trueos_cabi_fs_write_begin(path.as_ptr(), path.len(), total_len, &mut handle)
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(handle)
}

#[inline]
pub fn write_chunk(handle: u32, data: &[u8]) -> Result<(), i32> {
    let rc = unsafe { vcabi::trueos_cabi_fs_write_chunk(handle, data.as_ptr(), data.len()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn write_finish(handle: u32) -> Result<(), i32> {
    let rc = unsafe { vcabi::trueos_cabi_fs_write_finish(handle) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn write_abort(handle: u32) -> Result<(), i32> {
    let rc = unsafe { vcabi::trueos_cabi_fs_write_abort(handle) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn remove(path: &[u8]) -> Result<(), i32> {
    let rc = unsafe { vcabi::trueos_cabi_fs_remove(path.as_ptr(), path.len()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn trueosfs_primary_html_tree(max_entries: u32) -> Result<Vec<u8>, i32> {
    let len = unsafe {
        vcabi::trueos_cabi_trueosfs_primary_html_tree(max_entries, core::ptr::null_mut(), 0)
    };
    if len < 0 {
        return Err(len as i32);
    }
    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_trueosfs_primary_html_tree(
            max_entries,
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    if got < 0 {
        return Err(got as i32);
    }
    bytes.truncate(got as usize);
    Ok(bytes)
}
