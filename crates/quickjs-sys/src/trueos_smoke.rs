use core::ffi::{c_char, c_int, CStr};

use crate as qjs;

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

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

unsafe fn dump_exception(ctx: *mut qjs::JSContext) {
    let exc = qjs::JS_GetException(ctx);
    let cstr = qjs::js_to_cstring(ctx, exc);
    log_str("quickjs: exception: ");
    if !cstr.is_null() {
        let bytes = CStr::from_ptr(cstr).to_bytes();
        // Best-effort: assume utf8 for logs; fallback to raw bytes.
        if let Ok(s) = core::str::from_utf8(bytes) {
            log_str(s);
        } else {
            log_bytes(bytes);
        }
        qjs::JS_FreeCString(ctx, cstr);
    } else {
        log_str("<toString failed>");
    }
    log_nl();
    qjs::js_free_value(ctx, exc);
}

unsafe fn install_print(ctx: *mut qjs::JSContext) {
    unsafe extern "C" fn qjs_print(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        log_str("qjs: ");
        if !argv.is_null() && argc > 0 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            for (i, arg) in args.iter().enumerate() {
                if i != 0 {
                    log_str(" ");
                }
                let cstr = qjs::js_to_cstring(ctx, *arg);
                if cstr.is_null() {
                    log_str("<toString failed>");
                    continue;
                }
                let bytes = CStr::from_ptr(cstr).to_bytes();
                if let Ok(s) = core::str::from_utf8(bytes) {
                    log_str(s);
                } else {
                    log_bytes(bytes);
                }
                qjs::JS_FreeCString(ctx, cstr);
            }
        }
        log_nl();
        qjs::JSValue::undefined()
    }

    let global = qjs::JS_GetGlobalObject(ctx);
    let name = b"print\0";
    let func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_print),
        name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, name.as_ptr() as *const c_char, func);
    qjs::js_free_value(ctx, global);
}

unsafe fn js_make_complex(ctx: *mut qjs::JSContext, re: f64, im: f64) -> qjs::JSValue {
    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return obj;
    }

    let re_name = b"re\0";
    let im_name = b"im\0";

    let re_val = unsafe { qjs::JS_NewFloat64(ctx, re) };
    if qjs::JS_SetPropertyStr(ctx, obj, re_name.as_ptr() as *const c_char, re_val) < 0 {
        qjs::js_free_value(ctx, obj);
        return qjs::JSValue::exception();
    }

    let im_val = unsafe { qjs::JS_NewFloat64(ctx, im) };
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

unsafe extern "C" fn qjs_module_loader(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut qjs::JSModuleDef {
    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let name = CStr::from_ptr(module_name).to_bytes();
    if name != b"complex" {
        return core::ptr::null_mut();
    }

    let m = qjs::JS_NewCModule(ctx, module_name, Some(qjs_complex_module_init));
    if m.is_null() {
        return core::ptr::null_mut();
    }

    let make_name = b"make\0";
    let add_name = b"add\0";
    let square_name = b"square\0";
    let _ = qjs::JS_AddModuleExport(ctx, m, make_name.as_ptr() as *const c_char);
    let _ = qjs::JS_AddModuleExport(ctx, m, add_name.as_ptr() as *const c_char);
    let _ = qjs::JS_AddModuleExport(ctx, m, square_name.as_ptr() as *const c_char);

    m
}

/// TRUEOS kernel QuickJS smoke test:
/// - Installs a minimal `print()` bridge.
/// - Installs a module loader that serves a native `complex` module.
/// - Evaluates an ES module that imports `complex` and asserts add/square results.
pub unsafe fn run() {
    let rt = qjs::JS_NewRuntime();
    if rt.is_null() {
        log_str("quickjs: JS_NewRuntime failed\n");
        return;
    }

    // Enable ES module loading (imports) by installing a module loader.
    qjs::JS_SetModuleLoaderFunc(rt, None, Some(qjs_module_loader), core::ptr::null_mut());

    let ctx = qjs::JS_NewContext(rt);
    if ctx.is_null() {
        log_str("quickjs: JS_NewContext failed\n");
        qjs::JS_FreeRuntime(rt);
        return;
    }

    install_print(ctx);

    let mod_filename = b"<smoke-module>\0";
    let mod_script = b"import { make, add, square } from 'complex';\n\
+const a = make(3, 4);\n\
+const b = make(1, 2);\n\
+const s = add(a, b);\n\
+if (s.re !== 4 || s.im !== 6) throw new Error('complex add failed');\n\
+const q = square(a);\n\
+if (q.re !== -7 || q.im !== 24) throw new Error('complex square failed');\n\
+globalThis.print('complex ok', s.re, s.im, q.re, q.im);\n\
+0\n\0";

    let mod_ret = qjs::JS_Eval(
        ctx,
        mod_script.as_ptr() as *const c_char,
        mod_script.len() - 1,
        mod_filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_MODULE,
    );

    if mod_ret.is_exception() {
        log_str("quickjs: module JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        qjs::js_free_value(ctx, mod_ret);
        log_str("quickjs: module eval ok\n");
    }

    // Keep the original minimal global eval as a baseline sanity check.
    let filename = b"<smoke>\0";
    let script = b"print('hello from quickjs'); 1 + 1\0";
    let ret = qjs::JS_Eval(
        ctx,
        script.as_ptr() as *const c_char,
        script.len() - 1,
        filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );

    if ret.is_exception() {
        log_str("quickjs: JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        let out = qjs::js_to_cstring(ctx, ret);
        if !out.is_null() {
            let bytes = CStr::from_ptr(out).to_bytes();
            log_str("quickjs: eval ok => ");
            if let Ok(s) = core::str::from_utf8(bytes) {
                log_str(s);
            } else {
                log_bytes(bytes);
            }
            log_nl();
            qjs::JS_FreeCString(ctx, out);
        } else {
            log_str("quickjs: eval ok (toString failed)\n");
        }
    }

    qjs::js_free_value(ctx, ret);

    log_str("quickjs: runtime/context ok\n");
    qjs::JS_FreeContext(ctx);
    qjs::JS_FreeRuntime(rt);
}
