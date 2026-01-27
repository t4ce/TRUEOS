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

    load_fs_module(ctx, module_name)
}

fn path_is_relative(spec: &[u8]) -> bool {
    spec.starts_with(b"./") || spec.starts_with(b"../")
}

fn path_is_absolute(spec: &[u8]) -> bool {
    spec.starts_with(b"/")
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

    // Leave native/bare specifiers unchanged; loader can still accept absolute paths.
    if path_is_absolute(spec) {
        let normalized = normalize_path_bytes(spec);
        return js_strdup(ctx, &normalized);
    }

    if !path_is_relative(spec) {
        return js_strdup(ctx, spec);
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

unsafe fn load_fs_module(ctx: *mut qjs::JSContext, module_name: *const c_char) -> *mut qjs::JSModuleDef {
    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let path = CStr::from_ptr(module_name).to_bytes();
    // Only attempt filesystem loading for absolute paths or explicit relative paths.
    if !(path_is_absolute(path) || path_is_relative(path)) {
        return core::ptr::null_mut();
    }

    let need = trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0);
    if need < 0 {
        return core::ptr::null_mut();
    }
    let need = need as usize;

    let buf = qjs::js_malloc(ctx, need + 1) as *mut u8;
    if buf.is_null() {
        return core::ptr::null_mut();
    }
    *buf.add(need) = 0;

    let got = trueos_cabi_fs_read_file(path.as_ptr(), path.len(), buf, need);
    if got < 0 {
        qjs::js_free(ctx, buf as *mut core::ffi::c_void);
        return core::ptr::null_mut();
    }
    let got = got as usize;

    let flags = qjs::JS_EVAL_TYPE_MODULE | qjs::JS_EVAL_FLAG_COMPILE_ONLY;
    let val = qjs::JS_Eval(
        ctx,
        buf as *const c_char,
        got,
        module_name,
        flags,
    );

    qjs::js_free(ctx, buf as *mut core::ffi::c_void);

    if val.is_exception() {
        return core::ptr::null_mut();
    }

    if val.tag != qjs::JS_TAG_MODULE {
        qjs::js_free_value(ctx, val);
        return core::ptr::null_mut();
    }

    val.u.ptr as *mut qjs::JSModuleDef
}

/// Install the TRUEOS module loader into a runtime.
///
/// Provides:
/// - Native modules: `"complex"`, `"fs"`
/// - Filesystem-backed ES modules: `import "/path/to/mod.mjs"` and relative imports.
pub unsafe fn install(rt: *mut qjs::JSRuntime) {
    if rt.is_null() {
        return;
    }
    qjs::JS_SetModuleLoaderFunc(rt, Some(trueos_module_normalize), Some(trueos_module_loader), core::ptr::null_mut());
}
