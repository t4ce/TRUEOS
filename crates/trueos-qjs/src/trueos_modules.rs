extern crate alloc;

use alloc::vec::Vec;
use core::ffi::{c_char, c_int, CStr};

use crate as qjs;

extern "C" {
    fn trueos_cabi_fs_read_file(path_ptr: *const u8, path_len: usize, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_cabi_fs_write_file(path_ptr: *const u8, path_len: usize, data_ptr: *const u8, data_len: usize) -> i32;
    fn trueos_cabi_fs_rename(src_ptr: *const u8, src_len: usize, dst_ptr: *const u8, dst_len: usize) -> i32;
    fn trueos_cabi_fs_list_dir(path_ptr: *const u8, path_len: usize, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_async_fs_read_file_start(path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_async_fs_result_len(op_id: u32) -> isize;
    fn trueos_cabi_async_fs_read_result(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_cabi_async_fs_discard(op_id: u32) -> i32;
    fn trueos_cabi_async_fs_wait_for_completion_blocking(timeout_ms: u64) -> i32;
    fn trueos_cabi_net_fetch_to_file(url_ptr: *const u8, url_len: usize, path_ptr: *const u8, path_len: usize) -> i32;
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

include!("../../../src/surface/cabi_codes.rs");

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

fn log_i32_dec(v: i32) {
    if v == 0 {
        log_str("0");
        return;
    }
    let mut n = if v < 0 { -(v as i64) as u64 } else { v as u64 };
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    while n != 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if v < 0 {
        log_str("-");
    }
    log_bytes(&buf[i..]);
}

fn log_usize_dec(v: usize) {
    if v == 0 {
        log_str("0");
        return;
    }
    let mut n = v as u64;
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while n != 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    log_bytes(&buf[i..]);
}

fn log_cstr_or_null(ptr: *const c_char) {
    if ptr.is_null() {
        log_str("<null>");
        return;
    }
    let bytes = unsafe { CStr::from_ptr(ptr).to_bytes() };
    log_bytes(bytes);
}

fn log_normalized(out: &[u8]) {
    log_str("qjs: normalize out=");
    log_bytes(out);
    log_nl();
}

unsafe fn js_arg_to_utf8_bytes(ctx: *mut qjs::JSContext, val: qjs::JSValueConst) -> Option<(*const u8, usize, *const c_char)> {
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, val, 0);
    if cstr.is_null() {
        return None;
    }
    Some((cstr as *const u8, len, cstr))
}

unsafe fn js_free_cstring(ctx: *mut qjs::JSContext, cstr: *const c_char) {
    if !cstr.is_null() {
        qjs::JS_FreeCString(ctx, cstr);
    }
}

unsafe fn js_make_complex(ctx: *mut qjs::JSContext, re: f64, im: f64) -> qjs::JSValue {
    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return obj;
    }

    let re_name = b"re\0";
    let im_name = b"im\0";

    let re_val = qjs::JS_NewFloat64(ctx, re);
    if qjs::JS_SetPropertyStr(ctx, obj, re_name.as_ptr() as *const c_char, re_val) < 0 {
        qjs::js_free_value(ctx, obj);
        return qjs::JSValue::exception();
    }

    let im_val = qjs::JS_NewFloat64(ctx, im);
    if qjs::JS_SetPropertyStr(ctx, obj, im_name.as_ptr() as *const c_char, im_val) < 0 {
        qjs::js_free_value(ctx, obj);
        return qjs::JSValue::exception();
    }

    obj
}

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

unsafe extern "C" fn qjs_process_next_tick(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let func = args[0];

    let call_argc = (argc - 1).max(0);
    let call_argv = if argc > 1 {
        unsafe { argv.add(1) }
    } else {
        core::ptr::null()
    };

    // Best-effort: call immediately. Later this can be wired to a real microtask/"nextTick" queue.
    unsafe { qjs::JS_Call(ctx, func, qjs::JSValue::undefined(), call_argc, call_argv) };
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_process_cwd(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // TODO: wire to per-context cwd once the shell/plumbing provides it.
    let cwd = b"/\0";
    unsafe { qjs::JS_NewStringLen(ctx, cwd.as_ptr() as *const c_char, cwd.len() - 1) }
}

unsafe extern "C" fn qjs_process_uptime(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // TODO: wire to kernel monotonic time.
    unsafe { qjs::JS_NewFloat64(core::ptr::null_mut(), 0.0) }
}

unsafe extern "C" fn qjs_process_hrtime(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // Node-style: [seconds, nanoseconds]. Best-effort zeros for now.
    let arr = unsafe { qjs::JS_NewArray(ctx) };
    if arr.is_exception() {
        return arr;
    }
    let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, 0, js_int32(0)) };
    let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, 1, js_int32(0)) };
    arr
}

unsafe extern "C" fn qjs_path_join(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return unsafe { qjs::JS_NewStringLen(ctx, b".\0".as_ptr() as *const c_char, 1) };
    }

    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let mut out: Vec<u8> = Vec::new();

    for &val in args {
        let mut len: usize = 0;
        let cstr = unsafe { qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, val, 0) };
        if cstr.is_null() {
            return qjs::JSValue::exception();
        }

        let bytes = unsafe { core::slice::from_raw_parts(cstr as *const u8, len) };
        if !bytes.is_empty() {
            if !out.is_empty() && out[out.len() - 1] != b'/' && bytes[0] != b'/' {
                out.push(b'/');
            }
            out.extend_from_slice(bytes);
        }

        unsafe { qjs::JS_FreeCString(ctx, cstr) };
    }

    if out.is_empty() {
        out.push(b'.');
    }

    unsafe { qjs::JS_NewStringLen(ctx, out.as_ptr() as *const c_char, out.len()) }
}

unsafe extern "C" fn qjs_path_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    if ctx.is_null() || m.is_null() {
        return -1;
    }

    let join_name = b"join\0";
    let join_fn = unsafe {
        qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_path_join),
            join_name.as_ptr() as *const c_char,
            2,
            qjs::JS_CFUNC_GENERIC,
            0,
        )
    };
    if unsafe { qjs::JS_SetModuleExport(ctx, m, join_name.as_ptr() as *const c_char, join_fn) } < 0 {
        return -1;
    }

    0
}

unsafe fn ensure_global_process(ctx: *mut qjs::JSContext) -> Result<qjs::JSValue, qjs::JSValue> {
    let global = qjs::JS_GetGlobalObject(ctx);
    let name = b"process\0";
    let mut proc = qjs::JS_GetPropertyStr(ctx, global, name.as_ptr() as *const c_char);
    if proc.is_exception() {
        qjs::js_free_value(ctx, global);
        return Err(qjs::JSValue::exception());
    }

    if proc.tag == qjs::JS_TAG_UNDEFINED || proc.tag == qjs::JS_TAG_NULL {
        // Drop the undefined/null handle and create a fresh process object.
        qjs::js_free_value(ctx, proc);

        let obj = qjs::JS_NewObject(ctx);
        if obj.is_exception() {
            qjs::js_free_value(ctx, global);
            return Err(qjs::JSValue::exception());
        }

        // env: plain object (userland can mutate)
        let env = qjs::JS_NewObject(ctx);
        if env.is_exception() {
            qjs::js_free_value(ctx, global);
            qjs::js_free_value(ctx, obj);
            return Err(qjs::JSValue::exception());
        }
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"env\0".as_ptr() as *const c_char, env);

        // argv: ["qjs"] default
        let argv = qjs::JS_NewArray(ctx);
        if argv.is_exception() {
            qjs::js_free_value(ctx, global);
            qjs::js_free_value(ctx, obj);
            return Err(qjs::JSValue::exception());
        }
        let qjs_str = qjs::JS_NewStringLen(ctx, b"qjs".as_ptr() as *const c_char, 3);
        let _ = qjs::JS_SetPropertyUint32(ctx, argv, 0, qjs_str);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"argv\0".as_ptr() as *const c_char, argv);

        // platform/arch/version
        let platform = qjs::JS_NewStringLen(ctx, b"trueos".as_ptr() as *const c_char, 6);
        let arch = qjs::JS_NewStringLen(ctx, b"x64".as_ptr() as *const c_char, 3);
        let version = qjs::JS_NewStringLen(ctx, b"0.0.0-trueos".as_ptr() as *const c_char, 12);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"platform\0".as_ptr() as *const c_char, platform);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"arch\0".as_ptr() as *const c_char, arch);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"version\0".as_ptr() as *const c_char, version);

        // pid (placeholder)
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"pid\0".as_ptr() as *const c_char, js_int32(1));

        // Functions: nextTick/cwd/hrtime/uptime
        let next_tick = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_next_tick),
            b"nextTick\0".as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let cwd = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_cwd),
            b"cwd\0".as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let hrtime = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_hrtime),
            b"hrtime\0".as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let uptime = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_uptime),
            b"uptime\0".as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"nextTick\0".as_ptr() as *const c_char, next_tick);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"cwd\0".as_ptr() as *const c_char, cwd);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"hrtime\0".as_ptr() as *const c_char, hrtime);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"uptime\0".as_ptr() as *const c_char, uptime);

        // Attach to globalThis.process (JS_SetPropertyStr consumes `obj` on success).
        let _ = qjs::JS_SetPropertyStr(ctx, global, name.as_ptr() as *const c_char, obj);

        // Re-read the process object so we have a handle we can return/inspect.
        proc = qjs::JS_GetPropertyStr(ctx, global, name.as_ptr() as *const c_char);
        if proc.is_exception() {
            qjs::js_free_value(ctx, global);
            return Err(qjs::JSValue::exception());
        }
    }

    qjs::js_free_value(ctx, global);
    Ok(proc)
}

unsafe extern "C" fn qjs_process_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    let proc = match ensure_global_process(ctx) {
        Ok(v) => v,
        Err(_) => return -1,
    };

    // Named exports: env/argv/cwd/nextTick/hrtime/uptime
    let env = qjs::JS_GetPropertyStr(ctx, proc, b"env\0".as_ptr() as *const c_char);
    let argv = qjs::JS_GetPropertyStr(ctx, proc, b"argv\0".as_ptr() as *const c_char);
    let cwd = qjs::JS_GetPropertyStr(ctx, proc, b"cwd\0".as_ptr() as *const c_char);
    let next_tick = qjs::JS_GetPropertyStr(ctx, proc, b"nextTick\0".as_ptr() as *const c_char);
    let hrtime = qjs::JS_GetPropertyStr(ctx, proc, b"hrtime\0".as_ptr() as *const c_char);
    let uptime = qjs::JS_GetPropertyStr(ctx, proc, b"uptime\0".as_ptr() as *const c_char);

    // Exporting consumes the values, so don't free them after.
    let _ = qjs::JS_SetModuleExport(ctx, m, b"env\0".as_ptr() as *const c_char, env);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"argv\0".as_ptr() as *const c_char, argv);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"cwd\0".as_ptr() as *const c_char, cwd);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"nextTick\0".as_ptr() as *const c_char, next_tick);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"hrtime\0".as_ptr() as *const c_char, hrtime);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"uptime\0".as_ptr() as *const c_char, uptime);

    let _ = qjs::JS_SetModuleExport(ctx, m, b"default\0".as_ptr() as *const c_char, proc);
    0
}

/// Install per-context globals that many npm packages assume exist.
///
/// Currently provides:
/// - `globalThis.process`
pub unsafe fn install_globals(ctx: *mut qjs::JSContext) {
    let Ok(proc) = ensure_global_process(ctx) else {
        return;
    };
    // Keep globalThis.process installed; drop our local handle.
    qjs::js_free_value(ctx, proc);
}

unsafe fn js_read_complex(
    ctx: *mut qjs::JSContext,
    val: qjs::JSValueConst,
) -> Result<trueos_math::Complex, qjs::JSValue> {
    let re_name = b"re\0";
    let im_name = b"im\0";

    let re_v = qjs::JS_GetPropertyStr(ctx, val, re_name.as_ptr() as *const c_char);
    if re_v.is_exception() {
        return Err(qjs::JSValue::exception());
    }
    let mut re = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut re as *mut f64, re_v) != 0 {
        qjs::js_free_value(ctx, re_v);
        return Err(qjs::JSValue::exception());
    }
    qjs::js_free_value(ctx, re_v);

    let im_v = qjs::JS_GetPropertyStr(ctx, val, im_name.as_ptr() as *const c_char);
    if im_v.is_exception() {
        return Err(qjs::JSValue::exception());
    }
    let mut im = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut im as *mut f64, im_v) != 0 {
        qjs::js_free_value(ctx, im_v);
        return Err(qjs::JSValue::exception());
    }
    qjs::js_free_value(ctx, im_v);

    Ok(trueos_math::Complex::new(re, im))
}

unsafe extern "C" fn qjs_complex_make(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut re = 0.0f64;
    let mut im = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut re as *mut f64, args[0]) != 0 {
        return qjs::JSValue::exception();
    }
    if qjs::JS_ToFloat64(ctx, &mut im as *mut f64, args[1]) != 0 {
        return qjs::JSValue::exception();
    }
    js_make_complex(ctx, re, im)
}

unsafe extern "C" fn qjs_complex_add(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let a = match js_read_complex(ctx, args[0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let b = match js_read_complex(ctx, args[1]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let out = a.add(b);
    js_make_complex(ctx, out.re, out.im)
}

unsafe extern "C" fn qjs_complex_square(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let a = match js_read_complex(ctx, args[0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let out = a.square();
    js_make_complex(ctx, out.re, out.im)
}

unsafe extern "C" fn qjs_complex_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    let make_name = b"make\0";
    let add_name = b"add\0";
    let square_name = b"square\0";

    let make_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_complex_make),
        make_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, make_name.as_ptr() as *const c_char, make_fn) < 0 {
        return -1;
    }

    let add_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_complex_add),
        add_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, add_name.as_ptr() as *const c_char, add_fn) < 0 {
        return -1;
    }

    let square_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_complex_square),
        square_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, square_name.as_ptr() as *const c_char, square_fn) < 0 {
        return -1;
    }

    0
}

unsafe extern "C" fn qjs_fs_read_file_bytes(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some((path_ptr, path_len, path_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };

    let need = unsafe { trueos_cabi_fs_read_file(path_ptr, path_len, core::ptr::null_mut(), 0) };
    if need < 0 {
        unsafe { js_free_cstring(ctx, path_cstr) };
        return qjs::JSValue::exception();
    }
    let need = need as usize;

    let buf = unsafe { qjs::js_malloc(ctx, need) } as *mut u8;
    if buf.is_null() {
        unsafe { js_free_cstring(ctx, path_cstr) };
        return qjs::JSValue::exception();
    }

    let got = unsafe { trueos_cabi_fs_read_file(path_ptr, path_len, buf, need) };
    unsafe { js_free_cstring(ctx, path_cstr) };
    if got < 0 {
        unsafe { qjs::js_free(ctx, buf as *mut core::ffi::c_void) };
        return qjs::JSValue::exception();
    }
    let got = got as usize;

    let ab = unsafe { qjs::JS_NewArrayBufferCopy(ctx, buf as *const u8, got) };
    unsafe { qjs::js_free(ctx, buf as *mut core::ffi::c_void) };
    ab
}

unsafe extern "C" fn qjs_fs_read_file_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some((path_ptr, path_len, path_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };

    let need = unsafe { trueos_cabi_fs_read_file(path_ptr, path_len, core::ptr::null_mut(), 0) };
    if need < 0 {
        unsafe { js_free_cstring(ctx, path_cstr) };
        return qjs::JSValue::exception();
    }
    let need = need as usize;

    let buf = unsafe { qjs::js_malloc(ctx, need + 1) } as *mut u8;
    if buf.is_null() {
        unsafe { js_free_cstring(ctx, path_cstr) };
        return qjs::JSValue::exception();
    }
    *unsafe { buf.add(need) } = 0;

    let got = unsafe { trueos_cabi_fs_read_file(path_ptr, path_len, buf, need) };
    unsafe { js_free_cstring(ctx, path_cstr) };
    if got < 0 {
        unsafe { qjs::js_free(ctx, buf as *mut core::ffi::c_void) };
        return qjs::JSValue::exception();
    }
    let got = got as usize;

    let s = unsafe { qjs::JS_NewStringLen(ctx, buf as *const c_char, got) };
    unsafe { qjs::js_free(ctx, buf as *mut core::ffi::c_void) };
    s
}

unsafe extern "C" fn qjs_fs_write_file_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);

    let Some((path_ptr, path_len, path_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };
    let Some((data_ptr, data_len, data_cstr)) = js_arg_to_utf8_bytes(ctx, args[1]) else {
        unsafe { js_free_cstring(ctx, path_cstr) };
        return qjs::JSValue::exception();
    };

    let rc = unsafe { trueos_cabi_fs_write_file(path_ptr, path_len, data_ptr, data_len) };
    unsafe { js_free_cstring(ctx, path_cstr) };
    unsafe { js_free_cstring(ctx, data_cstr) };
    if rc != 0 {
        return qjs::JSValue::exception();
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_fs_rename(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);

    let Some((src_ptr, src_len, src_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };
    let Some((dst_ptr, dst_len, dst_cstr)) = js_arg_to_utf8_bytes(ctx, args[1]) else {
        unsafe { js_free_cstring(ctx, src_cstr) };
        return qjs::JSValue::exception();
    };

    let rc = unsafe { trueos_cabi_fs_rename(src_ptr, src_len, dst_ptr, dst_len) };
    unsafe { js_free_cstring(ctx, src_cstr) };
    unsafe { js_free_cstring(ctx, dst_cstr) };
    if rc != 0 {
        return qjs::JSValue::exception();
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_fs_list_dir(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some((path_ptr, path_len, path_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };

    let need = unsafe { trueos_cabi_fs_list_dir(path_ptr, path_len, core::ptr::null_mut(), 0) };
    if need < 0 {
        unsafe { js_free_cstring(ctx, path_cstr) };
        return qjs::JSValue::exception();
    }
    let need = need as usize;

    let buf = unsafe { qjs::js_malloc(ctx, need + 1) } as *mut u8;
    if buf.is_null() {
        unsafe { js_free_cstring(ctx, path_cstr) };
        return qjs::JSValue::exception();
    }
    *unsafe { buf.add(need) } = 0;

    let got = unsafe { trueos_cabi_fs_list_dir(path_ptr, path_len, buf, need) };
    unsafe { js_free_cstring(ctx, path_cstr) };
    if got < 0 {
        unsafe { qjs::js_free(ctx, buf as *mut core::ffi::c_void) };
        return qjs::JSValue::exception();
    }
    let got = got as usize;
    let s = unsafe { qjs::JS_NewStringLen(ctx, buf as *const c_char, got) };
    unsafe { qjs::js_free(ctx, buf as *mut core::ffi::c_void) };
    s
}

unsafe extern "C" fn qjs_fs_remove(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some((path_ptr, path_len, path_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };

    let rc = unsafe { trueos_cabi_fs_remove(path_ptr, path_len) };
    unsafe { js_free_cstring(ctx, path_cstr) };
    if rc != 0 {
        return qjs::JSValue::exception();
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_fs_read_file_text_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some((path_ptr, path_len, path_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };

    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);
    let path = core::slice::from_raw_parts(path_ptr, path_len);
    let op_id = qjs::async_ops::start_read_file(path);

    js_free_cstring(ctx, path_cstr);

    match op_id {
        Ok(id) => {
            qjs::async_ops::register_promise(
                ctx,
                id,
                qjs::async_ops::OpKind::ReadText,
                resolve,
                reject,
            );
        }
        Err(code) => {
            let arg = js_int32(code);
            let _ = qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &arg as *const qjs::JSValue);
        }
    }

    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn qjs_fs_read_file_bytes_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some((path_ptr, path_len, path_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };

    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);
    let path = core::slice::from_raw_parts(path_ptr, path_len);
    let op_id = qjs::async_ops::start_read_file(path);

    js_free_cstring(ctx, path_cstr);

    match op_id {
        Ok(id) => {
            qjs::async_ops::register_promise(
                ctx,
                id,
                qjs::async_ops::OpKind::ReadBytes,
                resolve,
                reject,
            );
        }
        Err(code) => {
            let arg = js_int32(code);
            let _ = qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &arg as *const qjs::JSValue);
        }
    }

    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn qjs_fs_write_file_text_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);

    let Some((path_ptr, path_len, path_cstr)) = js_arg_to_utf8_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };
    let Some((data_ptr, data_len, data_cstr)) = js_arg_to_utf8_bytes(ctx, args[1]) else {
        js_free_cstring(ctx, path_cstr);
        return qjs::JSValue::exception();
    };

    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);

    let path = core::slice::from_raw_parts(path_ptr, path_len);
    let data = core::slice::from_raw_parts(data_ptr, data_len);
    let op_id = qjs::async_ops::start_write_file(path, data);

    js_free_cstring(ctx, path_cstr);
    js_free_cstring(ctx, data_cstr);

    match op_id {
        Ok(id) => {
            qjs::async_ops::register_promise(
                ctx,
                id,
                qjs::async_ops::OpKind::WriteText,
                resolve,
                reject,
            );
        }
        Err(code) => {
            let arg = js_int32(code);
            let _ = qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &arg as *const qjs::JSValue);
        }
    }

    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn qjs_fs_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    let read_bytes_name = b"readFileBytes\0";
    let read_text_name = b"readFile\0";
    let write_text_name = b"writeFile\0";
    let rename_name = b"rename\0";
    let list_dir_name = b"listDir\0";
    let remove_name = b"remove\0";

    let read_text_async_name = b"readFileAsync\0";
    let read_bytes_async_name = b"readFileBytesAsync\0";
    let write_text_async_name = b"writeFileAsync\0";

    let read_bytes_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_read_file_bytes),
        read_bytes_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, read_bytes_name.as_ptr() as *const c_char, read_bytes_fn) < 0 {
        return -1;
    }

    let read_text_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_read_file_text),
        read_text_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, read_text_name.as_ptr() as *const c_char, read_text_fn) < 0 {
        return -1;
    }

    let write_text_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_write_file_text),
        write_text_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, write_text_name.as_ptr() as *const c_char, write_text_fn) < 0 {
        return -1;
    }

    let rename_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_rename),
        rename_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, rename_name.as_ptr() as *const c_char, rename_fn) < 0 {
        return -1;
    }

    let list_dir_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_list_dir),
        list_dir_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, list_dir_name.as_ptr() as *const c_char, list_dir_fn) < 0 {
        return -1;
    }

    let remove_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_remove),
        remove_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, remove_name.as_ptr() as *const c_char, remove_fn) < 0 {
        return -1;
    }

    let read_text_async_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_read_file_text_async),
        read_text_async_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, read_text_async_name.as_ptr() as *const c_char, read_text_async_fn) < 0 {
        return -1;
    }

    let read_bytes_async_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_read_file_bytes_async),
        read_bytes_async_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, read_bytes_async_name.as_ptr() as *const c_char, read_bytes_async_fn) < 0 {
        return -1;
    }

    let write_text_async_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_write_file_text_async),
        write_text_async_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, write_text_async_name.as_ptr() as *const c_char, write_text_async_fn) < 0 {
        return -1;
    }

    0
}

/// Attempt to load a TRUEOS-provided native module.
///
/// Returns null if the module is not recognized.
pub unsafe fn load_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let name = CStr::from_ptr(module_name).to_bytes();

    let (init, exports): (qjs::JSModuleInitFunc, &[&[u8]]) = if name == b"complex" {
        (qjs_complex_module_init, &[b"make\0", b"add\0", b"square\0"])
    } else if name == b"fs" {
        (
            qjs_fs_module_init,
            &[
                b"readFileBytes\0",
                b"readFile\0",
                b"writeFile\0",
                b"rename\0",
                b"listDir\0",
                b"remove\0",
				b"readFileAsync\0",
				b"readFileBytesAsync\0",
				b"writeFileAsync\0",
            ],
        )
    } else if name == b"process" || name == b"node:process" {
        (
            qjs_process_module_init,
            &[b"default\0", b"env\0", b"argv\0", b"cwd\0", b"nextTick\0", b"hrtime\0", b"uptime\0"],
        )
    } else if name == b"path" || name == b"node:path" {
        (qjs_path_module_init, &[b"join\0"])
    } else {
        return core::ptr::null_mut();
    };

    let m = qjs::JS_NewCModule(ctx, module_name, Some(init));
    if m.is_null() {
        return core::ptr::null_mut();
    }

    for &e in exports {
        let _ = qjs::JS_AddModuleExport(ctx, m, e.as_ptr() as *const c_char);
    }

    m
}

pub(crate) unsafe extern "C" fn trueos_module_loader(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut qjs::JSModuleDef {
    if !module_name.is_null() {
        let spec = CStr::from_ptr(module_name).to_bytes();
        log_str("qjs: loader spec=");
        log_bytes(spec);
        log_nl();
    } else {
        log_str("qjs: loader spec=<null>\n");
    }

    let m = load_native_module(ctx, module_name);
    if !m.is_null() {
        log_str("qjs: loader native ok\n");
        return m;
    }

    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let spec = CStr::from_ptr(module_name).to_bytes();
    if spec_is_url(spec) {
        log_str("qjs: loader url\n");
        return load_url_module(ctx, module_name, spec);
    }

    log_str("qjs: loader fs\n");

    load_fs_module(ctx, module_name)
}

fn path_is_relative(spec: &[u8]) -> bool {
    spec.starts_with(b"./") || spec.starts_with(b"../")
}

fn path_is_absolute(spec: &[u8]) -> bool {
    spec.starts_with(b"/")
}

fn spec_is_url(spec: &[u8]) -> bool {
    spec.starts_with(b"https://") || spec.starts_with(b"http://")
}

const ESM_SH_PREFIX: &[u8] = b"https://esm.sh/";
const CDN_DIR: &[u8] = b"/qjs/cdn/";
const CACHE_READ_TIMEOUT_MS: u64 = 8000;

fn push_i32_dec(out: &mut Vec<u8>, v: i32) {
    if v == 0 {
        out.push(b'0');
        return;
    }
    let neg = v < 0;
    let mut n = if neg { -(v as i64) as u64 } else { v as u64 };
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    while n != 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if neg {
        out.push(b'-');
    }
    out.extend_from_slice(&buf[i..]);
}

unsafe fn throw_error(ctx: *mut qjs::JSContext, msg: &[u8]) {
    let err = qjs::JS_NewError(ctx);
    if err.is_exception() {
        return;
    }
    let m = qjs::JS_NewStringLen(ctx, msg.as_ptr() as *const c_char, msg.len());
    let _ = qjs::JS_SetPropertyStr(ctx, err, b"message\0".as_ptr() as *const c_char, m);
    let _ = qjs::JS_Throw(ctx, err);
}

unsafe fn read_file_js_malloc_rc(ctx: *mut qjs::JSContext, path: &[u8]) -> Result<(*mut u8, usize), isize> {
    let need = trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0);
    if need < 0 {
        return Err(need);
    }
    let need = need as usize;

    let buf = qjs::js_malloc(ctx, need + 1) as *mut u8;
    if buf.is_null() {
        return Err(-4); // FS_ERR_BAD_PARAM (best-effort sentinel)
    }
    *buf.add(need) = 0;

    let got = trueos_cabi_fs_read_file(path.as_ptr(), path.len(), buf, need);
    if got < 0 {
        qjs::js_free(ctx, buf as *mut core::ffi::c_void);
        return Err(got);
    }
    Ok((buf, got as usize))
}

unsafe fn read_file_js_malloc_rc_async(
    ctx: *mut qjs::JSContext,
    path: &[u8],
    timeout_ms: u64,
) -> Result<(*mut u8, usize), isize> {
    let op_id = trueos_cabi_async_fs_read_file_start(path.as_ptr(), path.len());
    if op_id < 0 {
        return Err(op_id as isize);
    }
    let op_id = op_id as u32;

    loop {
        let len = trueos_cabi_async_fs_result_len(op_id);
        if len == FS_ERR_NOT_FOUND as isize {
            let ok = trueos_cabi_async_fs_wait_for_completion_blocking(timeout_ms);
            if ok == 0 {
                let _ = trueos_cabi_async_fs_discard(op_id);
                log_str("qjs: async fs read timeout (treat miss)\n");
                return Err(FS_ERR_NOT_FOUND as isize);
            }
            continue;
        }
        if len < 0 {
            let _ = trueos_cabi_async_fs_discard(op_id);
            return Err(len);
        }

        let len = len as usize;
        let buf = qjs::js_malloc(ctx, len + 1) as *mut u8;
        if buf.is_null() {
            let _ = trueos_cabi_async_fs_discard(op_id);
            return Err(FS_ERR_NO_SPACE as isize);
        }
        *buf.add(len) = 0;

        let got = trueos_cabi_async_fs_read_result(op_id, buf, len);
        if got < 0 {
            qjs::js_free(ctx, buf as *mut core::ffi::c_void);
            return Err(got);
        }
        return Ok((buf, got as usize));
    }
}

unsafe fn read_file_js_malloc(ctx: *mut qjs::JSContext, path: &[u8]) -> Result<(*mut u8, usize), ()> {
    read_file_js_malloc_rc(ctx, path).map_err(|_| ())
}

unsafe fn compile_module_from_buf(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
    buf: *const u8,
    len: usize,
) -> *mut qjs::JSModuleDef {
    let flags = qjs::JS_EVAL_TYPE_MODULE | qjs::JS_EVAL_FLAG_COMPILE_ONLY;
    let val = qjs::JS_Eval(ctx, buf as *const c_char, len, module_name, flags);

    if val.is_exception() {
        return core::ptr::null_mut();
    }
    if val.tag != qjs::JS_TAG_MODULE {
        qjs::js_free_value(ctx, val);
        return core::ptr::null_mut();
    }

    // `JS_Eval(..., COMPILE_ONLY|MODULE)` returns a `JSValue` (tagged as MODULE)
    // which must be released after extracting the module pointer, otherwise the
    // runtime will retain an extra reference and assert in `JS_FreeRuntime`.
    let m = val.u.ptr as *mut qjs::JSModuleDef;
    qjs::js_free_value(ctx, val);
    m
}

fn split_url(spec: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    // Returns (scheme_with_slashes, authority, path_with_leading_slash)
    let (scheme, rest) = if let Some(r) = spec.strip_prefix(b"https://") {
        (b"https://".as_slice(), r)
    } else if let Some(r) = spec.strip_prefix(b"http://") {
        (b"http://".as_slice(), r)
    } else {
        return None;
    };

    let (authority, path) = match rest.iter().position(|&b| b == b'/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, b"/".as_slice()),
    };
    if authority.is_empty() {
        return None;
    }
    Some((scheme, authority, path))
}

fn url_origin_prefix(url: &[u8]) -> Option<Vec<u8>> {
    let (scheme, authority, _path) = split_url(url)?;
    let mut out = Vec::new();
    out.extend_from_slice(scheme);
    out.extend_from_slice(authority);
    Some(out)
}

fn url_dir_base(url: &[u8]) -> Option<Vec<u8>> {
    // Base directory prefix for resolving relative URL specifiers.
    // Example: https://host/a/b/c.mjs -> https://host/a/b
    let (scheme, authority, path) = split_url(url)?;
    let last_slash = path.iter().rposition(|&b| b == b'/').unwrap_or(0);
    let dir = &path[..last_slash];

    let mut out = Vec::new();
    out.extend_from_slice(scheme);
    out.extend_from_slice(authority);
    if !dir.starts_with(b"/") {
        out.push(b'/');
    }
    out.extend_from_slice(dir);
    Some(out)
}

fn normalize_url_bytes(url: &[u8]) -> Option<Vec<u8>> {
    let (scheme, authority, path) = split_url(url)?;
    let normalized_path = normalize_path_bytes(path);
    let mut out = Vec::new();
    out.extend_from_slice(scheme);
    out.extend_from_slice(authority);
    if !normalized_path.starts_with(b"/") {
        out.push(b'/');
    }
    out.extend_from_slice(&normalized_path);
    Some(out)
}

fn resolve_relative_url(base_url: &[u8], rel: &[u8]) -> Option<Vec<u8>> {
    let mut base_dir = url_dir_base(base_url)?;
    if !base_dir.ends_with(b"/") {
        base_dir.push(b'/');
    }
    base_dir.extend_from_slice(rel);
    normalize_url_bytes(&base_dir)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn push_hex_u64(out: &mut Vec<u8>, v: u64) {
    for i in (0..16).rev() {
        let nibble = ((v >> (i * 4)) & 0xF) as u8;
        let c = match nibble {
            0..=9 => b'0' + nibble,
            _ => b'a' + (nibble - 10),
        };
        out.push(c);
    }
}

fn normalize_path_bytes(path: &[u8]) -> Vec<u8> {
    // Very small path normalizer:
    // - keeps a leading '/'
    // - removes '.' segments
    // - resolves '..' segments
    let is_abs = path.starts_with(b"/");
    let mut parts: Vec<&[u8]> = Vec::new();

    for seg in path.split(|&b| b == b'/') {
        if seg.is_empty() || seg == b"." {
            continue;
        }
        if seg == b".." {
            if let Some(_) = parts.pop() {
                continue;
            }
            // If we're absolute, don't allow escaping above root.
            if is_abs {
                continue;
            }
            parts.push(seg);
            continue;
        }
        parts.push(seg);
    }

    let mut out = Vec::new();
    if is_abs {
        out.push(b'/');
    }
    for (i, seg) in parts.iter().enumerate() {
        if i != 0 {
            out.push(b'/');
        }
        out.extend_from_slice(seg);
    }
    out
}

unsafe fn js_strdup(ctx: *mut qjs::JSContext, bytes: &[u8]) -> *mut c_char {
    let buf = qjs::js_malloc(ctx, bytes.len() + 1) as *mut u8;
    if buf.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
    *buf.add(bytes.len()) = 0;
    buf as *mut c_char
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NormalizeMode {
    /// Base TRUEOS resolution: bare specifiers go to esm.sh; `node:*` stays native.
    Base,
    /// Node-ish resolution: common Node builtins and unknown `node:*` may be routed to esm.sh.
    Node,
}

fn node_builtin_shim_url(spec: &[u8]) -> Option<&'static [u8]> {
    // esm.sh does not currently serve `node:*` specifiers directly (e.g. `https://esm.sh/node:path` is 404).
    // For Node-ish compatibility, route common builtins to browser/polyfill packages instead.
    match spec {
        b"assert" => Some(b"https://esm.sh/assert@2.1.0"),
        b"buffer" => Some(b"https://esm.sh/buffer@6.0.3"),
        b"crypto" => Some(b"https://esm.sh/crypto-browserify@3.12.0"),
        b"events" => Some(b"https://esm.sh/events@3.3.0"),
        b"path" => Some(b"https://esm.sh/path-browserify@1.0.1"),
        b"stream" => Some(b"https://esm.sh/stream-browserify@3.0.0"),
        b"timers" => Some(b"https://esm.sh/timers-browserify@2.0.12"),
        b"tty" => Some(b"https://esm.sh/tty-browserify@0.0.1"),
        b"url" => Some(b"https://esm.sh/url@0.11.3"),
        b"util" => Some(b"https://esm.sh/util@0.12.5"),
        _ => None,
    }
}

pub(crate) unsafe fn normalize_with_mode(
    ctx: *mut qjs::JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    mode: NormalizeMode,
) -> *mut c_char {
    log_str("qjs: normalize mode=");
    match mode {
        NormalizeMode::Base => log_str("base"),
        NormalizeMode::Node => log_str("node"),
    }
    log_str(" base=");
    log_cstr_or_null(module_base_name);
    log_str(" spec=");
    log_cstr_or_null(module_name);
    log_nl();

    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let spec = CStr::from_ptr(module_name).to_bytes();

    // URL imports: keep absolute URLs as-is; resolve relative URLs against URL base.
    if spec_is_url(spec) {
        if let Some(normalized) = normalize_url_bytes(spec) {
            log_normalized(&normalized);
            return js_strdup(ctx, &normalized);
        }
        log_normalized(spec);
        return js_strdup(ctx, spec);
    }

    if path_is_relative(spec) {
        if !module_base_name.is_null() {
            let base = CStr::from_ptr(module_base_name).to_bytes();
            if spec_is_url(base) {
                if let Some(resolved) = resolve_relative_url(base, spec) {
                    log_normalized(&resolved);
                    return js_strdup(ctx, &resolved);
                }
            }
        }
    }

    // If the base is a URL, treat "/foo" as an origin-relative URL path.
    if path_is_absolute(spec) {
        if !module_base_name.is_null() {
            let base = CStr::from_ptr(module_base_name).to_bytes();
            if spec_is_url(base) {
                if let Some(mut origin) = url_origin_prefix(base) {
                    let normalized_path = normalize_path_bytes(spec);
                    origin.extend_from_slice(&normalized_path);
                    log_normalized(&origin);
                    return js_strdup(ctx, &origin);
                }
            }
        }
    }

    // Absolute filesystem paths.
    if path_is_absolute(spec) {
        let normalized = normalize_path_bytes(spec);
        log_normalized(&normalized);
        return js_strdup(ctx, &normalized);
    }

    // Bare specifiers.
    if !path_is_relative(spec) {
        // Always keep known TRUEOS native modules.
        if spec == b"complex"
            || spec == b"fs"
            || spec == b"process"
            || spec == b"node:process"
            || spec == b"path"
            || spec == b"node:path"
        {
            log_normalized(spec);
            return js_strdup(ctx, spec);
        }

        // `node:*` handling differs by mode.
        if spec.starts_with(b"node:") {
            match mode {
                NormalizeMode::Base => return js_strdup(ctx, spec),
                NormalizeMode::Node => {
                    // Only keep truly native `node:*` modules; shim the rest.
                    if spec == b"node:process" || spec == b"node:path" {
                        log_normalized(spec);
                        return js_strdup(ctx, spec);
                    }

                    // Try a curated polyfill mapping first.
                    let name = &spec[b"node:".len()..];
                    if let Some(url) = node_builtin_shim_url(name) {
                        log_normalized(url);
                        return js_strdup(ctx, url);
                    }

                    // Fallback: strip `node:` and ask esm.sh for the corresponding package name.
                    let mut url = Vec::new();
                    url.extend_from_slice(ESM_SH_PREFIX);
                    url.extend_from_slice(name);
                    log_normalized(&url);
                    return js_strdup(ctx, &url);
                }
            }
        }

        // In Node mode, treat common Node builtins as shims.
        if mode == NormalizeMode::Node {
            if let Some(url) = node_builtin_shim_url(spec) {
                log_normalized(url);
                return js_strdup(ctx, url);
            }
        }

        // Default: route bare specifiers through esm.sh.
        let mut url = Vec::new();
        url.extend_from_slice(ESM_SH_PREFIX);
        url.extend_from_slice(spec);
        log_normalized(&url);
        return js_strdup(ctx, &url);
    }

    // Resolve relative specifiers against the directory part of the base name.
    if module_base_name.is_null() {
        return js_strdup(ctx, spec);
    }

    let base = CStr::from_ptr(module_base_name).to_bytes();
    let base_dir = match base.iter().rposition(|&b| b == b'/') {
        Some(pos) => &base[..pos],
        None => b"",
    };

    let mut combined = Vec::new();
    if !base_dir.is_empty() {
        combined.extend_from_slice(base_dir);
        combined.push(b'/');
    }
    combined.extend_from_slice(spec);

    let normalized = normalize_path_bytes(&combined);
    log_normalized(&normalized);
    js_strdup(ctx, &normalized)
}

unsafe extern "C" fn trueos_module_normalize(
    ctx: *mut qjs::JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut c_char {
    normalize_with_mode(ctx, module_base_name, module_name, NormalizeMode::Base)
}

unsafe fn load_url_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
    url: &[u8],
) -> *mut qjs::JSModuleDef {
    // Cache path: /qjs/cdn/<hash>.mjs
    let hash = fnv1a64(url);
    let mut cache_path: Vec<u8> = Vec::new();
    cache_path.extend_from_slice(CDN_DIR);
    push_hex_u64(&mut cache_path, hash);
    cache_path.extend_from_slice(b".mjs");

    log_str("qjs: url cache=");
    log_bytes(&cache_path);
    log_nl();

    // Fast-path: if the cached module is already present, avoid refetching.
    match read_file_js_malloc_rc_async(ctx, &cache_path, CACHE_READ_TIMEOUT_MS) {
        Ok((buf, len)) => {
            log_str("qjs: url cache hit len=");
            log_usize_dec(len);
            log_nl();
            log_str("qjs: url compile start\n");
            let m = compile_module_from_buf(ctx, module_name, buf, len);
            log_str("qjs: url compile done\n");
            qjs::js_free(ctx, buf as *mut core::ffi::c_void);
            return m;
        }
        Err(rc) => {
            log_str("qjs: url cache miss rc=");
            log_i32_dec(rc as i32);
            log_nl();
            // Fall back to network fetch on NOT_FOUND.
            // Also treat IO errors as cache-miss: we may see transient FS errors or a partially
            // written cache entry, and fetching again is a safe recovery path.
            if rc != -8 && rc != -2 {
                let mut msg = Vec::new();
                msg.extend_from_slice(b"read cached module failed rc=");
                push_i32_dec(&mut msg, rc as i32);
                msg.extend_from_slice(b" (");
                msg.extend_from_slice(cabi_rc_name(rc as i32));
                msg.extend_from_slice(b") cache=");
                msg.extend_from_slice(&cache_path);
                throw_error(ctx, &msg);
                return core::ptr::null_mut();
            }
        }
    }


    log_str("qjs: url fetch start url=");
    log_bytes(url);
    log_nl();
    let rc = trueos_cabi_net_fetch_to_file(url.as_ptr(), url.len(), cache_path.as_ptr(), cache_path.len());
    if rc != 0 {
        let mut msg = Vec::new();
        msg.extend_from_slice(b"fetch-to-cache failed rc=");
        push_i32_dec(&mut msg, rc);
        msg.extend_from_slice(b" (");
        msg.extend_from_slice(cabi_rc_name(rc));
        msg.extend_from_slice(b")");
        msg.extend_from_slice(b" url=");
        msg.extend_from_slice(url);
        msg.extend_from_slice(b" cache=");
        msg.extend_from_slice(&cache_path);
        throw_error(ctx, &msg);
        return core::ptr::null_mut();
    }
    log_str("qjs: url fetch ok\n");

    let (buf, len) = match read_file_js_malloc_rc_async(ctx, &cache_path, CACHE_READ_TIMEOUT_MS) {
        Ok(v) => v,
        Err(_) => {
            let mut msg = Vec::new();
            msg.extend_from_slice(b"read cached module failed url=");
            msg.extend_from_slice(url);
            msg.extend_from_slice(b" cache=");
            msg.extend_from_slice(&cache_path);
            throw_error(ctx, &msg);
            return core::ptr::null_mut();
        }
    };

    // Use the URL as the module filename so relative URL imports resolve correctly.
    log_str("qjs: url compile start\n");
    let m = compile_module_from_buf(ctx, module_name, buf, len);
    log_str("qjs: url compile done\n");
    qjs::js_free(ctx, buf as *mut core::ffi::c_void);
    m
}

unsafe fn load_fs_module(ctx: *mut qjs::JSContext, module_name: *const c_char) -> *mut qjs::JSModuleDef {
    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let path = CStr::from_ptr(module_name).to_bytes();
    log_str("qjs: fs load path=");
    log_bytes(path);
    log_nl();
    // Only attempt filesystem loading for absolute paths or explicit relative paths.
    if !(path_is_absolute(path) || path_is_relative(path)) {
        return core::ptr::null_mut();
    }

    let (buf, len) = match read_file_js_malloc(ctx, path) {
        Ok(v) => v,
        Err(()) => {
            let mut msg = Vec::new();
            msg.extend_from_slice(b"read module failed path=");
            msg.extend_from_slice(path);
            throw_error(ctx, &msg);
            return core::ptr::null_mut();
        }
    };

    log_str("qjs: fs read len=");
    log_usize_dec(len);
    log_nl();
    log_str("qjs: fs compile start\n");
    let m = compile_module_from_buf(ctx, module_name, buf, len);
    log_str("qjs: fs compile done\n");
    qjs::js_free(ctx, buf as *mut core::ffi::c_void);
    m
}

/// Install the TRUEOS module loader into a runtime.
///
/// Provides:
/// - Native modules: `"complex"`, `"fs"`
/// - Filesystem-backed ES modules: `import "/path/to/mod.mjs"` and relative imports.
/// - URL ES modules: `import "https://..."` (cached under `/qjs/cdn/`).
pub unsafe fn install(rt: *mut qjs::JSRuntime) {
    if rt.is_null() {
        return;
    }
    qjs::JS_SetModuleLoaderFunc(rt, Some(trueos_module_normalize), Some(trueos_module_loader), core::ptr::null_mut());
}
