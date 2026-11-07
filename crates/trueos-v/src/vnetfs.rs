extern crate alloc;

use alloc::string::String;
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

/// Start a finite native GET. The deadline includes service readiness, DNS,
/// redirects and body reception; the decoded body is bounded before publishing.
pub fn fetch_bytes_with_options(url: &[u8], timeout_ms: u32, max_bytes: usize) -> Result<u32, i32> {
    if timeout_ms == 0 || timeout_ms > 90_000 || max_bytes == 0 || max_bytes > 512 * 1024 * 1024 {
        return Err(-4);
    }
    let id = unsafe {
        vcabi::trueos_cabi_net_fetch_bytes_start_with_options_v1(
            url.as_ptr(), url.len(), timeout_ms, max_bytes as u64,
        )
    };
    if id == 0 { Err(-4) } else { Ok(id) }
}

/// Fetch a browser subdocument with explicit, bounded Referer context.
#[inline]
pub fn fetch_bytes_with_referrer(url: &[u8], referrer: &[u8]) -> Result<u32, i32> {
    let id = unsafe {
        vcabi::trueos_cabi_net_fetch_bytes_start_with_referrer(
            url.as_ptr(), url.len(), referrer.as_ptr(), referrer.len(),
        )
    };
    if id == 0 { Err(-1) } else { Ok(id) }
}

/// Race IPv6 and IPv4 only for the initial HTTPS connection of this fetch.
#[inline]
pub fn fetch_bytes_happy_eyeballs(url: &[u8]) -> Result<u32, i32> {
    let id =
        unsafe { vcabi::trueos_cabi_net_fetch_bytes_start_happy_eyeballs(url.as_ptr(), url.len()) };
    if id == 0 { Err(-1) } else { Ok(id) }
}

#[inline]
pub fn fetch_post_json_to_file(
    url: &[u8],
    path: &[u8],
    body: &[u8],
    bearer: Option<&[u8]>,
) -> Result<u32, i32> {
    fetch_post_json_to_file_with_timeout(url, path, body, bearer, 15_000)
}

#[inline]
pub fn fetch_post_json_to_file_with_timeout(
    url: &[u8],
    path: &[u8],
    body: &[u8],
    bearer: Option<&[u8]>,
    timeout_ms: u32,
) -> Result<u32, i32> {
    let (bearer_ptr, bearer_len) = match bearer {
        Some(token) => (token.as_ptr(), token.len()),
        None => (core::ptr::null(), 0),
    };
    let op_id = unsafe {
        vcabi::trueos_cabi_net_fetch_post_json_start_with_timeout(
            url.as_ptr(),
            url.len(),
            path.as_ptr(),
            path.len(),
            body.as_ptr(),
            body.len(),
            bearer_ptr,
            bearer_len,
            timeout_ms,
        )
    };
    if op_id == 0 {
        return Err(-1);
    }
    Ok(op_id)
}

#[inline]
pub fn fetch_post_json_bytes(url: &[u8], body: &[u8], bearer: Option<&[u8]>) -> Result<u32, i32> {
    fetch_post_json_bytes_with_timeout(url, body, bearer, 15_000)
}

#[inline]
pub fn fetch_post_json_bytes_with_timeout(
    url: &[u8],
    body: &[u8],
    bearer: Option<&[u8]>,
    timeout_ms: u32,
) -> Result<u32, i32> {
    let (bearer_ptr, bearer_len) = match bearer {
        Some(token) => (token.as_ptr(), token.len()),
        None => (core::ptr::null(), 0),
    };
    let op_id = unsafe {
        vcabi::trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
            url.as_ptr(),
            url.len(),
            body.as_ptr(),
            body.len(),
            bearer_ptr,
            bearer_len,
            timeout_ms,
        )
    };
    if op_id == 0 {
        return Err(-1);
    }
    Ok(op_id)
}

#[inline]
pub fn fetch_result(op_id: u32) -> i32 {
    vcabi::trueos_cabi_net_fetch_result(op_id)
}

#[inline]
pub fn fetch_wait(op_id: u32, timeout_ms: u64) -> i32 {
    vcabi::trueos_cabi_net_fetch_wait(op_id, timeout_ms)
}

#[inline]
pub fn fetch_discard(op_id: u32) -> i32 {
    vcabi::trueos_cabi_net_fetch_discard(op_id)
}

#[inline]
pub fn fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32 {
    vcabi::trueos_cabi_net_fetch_bytes_wait(op_id, timeout_ms)
}

#[inline]
pub fn fetch_bytes_result_len(op_id: u32) -> Result<usize, i32> {
    let len = unsafe { vcabi::trueos_cabi_net_fetch_bytes_result_len(op_id) };
    if len < 0 {
        return Err(len as i32);
    }
    Ok(len as usize)
}

/// Final URL from the same completed GET. Read before the body, whose final
/// chunk consumes the operation. This metadata read leaves the body intact.
#[inline]
pub fn fetch_bytes_final_url(op_id: u32) -> Result<String, i32> {
    let len = unsafe { vcabi::trueos_cabi_net_fetch_bytes_final_url(op_id, core::ptr::null_mut(), 0) };
    if len < 0 { return Err(len as i32); }
    if len == 0 { return Err(-4); }
    if len > 64 * 1024 { return Err(-7); }
    let mut bytes = vec![0u8; len as usize];
    let got = unsafe { vcabi::trueos_cabi_net_fetch_bytes_final_url(op_id, bytes.as_mut_ptr(), bytes.len()) };
    if got < 0 { return Err(got as i32); }
    if got != len { return Err(-2); }
    String::from_utf8(bytes).map_err(|_| -1)
}

#[inline]
/// Consume one completed body with a caller-owned size limit. Any failure
/// discards the operation; short reads are errors rather than truncated success.
pub fn fetch_bytes_read_bounded(op_id: u32, max_bytes: usize) -> Result<Vec<u8>, i32> {
    struct Operation(u32);
    impl Drop for Operation {
        fn drop(&mut self) { let _ = fetch_bytes_discard(self.0); }
    }
    let _operation = Operation(op_id);
    if op_id == 0 || max_bytes == 0 || max_bytes > 512 * 1024 * 1024 { return Err(-4); }
    let len = fetch_bytes_result_len(op_id)?;
    if len > max_bytes { return Err(-7); }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| -5)?;
    bytes.resize(len, 0);
    let got = unsafe { vcabi::trueos_cabi_net_fetch_bytes_read(op_id, bytes.as_mut_ptr(), bytes.len()) };
    if got < 0 { return Err(got as i32); }
    if got as usize != len { return Err(-2); }
    Ok(bytes)
}

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
    vcabi::trueos_cabi_net_fetch_bytes_discard(op_id)
}
