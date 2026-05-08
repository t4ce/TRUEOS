extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

use crate::vcabi;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FsNodeKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FsStat {
    pub kind: FsNodeKind,
    pub len: u64,
}

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
pub fn exists(path: &[u8]) -> Result<bool, i32> {
    let rc = unsafe { vcabi::trueos_cabi_fs_exists(path.as_ptr(), path.len()) };
    if rc < 0 {
        return Err(rc);
    }
    Ok(rc != 0)
}

#[inline]
pub fn stat(path: &[u8]) -> Result<FsStat, i32> {
    let mut raw_kind = 0u32;
    let mut len = 0u64;
    let rc = unsafe {
        vcabi::trueos_cabi_fs_stat(
            path.as_ptr(),
            path.len(),
            &mut raw_kind as *mut u32,
            &mut len as *mut u64,
        )
    };
    if rc != 0 {
        return Err(rc);
    }
    let kind = match raw_kind {
        1 => FsNodeKind::File,
        2 => FsNodeKind::Directory,
        _ => return Err(-4),
    };
    Ok(FsStat { kind, len })
}

#[inline]
pub fn write_file(path: &[u8], data: &[u8]) -> Result<(), i32> {
    let handle = write_begin(path, data.len() as u64)?;
    if let Err(rc) = write_chunk(handle, data) {
        let _ = write_abort(handle);
        return Err(rc);
    }
    if let Err(rc) = write_finish(handle) {
        let _ = write_abort(handle);
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn create_dir_all(path: &[u8]) -> Result<(), i32> {
    let rc = unsafe { vcabi::trueos_cabi_fs_create_dir_all(path.as_ptr(), path.len()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

#[inline]
pub fn write_file_utf8(path: &[u8], data: &str) -> Result<(), i32> {
    write_file(path, data.as_bytes())
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
        vcabi::trueos_cabi_trueosfs_primary_html_tree(max_entries, bytes.as_mut_ptr(), bytes.len())
    };
    if got < 0 {
        return Err(got as i32);
    }
    bytes.truncate(got as usize);
    Ok(bytes)
}

#[inline]
pub fn trueosfs_primary_html_tree_utf8(max_entries: u32) -> Result<String, i32> {
    let bytes = trueosfs_primary_html_tree(max_entries)?;
    String::from_utf8(bytes).map_err(|_| -1)
}

#[inline]
pub fn trueosfs_json_all(max_entries: u32) -> Result<Vec<u8>, i32> {
    let len =
        unsafe { vcabi::trueos_cabi_trueosfs_json_all(max_entries, core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_trueosfs_json_all(max_entries, bytes.as_mut_ptr(), bytes.len())
    };
    if got < 0 {
        return Err(got as i32);
    }
    bytes.truncate(got as usize);
    Ok(bytes)
}

#[inline]
pub fn trueosfs_json_all_utf8(max_entries: u32) -> Result<String, i32> {
    let bytes = trueosfs_json_all(max_entries)?;
    String::from_utf8(bytes).map_err(|_| -1)
}
