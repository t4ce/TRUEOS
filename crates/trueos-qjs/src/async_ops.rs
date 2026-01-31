#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::vec::Vec;

use spin::Mutex;

use crate as qjs;

extern "C" {
    fn trueos_cabi_async_fs_read_file_start(path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_async_fs_write_file_start(
        path_ptr: *const u8,
        path_len: usize,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;

    fn trueos_cabi_async_fs_poll_completed(out_id: *mut u32) -> i32;
    fn trueos_cabi_async_fs_result_len(op_id: u32) -> isize;
    fn trueos_cabi_async_fs_read_result(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_cabi_async_fs_discard(op_id: u32) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpKind {
    ReadText,
    ReadBytes,
    WriteText,
}

#[derive(Clone)]
struct PendingOp {
    ctx_owner: *mut qjs::JSContext,
    op_id: u32,
    kind: OpKind,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
}

// NOTE: This satisfies `static` requirements for `spin::Mutex`.
// QuickJS execution is expected to remain within a single owning task.
unsafe impl Send for PendingOp {}

static PENDING: Mutex<Vec<PendingOp>> = Mutex::new(Vec::new());

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

pub unsafe fn start_read_file(path: &[u8]) -> Result<u32, i32> {
    let rc = unsafe { trueos_cabi_async_fs_read_file_start(path.as_ptr(), path.len()) };
    if rc < 0 {
        Err(rc)
    } else {
        Ok(rc as u32)
    }
}

pub unsafe fn start_write_file(path: &[u8], data: &[u8]) -> Result<u32, i32> {
    let rc = unsafe {
        trueos_cabi_async_fs_write_file_start(path.as_ptr(), path.len(), data.as_ptr(), data.len())
    };
    if rc < 0 {
        Err(rc)
    } else {
        Ok(rc as u32)
    }
}

pub unsafe fn register_promise(
    ctx: *mut qjs::JSContext,
    op_id: u32,
    kind: OpKind,
    resolve: qjs::JSValue,
    reject: qjs::JSValue,
) {
    let op = PendingOp {
        ctx_owner: ctx,
        op_id,
        kind,
        resolve: unsafe { qjs::js_dup_value(ctx, resolve) },
        reject: unsafe { qjs::js_dup_value(ctx, reject) },
    };
    PENDING.lock().push(op);
}

pub unsafe fn has_pending(ctx: *mut qjs::JSContext) -> bool {
    PENDING.lock().iter().any(|p| p.ctx_owner == ctx)
}

unsafe fn take_pending(ctx: *mut qjs::JSContext, op_id: u32) -> Option<PendingOp> {
    let mut pending = PENDING.lock();
    let pos = pending.iter().position(|p| p.ctx_owner == ctx && p.op_id == op_id)?;
    Some(pending.remove(pos))
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

    loop {
        let mut done_id: u32 = 0;
        let has = unsafe { trueos_cabi_async_fs_poll_completed(&mut done_id as *mut u32) };
        if has <= 0 {
            break;
        }
        progress = true;

		let Some(op) = (unsafe { take_pending(ctx, done_id) }) else {
            // Unknown/expired op id: drop completion to avoid leaks.
            let _ = unsafe { trueos_cabi_async_fs_discard(done_id) };
            continue;
        };

        let len = unsafe { trueos_cabi_async_fs_result_len(done_id) };
        if len < 0 {
            unsafe { reject_with_code(ctx, &op, len as i32) };
            unsafe { qjs::js_free_value(ctx, op.resolve) };
            unsafe { qjs::js_free_value(ctx, op.reject) };
            continue;
        }

        match op.kind {
            OpKind::WriteText => {
                // Consume completion payload (if any) and resolve.
                let _ = unsafe { trueos_cabi_async_fs_read_result(done_id, core::ptr::null_mut(), 0) };
                unsafe { resolve_undefined(ctx, &op) };
            }
            OpKind::ReadBytes => {
                let n = len as usize;
                let mut buf: Vec<u8> = Vec::with_capacity(n);
                buf.resize(n, 0);

                let got = unsafe { trueos_cabi_async_fs_read_result(done_id, buf.as_mut_ptr(), buf.len()) };
                if got < 0 {
                    unsafe { reject_with_code(ctx, &op, got as i32) };
                } else {
                    let ab = unsafe { qjs::JS_NewArrayBufferCopy(ctx, buf.as_ptr(), got as usize) };
                    unsafe { resolve_with_value(ctx, &op, ab) };
                }
            }
            OpKind::ReadText => {
                let n = len as usize;
                let mut buf: Vec<u8> = Vec::with_capacity(n);
                buf.resize(n, 0);

                let got = unsafe { trueos_cabi_async_fs_read_result(done_id, buf.as_mut_ptr(), buf.len()) };
                if got < 0 {
                    unsafe { reject_with_code(ctx, &op, got as i32) };
                } else {
                    let s = unsafe {
                        qjs::JS_NewStringLen(ctx, buf.as_ptr() as *const core::ffi::c_char, got as usize)
                    };
                    unsafe { resolve_with_value(ctx, &op, s) };
                }
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
        let _ = trueos_cabi_async_fs_discard(op.op_id);
    }
}
