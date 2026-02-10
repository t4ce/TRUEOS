#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::{collections::VecDeque, string::String, vec::Vec};
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

use crate::trueos_shims::{
    trueos_cabi_fs_read_file, trueos_cabi_fs_write_abort, trueos_cabi_fs_write_begin,
    trueos_cabi_fs_write_chunk, trueos_cabi_fs_write_finish,
    trueos_cabi_net_fetch_to_file,
    trueos_cabi_poll_once,
};

include!("../../../src/surface/cabi_codes.rs");

const ASYNC_FS_MAX_QUEUE: usize = 64;
const ASYNC_FS_WRITE_CHUNK: usize = 256 * 1024;

static ASYNC_FS_SEQ: AtomicU32 = AtomicU32::new(1);
static SERVICE_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
enum AsyncFsRequest {
    ReadFile { id: u32, path: String },
    WriteBegin { id: u32, path: String, total_len: u64 },
    WriteChunk { id: u32, data: Vec<u8> },
    WriteFinish { id: u32 },
    NetFetchToFile { id: u32, url: String, path: String },
}

#[derive(Clone, Debug)]
struct AsyncFsCompletion {
    id: u32,
    rc: i32,
    data: Vec<u8>,
}

static ASYNC_FS_REQS: Mutex<VecDeque<AsyncFsRequest>> = Mutex::new(VecDeque::new());
static ASYNC_FS_DONE: Mutex<VecDeque<u32>> = Mutex::new(VecDeque::new());
static ASYNC_FS_RESULTS: Mutex<BTreeMap<u32, AsyncFsCompletion>> = Mutex::new(BTreeMap::new());
static ASYNC_FS_WRITES: Mutex<BTreeMap<u32, u32>> = Mutex::new(BTreeMap::new());

#[inline]
fn next_async_fs_id() -> u32 {
    ASYNC_FS_SEQ.fetch_add(1, Ordering::Relaxed)
}

fn push_async_fs_req(req: AsyncFsRequest) -> Result<(), i32> {
    let mut q = ASYNC_FS_REQS.lock();
    if q.len() >= ASYNC_FS_MAX_QUEUE {
        return Err(FS_ERR_NO_SPACE);
    }
    q.push_back(req);
    Ok(())
}

fn push_async_fs_req_wait(req: AsyncFsRequest) {
    let mut req = Some(req);
    loop {
        let pushed = {
            let mut q = ASYNC_FS_REQS.lock();
            if q.len() < ASYNC_FS_MAX_QUEUE {
                q.push_back(req.take().expect("request present"));
                true
            } else {
                false
            }
        };
        if pushed {
            return;
        }
        unsafe { trueos_cabi_poll_once() };
    }
}

fn take_async_fs_req() -> Option<AsyncFsRequest> {
    let mut q = ASYNC_FS_REQS.lock();
    q.pop_front()
}

fn push_async_fs_completion(done: AsyncFsCompletion) {
    let id = done.id;
    let inserted = {
        let mut res = ASYNC_FS_RESULTS.lock();
        if res.contains_key(&id) {
            false
        } else {
            res.insert(id, done);
            true
        }
    };
    if inserted {
        ASYNC_FS_DONE.lock().push_back(id);
    }
}

fn completion_rc_len(id: u32) -> Option<(i32, usize)> {
    let res = ASYNC_FS_RESULTS.lock();
    res.get(&id).map(|c| (c.rc, c.data.len()))
}

fn remove_async_fs_completion(id: u32) -> Option<AsyncFsCompletion> {
    let mut res = ASYNC_FS_RESULTS.lock();
    res.remove(&id)
}

fn write_handle_for(id: u32) -> Option<u32> {
    ASYNC_FS_WRITES.lock().get(&id).copied()
}

fn set_write_handle(id: u32, handle: u32) {
    ASYNC_FS_WRITES.lock().insert(id, handle);
}

fn take_write_handle(id: u32) -> Option<u32> {
    ASYNC_FS_WRITES.lock().remove(&id)
}

fn has_result(id: u32) -> bool {
    ASYNC_FS_RESULTS.lock().contains_key(&id)
}

pub fn has_completion_result(op_id: u32) -> bool {
    has_result(op_id)
}

fn remove_queued_reqs(id: u32) {
    let mut q = ASYNC_FS_REQS.lock();
    q.retain(|req| match req {
        AsyncFsRequest::ReadFile { id: rid, .. }
        | AsyncFsRequest::WriteBegin { id: rid, .. }
        | AsyncFsRequest::WriteChunk { id: rid, .. }
        | AsyncFsRequest::WriteFinish { id: rid }
        | AsyncFsRequest::NetFetchToFile { id: rid, .. } => *rid != id,
    });
}

fn remove_done_id(id: u32) {
    let mut done = ASYNC_FS_DONE.lock();
    if let Some(pos) = done.iter().position(|x| *x == id) {
        done.remove(pos);
    }
}

fn has_completion() -> bool {
    !ASYNC_FS_DONE.lock().is_empty()
}

fn read_file_via_cabi(path: &str) -> Result<Vec<u8>, i32> {
    let len = unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    let len = len as usize;
    let mut buf = Vec::with_capacity(len);
    buf.resize(len, 0);
    let got = unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), buf.as_mut_ptr(), len) };
    if got < 0 {
        return Err(got as i32);
    }
    buf.truncate(got as usize);
    Ok(buf)
}

fn write_begin_via_cabi(path: &str, total_len: u64) -> Result<u32, i32> {
    let mut handle = 0u32;
    let rc = unsafe {
        trueos_cabi_fs_write_begin(path.as_ptr(), path.len(), total_len, &mut handle as *mut u32)
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(handle)
}

fn write_chunk_via_cabi(handle: u32, chunk: &[u8]) -> Result<(), i32> {
    let rc = unsafe { trueos_cabi_fs_write_chunk(handle, chunk.as_ptr(), chunk.len()) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

fn write_finish_via_cabi(handle: u32) -> Result<(), i32> {
    let rc = unsafe { trueos_cabi_fs_write_finish(handle) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

fn net_fetch_to_file_via_cabi(url: &str, path: &str) -> Result<(), i32> {
    let rc = unsafe {
        trueos_cabi_net_fetch_to_file(url.as_ptr(), url.len(), path.as_ptr(), path.len())
    };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

pub fn ensure_service_started(spawner: &Spawner) {
    if SERVICE_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let _ = spawner.spawn(async_fs_service_task());
}

#[embassy_executor::task]
pub async fn async_fs_service_task() {
    loop {
        let mut processed = 0usize;
        loop {
            let Some(req) = take_async_fs_req() else {
                break;
            };

            match req {
                AsyncFsRequest::ReadFile { id, path } => match read_file_via_cabi(path.as_str()) {
                    Ok(bytes) => push_async_fs_completion(AsyncFsCompletion {
                        id,
                        rc: 0,
                        data: bytes,
                    }),
                    Err(rc) => push_async_fs_completion(AsyncFsCompletion {
                        id,
                        rc,
                        data: Vec::new(),
                    }),
                },
                AsyncFsRequest::WriteBegin { id, path, total_len } => {
                    if has_result(id) {
                        processed = processed.saturating_add(1);
                        if processed >= ASYNC_FS_MAX_QUEUE {
                            break;
                        }
                        continue;
                    }
                    match write_begin_via_cabi(path.as_str(), total_len) {
                        Ok(handle) => set_write_handle(id, handle),
                        Err(rc) => push_async_fs_completion(AsyncFsCompletion {
                            id,
                            rc,
                            data: Vec::new(),
                        }),
                    }
                }
                AsyncFsRequest::WriteChunk { id, data } => {
                    if has_result(id) {
                        processed = processed.saturating_add(1);
                        if processed >= ASYNC_FS_MAX_QUEUE {
                            break;
                        }
                        continue;
                    }
                    let Some(handle) = write_handle_for(id) else {
                        processed = processed.saturating_add(1);
                        if processed >= ASYNC_FS_MAX_QUEUE {
                            break;
                        }
                        continue;
                    };
                    if let Err(rc) = write_chunk_via_cabi(handle, data.as_slice()) {
                        let _ = unsafe { trueos_cabi_fs_write_abort(handle) };
                        let _ = take_write_handle(id);
                        push_async_fs_completion(AsyncFsCompletion {
                            id,
                            rc,
                            data: Vec::new(),
                        });
                    }
                }
                AsyncFsRequest::WriteFinish { id } => {
                    if has_result(id) {
                        processed = processed.saturating_add(1);
                        if processed >= ASYNC_FS_MAX_QUEUE {
                            break;
                        }
                        continue;
                    }
                    let Some(handle) = take_write_handle(id) else {
                        push_async_fs_completion(AsyncFsCompletion {
                            id,
                            rc: FS_ERR_BAD_PARAM,
                            data: Vec::new(),
                        });
                        processed = processed.saturating_add(1);
                        if processed >= ASYNC_FS_MAX_QUEUE {
                            break;
                        }
                        continue;
                    };
                    match write_finish_via_cabi(handle) {
                        Ok(()) => push_async_fs_completion(AsyncFsCompletion {
                            id,
                            rc: 0,
                            data: Vec::new(),
                        }),
                        Err(rc) => {
                            let _ = unsafe { trueos_cabi_fs_write_abort(handle) };
                            push_async_fs_completion(AsyncFsCompletion {
                                id,
                                rc,
                                data: Vec::new(),
                            });
                        }
                    }
                }
                AsyncFsRequest::NetFetchToFile { id, url, path } => {
                    match net_fetch_to_file_via_cabi(url.as_str(), path.as_str()) {
                        Ok(()) => push_async_fs_completion(AsyncFsCompletion {
                            id,
                            rc: 0,
                            data: Vec::new(),
                        }),
                        Err(rc) => push_async_fs_completion(AsyncFsCompletion {
                            id,
                            rc,
                            data: Vec::new(),
                        }),
                    }
                }
            }

            processed = processed.saturating_add(1);
            if processed >= ASYNC_FS_MAX_QUEUE {
                break;
            }
        }

        if processed == 0 {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }
}

pub fn start_net_fetch_to_file(url: &[u8], path: &[u8]) -> Result<u32, i32> {
    if url.is_empty() || path.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let Ok(url_str) = core::str::from_utf8(url) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let Ok(path_str) = core::str::from_utf8(path) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let id = next_async_fs_id();
    let req = AsyncFsRequest::NetFetchToFile {
        id,
        url: url_str.to_string(),
        path: path_str.to_string(),
    };
    push_async_fs_req(req)?;
    Ok(id)
}

pub fn start_read_file(path: &[u8]) -> Result<u32, i32> {
    if path.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let Ok(path_str) = core::str::from_utf8(path) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let id = next_async_fs_id();
    let req = AsyncFsRequest::ReadFile {
        id,
        path: path_str.to_string(),
    };
    push_async_fs_req(req)?;
    Ok(id)
}

pub fn start_write_file(path: &[u8], data: &[u8]) -> Result<u32, i32> {
    if path.is_empty() {
        return Err(FS_ERR_BAD_PARAM);
    }
    if path.len() > QJS_ASYNC_FS_MAX_PATH {
        return Err(FS_ERR_TOO_LARGE);
    }
    let Ok(path_str) = core::str::from_utf8(path) else {
        return Err(FS_ERR_BAD_UTF8);
    };
    let id = next_async_fs_id();
    let path = path_str.to_string();
    push_async_fs_req_wait(AsyncFsRequest::WriteBegin {
        id,
        path,
        total_len: data.len() as u64,
    });
    for chunk in data.chunks(ASYNC_FS_WRITE_CHUNK) {
        push_async_fs_req_wait(AsyncFsRequest::WriteChunk {
            id,
            data: chunk.to_vec(),
        });
    }
    let req = AsyncFsRequest::WriteFinish {
        id,
    };
    push_async_fs_req_wait(req);
    Ok(id)
}

pub fn poll_completed(out_id: *mut u32) -> i32 {
    if out_id.is_null() {
        return 0;
    }
    let mut done = ASYNC_FS_DONE.lock();
    let Some(id) = done.pop_front() else {
        return 0;
    };
    unsafe { *out_id = id };
    1
}

pub fn result_len(op_id: u32) -> isize {
    let Some((rc, len)) = completion_rc_len(op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    if rc != 0 {
        return rc as isize;
    }
    len as isize
}

pub fn read_result(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize {
    let Some((rc, len)) = completion_rc_len(op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    if rc != 0 {
        remove_async_fs_completion(op_id);
        return rc as isize;
    }

    if out_ptr.is_null() || out_cap == 0 {
        return len as isize;
    }
    if len > out_cap {
        return FS_ERR_NO_SPACE as isize;
    }

    let Some(c) = remove_async_fs_completion(op_id) else {
        return FS_ERR_NOT_FOUND as isize;
    };
    unsafe { core::ptr::copy_nonoverlapping(c.data.as_ptr(), out_ptr, c.data.len()) };
    let n = c.data.len() as isize;
    n
}

pub fn discard(op_id: u32) -> i32 {
    remove_done_id(op_id);
    remove_async_fs_completion(op_id);
    remove_queued_reqs(op_id);
    if let Some(handle) = take_write_handle(op_id) {
        let _ = unsafe { trueos_cabi_fs_write_abort(handle) };
    }
    0
}

pub async fn wait_for_completion(timeout_ms: u64) -> bool {
    if timeout_ms == 0 {
        return has_completion();
    }
    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
    loop {
        if has_completion() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

pub fn wait_for_completion_blocking(timeout_ms: u64) -> bool {
    let hz = embassy_time_driver::TICK_HZ as u64;
    let max_ticks = if timeout_ms == 0 || hz == 0 {
        0
    } else {
        (timeout_ms.saturating_mul(hz) + 999) / 1000
    };
    let deadline = if max_ticks == 0 {
        0
    } else {
        embassy_time_driver::now().saturating_add(max_ticks)
    };

    loop {
        if has_completion() {
            return true;
        }
        if max_ticks != 0 && embassy_time_driver::now() >= deadline {
            return false;
        }
        unsafe { trueos_cabi_poll_once() };
    }
}
