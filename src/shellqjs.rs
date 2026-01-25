use core::ffi::{c_char, c_int, CStr};

use crate::shell::{ShellBackend, ShellIo};

static mut QJS_SHELL_IO: Option<&'static dyn ShellBackend> = None;

unsafe extern "C" fn qjs_shell_print(
    ctx: *mut trueos_qjs::JSContext,
    _this_val: trueos_qjs::JSValueConst,
    argc: c_int,
    argv: *const trueos_qjs::JSValueConst,
) -> trueos_qjs::JSValue {
    let io = match unsafe { QJS_SHELL_IO } {
        Some(io) => io,
        None => return trueos_qjs::JSValue::undefined(),
    };

    io.write_str("qjs: ");
    if !argv.is_null() && argc > 0 {
        let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
        for (i, arg) in args.iter().enumerate() {
            if i != 0 {
                io.write_str(" ");
            }
            let cstr = unsafe { trueos_qjs::js_to_cstring(ctx, *arg) };
            if cstr.is_null() {
                io.write_str("<toString failed>");
                continue;
            }
            let bytes = unsafe { CStr::from_ptr(cstr).to_bytes() };
            if let Ok(s) = core::str::from_utf8(bytes) {
                io.write_str(s);
            } else {
                for &b in bytes {
                    io.write_byte(b);
                }
            }
            unsafe { trueos_qjs::JS_FreeCString(ctx, cstr) };
        }
    }
    io.write_str("\r\n");
    trueos_qjs::JSValue::undefined()
}

fn dump_exception(io: &dyn ShellIo, ctx: *mut trueos_qjs::JSContext) {
    unsafe {
        let exc = trueos_qjs::JS_GetException(ctx);
        let cstr = trueos_qjs::js_to_cstring(ctx, exc);

        io.write_str("qjs: exception: ");
        if !cstr.is_null() {
            let bytes = CStr::from_ptr(cstr).to_bytes();
            if let Ok(s) = core::str::from_utf8(bytes) {
                io.write_str(s);
            } else {
                for &b in bytes {
                    io.write_byte(b);
                }
            }
            trueos_qjs::JS_FreeCString(ctx, cstr);
        } else {
            io.write_str("<toString failed>");
        }
        io.write_str("\r\n");

        trueos_qjs::js_free_value(ctx, exc);
    }
}

pub(crate) fn looks_like_module_bytes(bytes: &[u8]) -> bool {
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            _ => break,
        }
    }
    let rest = &bytes[i..];

    fn is_import_suffix(b: u8) -> bool {
        matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'*' | b'(' | b'\'' | b'"')
    }
    fn is_export_suffix(b: u8) -> bool {
        matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'*')
    }

    if rest.starts_with(b"import") {
        return rest.get(6).copied().map(is_import_suffix).unwrap_or(true);
    }
    if rest.starts_with(b"export") {
        return rest.get(6).copied().map(is_export_suffix).unwrap_or(true);
    }
    false
}

pub(crate) fn eval_bytes(
    io: &'static dyn ShellBackend,
    filename: *const c_char,
    bytes: &[u8],
    eval_flags: c_int,
) {
    unsafe {
        let rt = trueos_qjs::JS_NewRuntime();
        if rt.is_null() {
            io.write_str("qjs: JS_NewRuntime failed\r\n");
            return;
        }

        // Enable ES module loading (imports) by installing the TRUEOS module loader.
        trueos_qjs::trueos_modules::install(rt);

        let ctx = trueos_qjs::JS_NewContext(rt);
        if ctx.is_null() {
            trueos_qjs::JS_FreeRuntime(rt);
            io.write_str("qjs: JS_NewContext failed\r\n");
            return;
        }

        QJS_SHELL_IO = Some(io);

        // Install globalThis.print(...)
        let global = trueos_qjs::JS_GetGlobalObject(ctx);
        let name = b"print\0";
        let func = trueos_qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_shell_print),
            name.as_ptr() as *const c_char,
            1,
            trueos_qjs::JS_CFUNC_GENERIC,
            0,
        );
        let _ = trueos_qjs::JS_SetPropertyStr(ctx, global, name.as_ptr() as *const c_char, func);
        trueos_qjs::js_free_value(ctx, global);

        let val = trueos_qjs::JS_Eval(
            ctx,
            bytes.as_ptr() as *const c_char,
            bytes.len(),
            filename,
            eval_flags,
        );

        if val.is_exception() {
            dump_exception(io, ctx);
        } else {
            // Print return value unless it's `undefined`.
            if val.tag != trueos_qjs::JS_TAG_UNDEFINED {
                let cstr = trueos_qjs::js_to_cstring(ctx, val);
                io.write_str("qjs: => ");
                if !cstr.is_null() {
                    let bytes = CStr::from_ptr(cstr).to_bytes();
                    if let Ok(s) = core::str::from_utf8(bytes) {
                        io.write_str(s);
                    } else {
                        for &b in bytes {
                            io.write_byte(b);
                        }
                    }
                    trueos_qjs::JS_FreeCString(ctx, cstr);
                } else {
                    io.write_str("<toString failed>");
                }
                io.write_str("\r\n");
            }
        }
        trueos_qjs::js_free_value(ctx, val);

        QJS_SHELL_IO = None;

        trueos_qjs::JS_FreeContext(ctx);
        trueos_qjs::JS_FreeRuntime(rt);
    }
}

pub(crate) fn eval(io: &'static dyn ShellBackend, source: &str) {
    let bytes = source.as_bytes();
    let is_module = looks_like_module_bytes(bytes);
    let filename = if is_module {
        b"<shell-module>\0".as_ptr() as *const c_char
    } else {
        b"<shell>\0".as_ptr() as *const c_char
    };
    let flags = if is_module {
        trueos_qjs::JS_EVAL_TYPE_MODULE
    } else {
        trueos_qjs::JS_EVAL_TYPE_GLOBAL
    };
    eval_bytes(io, filename, bytes, flags);
}

pub(crate) fn eval_module(io: &'static dyn ShellBackend, source: &str) {
    let filename = b"<shell-module>\0".as_ptr() as *const c_char;
    eval_bytes(io, filename, source.as_bytes(), trueos_qjs::JS_EVAL_TYPE_MODULE);
}
