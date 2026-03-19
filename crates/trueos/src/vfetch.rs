extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::vcabi;

#[inline]
pub fn prewarm_url(url: &[u8]) -> i32 {
    unsafe { vcabi::trueos_cabi_net_prewarm_url_start(url.as_ptr(), url.len()) }
}

#[inline]
pub fn fetch_to_file(url: &[u8], path: &[u8]) -> Result<u32, i32> {
    let op_id = unsafe {
        vcabi::trueos_cabi_net_fetch_start(url.as_ptr(), url.len(), path.as_ptr(), path.len())
    };
    if op_id == 0 {
        return Err(-1);
    }
    Ok(op_id)
}

#[inline]
pub fn fetch_bytes(url: &[u8]) -> Result<u32, i32> {
    let op_id = unsafe { vcabi::trueos_cabi_net_fetch_bytes_start(url.as_ptr(), url.len()) };
    if op_id == 0 {
        return Err(-1);
    }
    Ok(op_id)
}

#[inline]
pub fn fetch_post_json_to_file(
    url: &[u8],
    path: &[u8],
    body: &[u8],
    bearer: Option<&[u8]>,
) -> Result<u32, i32> {
    let (bearer_ptr, bearer_len) = match bearer {
        Some(token) => (token.as_ptr(), token.len()),
        None => (core::ptr::null(), 0),
    };
    let op_id = unsafe {
        vcabi::trueos_cabi_net_fetch_post_json_start(
            url.as_ptr(),
            url.len(),
            path.as_ptr(),
            path.len(),
            body.as_ptr(),
            body.len(),
            bearer_ptr,
            bearer_len,
        )
    };
    if op_id == 0 {
        return Err(-1);
    }
    Ok(op_id)
}

#[inline]
pub fn fetch_result(op_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_net_fetch_result(op_id) }
}

#[inline]
pub fn fetch_wait(op_id: u32, timeout_ms: u64) -> i32 {
    unsafe { vcabi::trueos_cabi_net_fetch_wait(op_id, timeout_ms) }
}

#[inline]
pub fn fetch_discard(op_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_net_fetch_discard(op_id) }
}

#[inline]
pub fn fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32 {
    unsafe { vcabi::trueos_cabi_net_fetch_bytes_wait(op_id, timeout_ms) }
}

#[inline]
pub fn fetch_bytes_result_len(op_id: u32) -> Result<usize, i32> {
    let len = unsafe { vcabi::trueos_cabi_net_fetch_bytes_result_len(op_id) };
    if len < 0 {
        return Err(len as i32);
    }
    Ok(len as usize)
}

#[inline]
pub fn fetch_bytes_read(op_id: u32) -> Result<Vec<u8>, i32> {
    let len = fetch_bytes_result_len(op_id)?;
    let mut bytes = vec![0u8; len];
    let got =
        unsafe { vcabi::trueos_cabi_net_fetch_bytes_read(op_id, bytes.as_mut_ptr(), bytes.len()) };
    if got < 0 {
        return Err(got as i32);
    }
    bytes.truncate(got as usize);
    Ok(bytes)
}

#[inline]
pub fn fetch_bytes_discard(op_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_net_fetch_bytes_discard(op_id) }
}
