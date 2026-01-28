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
    fn trueos_cabi_net_fetch_to_file(url_ptr: *const u8, url_len: usize, path_ptr: *const u8, path_len: usize) -> i32;
}

#[inline]
fn cabi_rc_name(rc: i32) -> &'static [u8] {
    match rc {
        0 => b"OK",
        -1 => b"FS_ERR_BAD_UTF8",
        -2 => b"FS_ERR_IO",
        -3 => b"FS_ERR_NO_SPACE",
        -4 => b"FS_ERR_BAD_PARAM",
        -5 => b"FS_ERR_USBMS_NOT_FOUND",
        -6 => b"FS_ERR_BAD_PATH",
        -7 => b"FS_ERR_TOO_LARGE",
        -8 => b"FS_ERR_NOT_FOUND",
        -9 => b"FS_ERR_ALREADY_EXISTS",
        -10 => b"NET_ERR_BAD_URL",
        -11 => b"NET_ERR_TIMEOUT",
        -12 => b"NET_ERR_HTTP",
        -13 => b"NET_ERR_TLS",
        -111 => b"NET_ERR_TIMEOUT_DNS",
        -112 => b"NET_ERR_TIMEOUT_CONNECT",
        -113 => b"NET_ERR_TIMEOUT_HEADERS",
        -114 => b"NET_ERR_TIMEOUT_BODY",
        _ => b"UNKNOWN",
    }
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

unsafe extern "C" fn qjs_fs_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    let read_bytes_name = b"readFileBytes\0";
    let read_text_name = b"readFile\0";
    let write_text_name = b"writeFile\0";
    let rename_name = b"rename\0";
    let list_dir_name = b"listDir\0";
    let remove_name = b"remove\0";

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
            ],
        )
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

unsafe extern "C" fn trueos_module_loader(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut qjs::JSModuleDef {
    let m = load_native_module(ctx, module_name);
    if !m.is_null() {
        return m;
    }

    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let spec = CStr::from_ptr(module_name).to_bytes();
    if spec_is_url(spec) {
        return load_url_module(ctx, module_name, spec);
    }

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

unsafe fn read_file_js_malloc(ctx: *mut qjs::JSContext, path: &[u8]) -> Result<(*mut u8, usize), ()> {
    let need = trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0);
    if need < 0 {
        return Err(());
    }
    let need = need as usize;

    let buf = qjs::js_malloc(ctx, need + 1) as *mut u8;
    if buf.is_null() {
        return Err(());
    }
    *buf.add(need) = 0;

    let got = trueos_cabi_fs_read_file(path.as_ptr(), path.len(), buf, need);
    if got < 0 {
        qjs::js_free(ctx, buf as *mut core::ffi::c_void);
        return Err(());
    }
    Ok((buf, got as usize))
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

    val.u.ptr as *mut qjs::JSModuleDef
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

unsafe extern "C" fn trueos_module_normalize(
    ctx: *mut qjs::JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut c_char {
    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let spec = CStr::from_ptr(module_name).to_bytes();

    // URL imports: keep absolute URLs as-is; resolve relative URLs against URL base.
    if spec_is_url(spec) {
        if let Some(normalized) = normalize_url_bytes(spec) {
            return js_strdup(ctx, &normalized);
        }
        return js_strdup(ctx, spec);
    }

    if path_is_relative(spec) {
        if !module_base_name.is_null() {
            let base = CStr::from_ptr(module_base_name).to_bytes();
            if spec_is_url(base) {
                if let Some(resolved) = resolve_relative_url(base, spec) {
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
                    return js_strdup(ctx, &origin);
                }
            }
        }
    }

    // Absolute filesystem paths.
    if path_is_absolute(spec) {
        let normalized = normalize_path_bytes(spec);
        return js_strdup(ctx, &normalized);
    }

    // Bare specifiers: keep native ones as-is, otherwise route through esm.sh.
    if !path_is_relative(spec) {
        if spec == b"complex" || spec == b"fs" {
            return js_strdup(ctx, spec);
        }

        let mut url = Vec::new();
        url.extend_from_slice(ESM_SH_PREFIX);
        url.extend_from_slice(spec);
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
    js_strdup(ctx, &normalized)
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

    let (buf, len) = match read_file_js_malloc(ctx, &cache_path) {
        Ok(v) => v,
        Err(()) => {
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
    let m = compile_module_from_buf(ctx, module_name, buf, len);
    qjs::js_free(ctx, buf as *mut core::ffi::c_void);
    m
}

unsafe fn load_fs_module(ctx: *mut qjs::JSContext, module_name: *const c_char) -> *mut qjs::JSModuleDef {
    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let path = CStr::from_ptr(module_name).to_bytes();
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

    let m = compile_module_from_buf(ctx, module_name, buf, len);
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
