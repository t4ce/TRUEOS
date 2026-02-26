#![cfg(feature = "trueos")]

use core::ffi::{CStr, c_char, c_int};
use core::sync::atomic::{AtomicU32, Ordering};

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static PROMISE_REJECTION_LOGS: AtomicU32 = AtomicU32::new(0);

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

#[inline]
fn log_nl() {
    log_bytes(b"\n");
}

#[inline]
fn log_i64_dec(v: i64) {
    if v == 0 {
        log_str("0");
        return;
    }
    let neg = v < 0;
    let mut n = if neg { v.unsigned_abs() } else { v as u64 };
    let mut buf = [0u8; 24];
    let mut i = buf.len();
    while n != 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if neg {
        log_str("-");
    }
    log_bytes(&buf[i..]);
}

#[inline]
unsafe fn try_log_via_global_string(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return false;
    }
    let string_fn = qjs::JS_GetPropertyStr(ctx, global, c"String".as_ptr());
    qjs::js_free_value(ctx, global);
    if string_fn.is_exception() || string_fn.tag == qjs::JS_TAG_UNDEFINED {
        qjs::js_free_value(ctx, string_fn);
        return false;
    }

    let mut arg = v;
    // Keep argument alive while calling into JS.
    arg = qjs::js_dup_value(ctx, arg);
    let out = qjs::JS_Call(
        ctx,
        string_fn,
        qjs::JSValue::undefined(),
        1,
        &arg as *const qjs::JSValueConst,
    );
    qjs::js_free_value(ctx, arg);
    qjs::js_free_value(ctx, string_fn);
    if out.is_exception() {
        let exc = qjs::JS_GetException(ctx);
        qjs::js_free_value(ctx, exc);
        return false;
    }
    let cstr = qjs::js_to_cstring(ctx, out);
    if cstr.is_null() {
        qjs::js_free_value(ctx, out);
        return false;
    }
    let bytes = CStr::from_ptr(cstr).to_bytes();
    log_bytes(bytes);
    qjs::JS_FreeCString(ctx, cstr);
    qjs::js_free_value(ctx, out);
    true
}

#[inline]
fn log_value(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) {
    let cstr = unsafe { qjs::js_to_cstring(ctx, v) };
    if cstr.is_null() {
        if unsafe { try_log_via_global_string(ctx, v) } {
            return;
        }
        log_str("<toString failed>");
        return;
    }
    let bytes = unsafe { CStr::from_ptr(cstr).to_bytes() };
    log_bytes(bytes);
    unsafe { qjs::JS_FreeCString(ctx, cstr) };
}

#[inline]
unsafe fn log_named_prop(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
    prefix: &str,
) {
    let v = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    if !v.is_exception() && v.tag != qjs::JS_TAG_UNDEFINED {
        log_str(prefix);
        log_value(ctx, v);
        log_nl();
    }
    qjs::js_free_value(ctx, v);
}

pub unsafe fn log_exception_value(ctx: *mut qjs::JSContext, label: &str, value: qjs::JSValueConst) {
    if ctx.is_null() {
        return;
    }
    log_str("quickjs: ");
    log_str(label);
    log_str(": ");
    log_value(ctx, value);
    log_nl();
    log_str("quickjs: tag: ");
    log_i64_dec(value.tag);
    log_nl();

    log_named_prop(ctx, value, b"name\0", "quickjs: name: ");
    log_named_prop(ctx, value, b"message\0", "quickjs: message: ");
    log_named_prop(ctx, value, b"stack\0", "quickjs: stack: ");
}

pub unsafe fn dump_last_exception(ctx: *mut qjs::JSContext, label: &str) {
    if ctx.is_null() {
        return;
    }
    let exc = qjs::JS_GetException(ctx);
    log_exception_value(ctx, label, exc);
    qjs::js_free_value(ctx, exc);
}

unsafe extern "C" fn host_promise_rejection_tracker(
    ctx: *mut qjs::JSContext,
    _promise: qjs::JSValueConst,
    reason: qjs::JSValueConst,
    is_handled: c_int,
    _opaque: *mut core::ffi::c_void,
) {
    if ctx.is_null() {
        return;
    }
    // Only log unhandled transitions; handled notifications are too noisy.
    if is_handled != 0 {
        return;
    }
    if PROMISE_REJECTION_LOGS.fetch_add(1, Ordering::Relaxed) < 64 {
        log_exception_value(ctx, "unhandled-promise-rejection", reason);
    }
}

unsafe extern "C" fn js_report_exception(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if ctx.is_null() || argv.is_null() || argc <= 0 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut label = "js-hook-exception";
    let label_cstr = qjs::js_to_cstring(ctx, args[0]);
    if !label_cstr.is_null() {
        if let Ok(s) = core::str::from_utf8(CStr::from_ptr(label_cstr).to_bytes()) {
            label = s;
        }
        qjs::JS_FreeCString(ctx, label_cstr);
    }
    let value = if argc >= 2 { args[1] } else { args[0] };
    log_exception_value(ctx, label, value);
    qjs::JSValue::undefined()
}

pub unsafe fn install_runtime(rt: *mut qjs::JSRuntime) {
    if rt.is_null() {
        return;
    }
    qjs::JS_SetHostPromiseRejectionTracker(
        rt,
        Some(host_promise_rejection_tracker),
        core::ptr::null_mut(),
    );
}

pub unsafe fn install_context(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    // Idempotency guard.
    let mark_key = b"__trueos_diag_installed\0";
    let mark = qjs::JS_GetPropertyStr(ctx, global, mark_key.as_ptr() as *const c_char);
    let already = !mark.is_exception() && mark.tag != qjs::JS_TAG_UNDEFINED;
    qjs::js_free_value(ctx, mark);
    if already {
        qjs::js_free_value(ctx, global);
        return;
    }

    let fn_name = b"__trueos_report_exception\0";
    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(js_report_exception),
        fn_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, fn_name.as_ptr() as *const c_char, f);
    // Mark after successful native hook install.
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        mark_key.as_ptr() as *const c_char,
        qjs::JSValue {
            u: qjs::JSValueUnion { int32: 1 },
            tag: qjs::JS_TAG_BOOL,
        },
    );
    qjs::js_free_value(ctx, global);
}
