#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::vec::Vec;

use spin::Mutex;

use crate as qjs;

use crate::async_fs;
use crate::trueos_shims::trueos_cabi_fs_read_file;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpKind {
    ReadText,
    ReadBytes,
    WriteText,
    /// Kernel net fetch (URL -> file) then read file and resolve with text.
    NetFetchText,
    /// Kernel net fetch (URL -> file) then read file and resolve with bytes.
    NetFetchBytes,
}

#[derive(Clone)]
struct PendingOp {
    ctx_owner: *mut qjs::JSContext,
    op_id: u32,
    kind: OpKind,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
    /// Optional auxiliary payload owned by this async op.
    /// For NetFetchText this is the destination file path (UTF-8 bytes).
    aux: Vec<u8>,
}

// NOTE: This satisfies `static` requirements for `spin::Mutex`.
// QuickJS execution is expected to remain within a single owning task.
unsafe impl Send for PendingOp {}

static PENDING: Mutex<Vec<PendingOp>> = Mutex::new(Vec::new());

#[derive(Clone, Copy)]
struct CompletedOp {
    ctx_owner: *mut qjs::JSContext,
    op_id: u32,
}

// QuickJS execution is expected to remain within a single owning task.
unsafe impl Send for CompletedOp {}

static COMPLETED: Mutex<Vec<CompletedOp>> = Mutex::new(Vec::new());

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

pub unsafe fn start_read_file(path: &[u8]) -> Result<u32, i32> {
    async_fs::start_read_file(path)
}

pub unsafe fn start_write_file(path: &[u8], data: &[u8]) -> Result<u32, i32> {
    async_fs::start_write_file(path, data)
}

pub unsafe fn start_net_fetch_to_file(url: &[u8], path: &[u8]) -> Result<u32, i32> {
    async_fs::start_net_fetch_to_file(url, path)
}

pub unsafe fn start_net_post_json_to_file(
    url: &[u8],
    path: &[u8],
    body_json: &[u8],
    bearer: Option<&[u8]>,
) -> Result<u32, i32> {
    async_fs::start_net_post_json_to_file(url, path, body_json, bearer)
}

pub unsafe fn register_promise(
    ctx: *mut qjs::JSContext,
    op_id: u32,
    kind: OpKind,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
    aux: Vec<u8>,
) {
    let op = PendingOp {
        ctx_owner: ctx,
        op_id,
        kind,
        resolve: unsafe { qjs::js_dup_value(ctx, resolve) },
        reject: unsafe { qjs::js_dup_value(ctx, reject) },
        aux,
    };
    PENDING.lock().push(op);
}

pub unsafe fn has_pending(ctx: *mut qjs::JSContext) -> bool {
    PENDING.lock().iter().any(|p| p.ctx_owner == ctx)
}

unsafe fn take_pending(ctx: *mut qjs::JSContext, op_id: u32) -> Option<PendingOp> {
    let mut pending = PENDING.lock();
    let pos = pending
        .iter()
        .position(|p| p.ctx_owner == ctx && p.op_id == op_id)?;
    Some(pending.remove(pos))
}

fn find_pending_owner(op_id: u32) -> Option<*mut qjs::JSContext> {
    PENDING
        .lock()
        .iter()
        .find(|p| p.op_id == op_id)
        .map(|p| p.ctx_owner)
}

fn push_completed(ctx: *mut qjs::JSContext, op_id: u32) {
    COMPLETED.lock().push(CompletedOp {
        ctx_owner: ctx,
        op_id,
    });
}

fn take_completed_for_ctx(ctx: *mut qjs::JSContext) -> Option<u32> {
    let mut completed = COMPLETED.lock();
    let pos = completed.iter().position(|c| c.ctx_owner == ctx)?;
    Some(completed.remove(pos).op_id)
}

unsafe fn reject_with_code(ctx: *mut qjs::JSContext, op: &PendingOp, code: i32) {
    let arg = js_int32(code);
    let _ = qjs::JS_Call(
        ctx,
        op.reject,
        qjs::JSValue::undefined(),
        1,
        &arg as *const qjs::JSValue,
    );
}

unsafe fn resolve_with_value(ctx: *mut qjs::JSContext, op: &PendingOp, val: qjs::JSValue) {
    let _ = qjs::JS_Call(
        ctx,
        op.resolve,
        qjs::JSValue::undefined(),
        1,
        &val as *const qjs::JSValue,
    );
    qjs::js_free_value(ctx, val);
}

unsafe fn resolve_undefined(ctx: *mut qjs::JSContext, op: &PendingOp) {
    let val = qjs::JSValue::undefined();
    let _ = qjs::JS_Call(
        ctx,
        op.resolve,
        qjs::JSValue::undefined(),
        1,
        &val as *const qjs::JSValue,
    );
}

fn read_file_via_cabi(path: &[u8]) -> Result<Vec<u8>, i32> {
    let len =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    let len = len as usize;
    let mut buf: Vec<u8> = Vec::with_capacity(len);
    buf.resize(len, 0);
    let got =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), buf.as_mut_ptr(), buf.len()) };
    if got < 0 {
        return Err(got as i32);
    }
    buf.truncate(got as usize);
    Ok(buf)
}

unsafe fn pump_net_fetch_text(ctx: *mut qjs::JSContext) -> bool {
    // Net ops don't show up in async_fs::poll_completed(), so we must poll their result here.
    let mut progress = false;

    // Collect op_ids to process to avoid holding the mutex while doing work.
    let op_ids: Vec<u32> = {
        let pending = PENDING.lock();
        pending
            .iter()
            .filter(|p| p.ctx_owner == ctx && p.kind == OpKind::NetFetchText)
            .map(|p| p.op_id)
            .collect()
    };

    for op_id in op_ids {
        // Still in flight?
        let rc_or_done = async_fs::result_len(op_id);
        if rc_or_done == async_fs::FS_ERR_NOT_FOUND as isize {
            continue;
        }

        let Some(op) = take_pending(ctx, op_id) else {
            let _ = async_fs::discard(op_id);
            continue;
        };

        progress = true;

        if rc_or_done < 0 {
            let _ = async_fs::discard(op_id);
            reject_with_code(ctx, &op, rc_or_done as i32);
            qjs::js_free_value(ctx, op.resolve);
            qjs::js_free_value(ctx, op.reject);
            continue;
        }

        // Consume the net op result (also discards the kernel op id).
        let _ = async_fs::read_result(op_id, core::ptr::null_mut(), 0);

        // Read downloaded bytes and resolve as a JS string.
        match read_file_via_cabi(op.aux.as_slice()) {
            Ok(buf) => {
                let s =
                    qjs::JS_NewStringLen(ctx, buf.as_ptr() as *const core::ffi::c_char, buf.len());
                resolve_with_value(ctx, &op, s);
            }
            Err(code) => {
                reject_with_code(ctx, &op, code);
            }
        }

        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
    }

    progress
}

unsafe fn pump_net_fetch_bytes(ctx: *mut qjs::JSContext) -> bool {
    let mut progress = false;

    let op_ids: Vec<u32> = {
        let pending = PENDING.lock();
        pending
            .iter()
            .filter(|p| p.ctx_owner == ctx && p.kind == OpKind::NetFetchBytes)
            .map(|p| p.op_id)
            .collect()
    };

    for op_id in op_ids {
        let rc_or_done = async_fs::result_len(op_id);
        if rc_or_done == async_fs::FS_ERR_NOT_FOUND as isize {
            continue;
        }

        let Some(op) = take_pending(ctx, op_id) else {
            let _ = async_fs::discard(op_id);
            continue;
        };

        progress = true;

        if rc_or_done < 0 {
            let _ = async_fs::discard(op_id);
            reject_with_code(ctx, &op, rc_or_done as i32);
            qjs::js_free_value(ctx, op.resolve);
            qjs::js_free_value(ctx, op.reject);
            continue;
        }

        let _ = async_fs::read_result(op_id, core::ptr::null_mut(), 0);

        match read_file_via_cabi(op.aux.as_slice()) {
            Ok(buf) => {
                let ab = qjs::JS_NewArrayBufferCopy(ctx, buf.as_ptr(), buf.len());
                resolve_with_value(ctx, &op, ab);
            }
            Err(code) => {
                reject_with_code(ctx, &op, code);
            }
        }

        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
    }

    progress
}

fn read_completion_bytes(done_id: u32, len: usize) -> Result<Vec<u8>, i32> {
    let mut buf: Vec<u8> = Vec::with_capacity(len);
    buf.resize(len, 0);
    let got = async_fs::read_result(done_id, buf.as_mut_ptr(), buf.len());
    if got < 0 {
        return Err(got as i32);
    }
    buf.truncate(got as usize);
    Ok(buf)
}

/// Pump completed kernel async FS operations into JS Promises.
///
/// Call this from the thread/task that owns the QuickJS runtime.
///
/// Returns `true` if it made progress (handled at least one completion).
pub unsafe fn pump(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }

    let mut progress = false;

    progress |= pump_net_fetch_text(ctx);
    progress |= pump_net_fetch_bytes(ctx);

    loop {
        let done_id = match take_completed_for_ctx(ctx) {
            Some(id) => {
                progress = true;
                id
            }
            None => {
                let mut id: u32 = 0;
                let has = async_fs::poll_completed(&mut id as *mut u32);
                if has <= 0 {
                    break;
                }
                progress = true;

                let owner = find_pending_owner(id);
                if let Some(owner) = owner {
                    if owner != ctx {
                        push_completed(owner, id);
                        continue;
                    }
                } else {
                    let _ = async_fs::discard(id);
                    continue;
                }
                id
            }
        };

        let Some(op) = (unsafe { take_pending(ctx, done_id) }) else {
            // Unknown/expired op id: drop completion to avoid leaks.
            let _ = async_fs::discard(done_id);
            continue;
        };

        let len = async_fs::result_len(done_id);
        if len < 0 {
            unsafe { reject_with_code(ctx, &op, len as i32) };
            unsafe { qjs::js_free_value(ctx, op.resolve) };
            unsafe { qjs::js_free_value(ctx, op.reject) };
            continue;
        }

        match op.kind {
            OpKind::WriteText => {
                // Consume completion payload (if any) and resolve.
                let _ = async_fs::read_result(done_id, core::ptr::null_mut(), 0);
                unsafe { resolve_undefined(ctx, &op) };
            }
            OpKind::ReadBytes => {
                let n = len as usize;
                match read_completion_bytes(done_id, n) {
                    Ok(buf) => {
                        let ab =
                            unsafe { qjs::JS_NewArrayBufferCopy(ctx, buf.as_ptr(), buf.len()) };
                        unsafe { resolve_with_value(ctx, &op, ab) };
                    }
                    Err(code) => unsafe { reject_with_code(ctx, &op, code) },
                }
            }
            OpKind::ReadText => {
                let n = len as usize;
                match read_completion_bytes(done_id, n) {
                    Ok(buf) => {
                        let s = unsafe {
                            qjs::JS_NewStringLen(
                                ctx,
                                buf.as_ptr() as *const core::ffi::c_char,
                                buf.len(),
                            )
                        };
                        unsafe { resolve_with_value(ctx, &op, s) };
                    }
                    Err(code) => unsafe { reject_with_code(ctx, &op, code) },
                }
            }
            OpKind::NetFetchText => {
                // Net fetches are handled by `pump_net_fetch_text`.
                // If one ever arrives here, discard and reject to avoid leaking.
                let _ = async_fs::discard(done_id);
                reject_with_code(ctx, &op, async_fs::FS_ERR_BAD_PARAM);
            }
            OpKind::NetFetchBytes => {
                // Net fetches are handled by `pump_net_fetch_bytes`.
                // If one ever arrives here, discard and reject to avoid leaking.
                let _ = async_fs::discard(done_id);
                reject_with_code(ctx, &op, async_fs::FS_ERR_BAD_PARAM);
            }
        }

        unsafe { qjs::js_free_value(ctx, op.resolve) };
        unsafe { qjs::js_free_value(ctx, op.reject) };
    }

    progress
}

/// Create a Promise (and its resolve/reject functions).
///
/// Returns (promise, resolve, reject).
pub unsafe fn new_promise(ctx: *mut qjs::JSContext) -> (qjs::JSValue, qjs::JSValue, qjs::JSValue) {
    let mut resolving: [qjs::JSValue; 2] = [qjs::JSValue::undefined(), qjs::JSValue::undefined()];
    let promise = unsafe { qjs::JS_NewPromiseCapability(ctx, resolving.as_mut_ptr()) };
    (promise, resolving[0], resolving[1])
}

pub unsafe fn drain_all_for_context(ctx: *mut qjs::JSContext) {
    // Best-effort cleanup: reject all pending promises for this context.
    let mut pending = PENDING.lock();
    let mut i = 0usize;
    while i < pending.len() {
        if pending[i].ctx_owner != ctx {
            i += 1;
            continue;
        }
        let op = pending.remove(i);
        reject_with_code(ctx, &op, -2);
        qjs::js_free_value(ctx, op.resolve);
        qjs::js_free_value(ctx, op.reject);
        let _ = async_fs::discard(op.op_id);
    }

    let mut discard_ids: Vec<u32> = Vec::new();
    {
        let mut completed = COMPLETED.lock();
        let mut j = 0usize;
        while j < completed.len() {
            if completed[j].ctx_owner == ctx {
                discard_ids.push(completed.remove(j).op_id);
            } else {
                j += 1;
            }
        }
    }
    for op_id in discard_ids {
        let _ = async_fs::discard(op_id);
    }
}

pub async fn wait_for_completion(timeout_ms: u64) -> bool {
    async_fs::wait_for_completion(timeout_ms).await
}
