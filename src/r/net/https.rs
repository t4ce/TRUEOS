extern crate alloc;

include!("../cabi_codes.rs");

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;

static CABI_NET_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);
static CABI_NET_FETCH_RESULTS: Mutex<BTreeMap<u32, Option<i32>>> = Mutex::new(BTreeMap::new());
static CABI_NET_FETCH_BYTES_RESULTS: Mutex<BTreeMap<u32, CabiNetFetchBytesResult>> =
    Mutex::new(BTreeMap::new());

#[derive(Default)]
struct CabiNetFetchBytesResult {
    rc: Option<i32>,
    body: Vec<u8>,
}

fn monotonic_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

fn fetch_error_to_code(err: &str) -> i32 {
    if err == "timed out" || err == "timeout" {
        NET_ERR_TIMEOUT
    } else if err == "url too long" || err == "empty url" {
        NET_ERR_BAD_URL
    } else {
        NET_ERR_HTTP
    }
}

fn write_bytes_to_file(path: &str, bytes: &[u8]) -> i32 {
    let Ok(handle) = crate::r::io::kfs::write_file_begin(path, bytes.len() as u64) else {
        return FS_ERR_IO;
    };
    if crate::r::io::kfs::write_file_chunk(handle, bytes).is_err() {
        let _ = crate::r::io::kfs::write_file_abort(handle);
        return FS_ERR_IO;
    }
    if crate::r::io::kfs::write_file_finish(handle).is_err() {
        return FS_ERR_IO;
    }
    0
}

async fn fetch_bytes(url: String, timeout_ms: u32, max_bytes: usize) -> Result<Vec<u8>, i32> {
    crate::surfer::html_shack::fetch_bytes_via_pool(url, timeout_ms as u64, max_bytes)
        .await
        .map(|fetch| fetch.bytes)
        .map_err(|err| fetch_error_to_code(err.as_str()))
}

async fn post_json_bytes(
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, i32> {
    let auth_header = bearer.map(|token| format!("Bearer {}", token));
    let headers_with_auth = [
        ("Accept", "application/json"),
        ("Authorization", auth_header.as_deref().unwrap_or_default()),
    ];
    let headers_without_auth = [("Accept", "application/json")];
    let headers = if auth_header.is_some() {
        &headers_with_auth[..]
    } else {
        &headers_without_auth[..]
    };

    crate::surfer::html_shack::post_bytes_via_pool(
        url,
        "application/json",
        headers,
        body_json.as_bytes(),
        timeout_ms as u64,
        max_bytes,
    )
    .await
    .map(|fetch| fetch.bytes)
    .map_err(|err| fetch_error_to_code(err.as_str()))
}

fn spawn_fetch_file(op_id: u32, url: String, path: String, timeout_ms: u32, max_bytes: usize) {
    crate::wait::spawn_local_detached(async move {
        let rc = match fetch_bytes(url, timeout_ms, max_bytes).await {
            Ok(bytes) => write_bytes_to_file(path.as_str(), bytes.as_slice()),
            Err(rc) => rc,
        };
        if let Some(slot) = CABI_NET_FETCH_RESULTS.lock().get_mut(&op_id) {
            *slot = Some(rc);
        }
    });
}

fn spawn_fetch_bytes(op_id: u32, url: String, timeout_ms: u32, max_bytes: usize) {
    crate::wait::spawn_local_detached(async move {
        let (rc, body) = match fetch_bytes(url, timeout_ms, max_bytes).await {
            Ok(bytes) => (0, bytes),
            Err(rc) => (rc, Vec::new()),
        };
        if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&op_id) {
            slot.rc = Some(rc);
            slot.body = body;
        }
    });
}

fn spawn_post_json_file(
    op_id: u32,
    url: String,
    path: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    crate::wait::spawn_local_detached(async move {
        let rc = match post_json_bytes(url, body_json, bearer, timeout_ms, max_bytes).await {
            Ok(bytes) => write_bytes_to_file(path.as_str(), bytes.as_slice()),
            Err(rc) => rc,
        };
        if let Some(slot) = CABI_NET_FETCH_RESULTS.lock().get_mut(&op_id) {
            *slot = Some(rc);
        }
    });
}

fn spawn_post_json_bytes(
    op_id: u32,
    url: String,
    body_json: String,
    bearer: Option<String>,
    timeout_ms: u32,
    max_bytes: usize,
) {
    crate::wait::spawn_local_detached(async move {
        let (rc, body) = match post_json_bytes(url, body_json, bearer, timeout_ms, max_bytes).await
        {
            Ok(bytes) => (0, bytes),
            Err(rc) => (rc, Vec::new()),
        };
        if let Some(slot) = CABI_NET_FETCH_BYTES_RESULTS.lock().get_mut(&op_id) {
            slot.rc = Some(rc);
            slot.body = body;
        }
    });
}

pub(crate) fn cabi_net_fetch_start_host(
    url_s: &str,
    path_s: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> u32 {
    if url_s.trim().is_empty() || path_s.trim().is_empty() {
        return 0;
    }
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);
    spawn_fetch_file(
        op_id,
        String::from(url_s),
        String::from(path_s),
        timeout_ms.max(1),
        max_bytes,
    );
    op_id
}

pub(crate) fn cabi_net_fetch_result_host(op_id: u32) -> i32 {
    match CABI_NET_FETCH_RESULTS.lock().get(&op_id) {
        Some(Some(rc)) => *rc,
        Some(None) | None => FS_ERR_NOT_FOUND,
    }
}

pub(crate) fn cabi_net_fetch_discard_host(op_id: u32) -> i32 {
    CABI_NET_FETCH_RESULTS.lock().remove(&op_id);
    0
}

pub(crate) fn cabi_net_fetch_bytes_start_host(
    url_s: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> u32 {
    if url_s.trim().is_empty() {
        return 0;
    }
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(op_id, CabiNetFetchBytesResult::default());
    spawn_fetch_bytes(op_id, String::from(url_s), timeout_ms.max(1), max_bytes);
    op_id
}

pub(crate) fn cabi_net_fetch_bytes_result_len_host(op_id: u32) -> isize {
    match CABI_NET_FETCH_BYTES_RESULTS.lock().get(&op_id) {
        Some(entry) => match entry.rc {
            Some(0) => entry.body.len() as isize,
            Some(rc) => rc as isize,
            None => FS_ERR_NOT_FOUND as isize,
        },
        None => FS_ERR_NOT_FOUND as isize,
    }
}

pub(crate) fn cabi_net_fetch_bytes_read_chunk_host(
    op_id: u32,
    offset: usize,
    out: &mut [u8],
) -> isize {
    let mut map = CABI_NET_FETCH_BYTES_RESULTS.lock();
    let Some(entry) = map.get(&op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    let Some(rc) = entry.rc else {
        return FS_ERR_NOT_FOUND as isize;
    };
    if rc != 0 {
        map.remove(&op_id);
        return rc as isize;
    }
    if offset > entry.body.len() {
        return FS_ERR_BAD_PARAM as isize;
    }
    let n = core::cmp::min(out.len(), entry.body.len().saturating_sub(offset));
    if n != 0 {
        out[..n].copy_from_slice(&entry.body[offset..offset + n]);
    }
    if offset.saturating_add(n) >= entry.body.len() {
        map.remove(&op_id);
    }
    n as isize
}

pub(crate) fn cabi_net_fetch_bytes_discard_host(op_id: u32) -> i32 {
    CABI_NET_FETCH_BYTES_RESULTS.lock().remove(&op_id);
    0
}

unsafe fn abi_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(ptr, len) }).ok()
}

unsafe fn optional_abi_string(ptr: *const u8, len: usize) -> Option<String> {
    if ptr.is_null() || len == 0 {
        None
    } else {
        unsafe { abi_str(ptr, len) }.map(String::from)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_start(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
) -> u32 {
    let Some(url) = (unsafe { abi_str(url_ptr, url_len) }) else {
        return 0;
    };
    let Some(path) = (unsafe { abi_str(path_ptr, path_len) }) else {
        return 0;
    };
    cabi_net_fetch_start_host(url, path, 45_000, 8 * 1024 * 1024)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_bytes_start(
    url_ptr: *const u8,
    url_len: usize,
) -> u32 {
    let Some(url) = (unsafe { abi_str(url_ptr, url_len) }) else {
        return 0;
    };
    cabi_net_fetch_bytes_start_host(url, 45_000, 8 * 1024 * 1024)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_prewarm_url_start(
    url_ptr: *const u8,
    url_len: usize,
) -> i32 {
    if unsafe { abi_str(url_ptr, url_len) }.is_some() {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_start(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
) -> u32 {
    unsafe {
        trueos_cabi_net_fetch_post_json_start_with_timeout(
            url_ptr, url_len, path_ptr, path_len, body_ptr, body_len, bearer_ptr, bearer_len,
            15_000,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_start_with_timeout(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
    timeout_ms: u32,
) -> u32 {
    let Some(url) = (unsafe { abi_str(url_ptr, url_len) }) else {
        return 0;
    };
    let Some(path) = (unsafe { abi_str(path_ptr, path_len) }) else {
        return 0;
    };
    let Some(body) = (unsafe { abi_str(body_ptr, body_len) }) else {
        return 0;
    };
    let bearer = unsafe { optional_abi_string(bearer_ptr, bearer_len) };
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);
    spawn_post_json_file(
        op_id,
        String::from(url),
        String::from(path),
        String::from(body),
        bearer,
        timeout_ms.max(1),
        4 * 1024 * 1024,
    );
    op_id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_bytes_start(
    url_ptr: *const u8,
    url_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
) -> u32 {
    unsafe {
        trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
            url_ptr, url_len, body_ptr, body_len, bearer_ptr, bearer_len, 15_000,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
    url_ptr: *const u8,
    url_len: usize,
    body_ptr: *const u8,
    body_len: usize,
    bearer_ptr: *const u8,
    bearer_len: usize,
    timeout_ms: u32,
) -> u32 {
    let Some(url) = (unsafe { abi_str(url_ptr, url_len) }) else {
        return 0;
    };
    let Some(body) = (unsafe { abi_str(body_ptr, body_len) }) else {
        return 0;
    };
    let bearer = unsafe { optional_abi_string(bearer_ptr, bearer_len) };
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_BYTES_RESULTS
        .lock()
        .insert(op_id, CabiNetFetchBytesResult::default());
    spawn_post_json_bytes(
        op_id,
        String::from(url),
        String::from(body),
        bearer,
        timeout_ms.max(1),
        4 * 1024 * 1024,
    );
    op_id
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_result(op_id: u32) -> i32 {
    cabi_net_fetch_result_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32 {
    cabi_net_fetch_discard_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_result_len(op_id: u32) -> isize {
    cabi_net_fetch_bytes_result_len_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_read(
    op_id: u32,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() || out_cap == 0 {
        return cabi_net_fetch_bytes_result_len_host(op_id);
    }
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    cabi_net_fetch_bytes_read_chunk_host(op_id, 0, out)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32 {
    cabi_net_fetch_bytes_discard_host(op_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }
    let start = monotonic_ms();
    loop {
        let rc = cabi_net_fetch_bytes_result_len_host(op_id);
        if rc != FS_ERR_NOT_FOUND as isize {
            return if rc < 0 { rc as i32 } else { 0 };
        }
        if timeout_ms == 0 || monotonic_ms().saturating_sub(start) >= timeout_ms {
            return FS_ERR_TIMEOUT;
        }
        crate::wait::spin_step();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }
    let start = monotonic_ms();
    loop {
        let rc = cabi_net_fetch_result_host(op_id);
        if rc != FS_ERR_NOT_FOUND {
            return rc;
        }
        if timeout_ms == 0 || monotonic_ms().saturating_sub(start) >= timeout_ms {
            return FS_ERR_TIMEOUT;
        }
        crate::wait::spin_step();
    }
}
