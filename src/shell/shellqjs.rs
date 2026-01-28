use alloc::{string::String, vec::Vec};
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
    fn i64_to_dec(buf: &mut [u8; 24], mut v: i64) -> &str {
        if v == 0 {
            buf[0] = b'0';
            return unsafe { core::str::from_utf8_unchecked(&buf[..1]) };
        }

        let neg = v < 0;
        if neg {
            // Avoid overflow on i64::MIN by doing the conversion in i128.
            let vv = -(v as i128);
            let mut n = vv as u128;
            let mut i = buf.len();
            while n != 0 {
                i -= 1;
                buf[i] = b'0' + (n % 10) as u8;
                n /= 10;
            }
            i -= 1;
            buf[i] = b'-';
            return unsafe { core::str::from_utf8_unchecked(&buf[i..]) };
        }

        let mut n = v as u64;
        let mut i = buf.len();
        while n != 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        unsafe { core::str::from_utf8_unchecked(&buf[i..]) }
    }

    unsafe fn write_js_string(io: &dyn ShellIo, ctx: *mut trueos_qjs::JSContext, val: trueos_qjs::JSValueConst) -> bool {
        let cstr = trueos_qjs::js_to_cstring(ctx, val);
        if cstr.is_null() {
            return false;
        }

        let bytes = CStr::from_ptr(cstr).to_bytes();
        if let Ok(s) = core::str::from_utf8(bytes) {
            io.write_str(s);
        } else {
            for &b in bytes {
                io.write_byte(b);
            }
        }
        trueos_qjs::JS_FreeCString(ctx, cstr);
        true
    }

    unsafe fn dump_prop(io: &dyn ShellIo, ctx: *mut trueos_qjs::JSContext, obj: trueos_qjs::JSValueConst, key: &[u8], label: &str) {
        let v = trueos_qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
        if v.is_exception() {
            let exc2 = trueos_qjs::JS_GetException(ctx);
            io.write_str("qjs: exception while reading ");
            io.write_str(label);
            io.write_str(": ");
            if !write_js_string(io, ctx, exc2) {
                io.write_str("<toString failed>");
            }
            io.write_str("\r\n");
            trueos_qjs::js_free_value(ctx, exc2);
            return;
        }

        if v.tag != trueos_qjs::JS_TAG_UNDEFINED {
            io.write_str("qjs: ");
            io.write_str(label);
            io.write_str(": ");
            if !write_js_string(io, ctx, v) {
                io.write_str("<toString failed>");
                io.write_str(" (tag=");
                let mut buf = [0u8; 24];
                io.write_str(i64_to_dec(&mut buf, v.tag));
                io.write_str(")");
            }
            io.write_str("\r\n");
        }
        trueos_qjs::js_free_value(ctx, v);
    }

    unsafe {
        let exc = trueos_qjs::JS_GetException(ctx);
        io.write_str("qjs: exception: ");
        let _ = write_js_string(io, ctx, exc);

        io.write_str(" (tag=");
        let mut buf = [0u8; 24];
        io.write_str(i64_to_dec(&mut buf, exc.tag));
        io.write_str(")");
        io.write_str("\r\n");

        // Try to print useful fields. QuickJS will ToObject() primitives like string/number.
        if exc.tag != trueos_qjs::JS_TAG_NULL && exc.tag != trueos_qjs::JS_TAG_UNDEFINED {
            dump_prop(io, ctx, exc, b"name\0", "name");
            dump_prop(io, ctx, exc, b"message\0", "message");
            dump_prop(io, ctx, exc, b"stack\0", "stack");
        }

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

pub(crate) fn looks_like_module_src(src: &str) -> bool {
    // Fast path: trim leading whitespace and check for a leading keyword.
    let s = src.trim_start_matches(|c: char| c.is_whitespace());
    if s.starts_with("import") {
        let b = s.as_bytes();
        if b.len() == 6 {
            return true;
        }
        if let Some(&next) = b.get(6) {
            if matches!(next, b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'*' | b'(' | b'\'' | b'"') {
                return true;
            }
        }
    }
    if s.starts_with("export") {
        let b = s.as_bytes();
        if b.len() == 6 {
            return true;
        }
        if let Some(&next) = b.get(6) {
            if matches!(next, b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'*') {
                return true;
            }
        }
    }

    // Heuristic scan for `import`/`export` outside comments and strings.
    // This intentionally avoids full JS parsing; it’s enough to streamline the shell UX.
    #[derive(Copy, Clone, PartialEq, Eq)]
    enum Mode {
        Code,
        LineComment,
        BlockComment,
        SingleQuote,
        DoubleQuote,
        Template,
    }

    fn is_ident(b: u8) -> bool {
        b == b'_' || b == b'$' || b.is_ascii_alphanumeric()
    }

    fn is_keyword_at(hay: &[u8], i: usize, kw: &[u8]) -> bool {
        if i + kw.len() > hay.len() {
            return false;
        }
        if &hay[i..i + kw.len()] != kw {
            return false;
        }
        if i > 0 {
            let prev = hay[i - 1];
            if is_ident(prev) {
                return false;
            }
        }
        if i + kw.len() < hay.len() {
            let next = hay[i + kw.len()];
            if is_ident(next) {
                return false;
            }
        }
        true
    }

    let bytes = src.as_bytes();
    let mut i = 0usize;
    let mut mode = Mode::Code;

    while i < bytes.len() {
        let b = bytes[i];
        match mode {
            Mode::Code => {
                // Start of comment?
                if b == b'/' {
                    if i + 1 < bytes.len() {
                        match bytes[i + 1] {
                            b'/' => {
                                mode = Mode::LineComment;
                                i += 2;
                                continue;
                            }
                            b'*' => {
                                mode = Mode::BlockComment;
                                i += 2;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }

                // Start of string?
                if b == b'\'' {
                    mode = Mode::SingleQuote;
                    i += 1;
                    continue;
                }
                if b == b'"' {
                    mode = Mode::DoubleQuote;
                    i += 1;
                    continue;
                }
                if b == b'`' {
                    mode = Mode::Template;
                    i += 1;
                    continue;
                }

                // Keywords.
                if b == b'i' && is_keyword_at(bytes, i, b"import") {
                    return true;
                }
                if b == b'e' && is_keyword_at(bytes, i, b"export") {
                    return true;
                }
                i += 1;
            }
            Mode::LineComment => {
                if b == b'\n' {
                    mode = Mode::Code;
                }
                i += 1;
            }
            Mode::BlockComment => {
                if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    mode = Mode::Code;
                    i += 2;
                    continue;
                }
                i += 1;
            }
            Mode::SingleQuote => {
                if b == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if b == b'\'' {
                    mode = Mode::Code;
                }
                i += 1;
            }
            Mode::DoubleQuote => {
                if b == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if b == b'"' {
                    mode = Mode::Code;
                }
                i += 1;
            }
            Mode::Template => {
                if b == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if b == b'`' {
                    mode = Mode::Code;
                }
                // We intentionally treat `${ ... }` as opaque here.
                i += 1;
            }
        }
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

        // Enable ES module loading (imports) using the shared TRUEOS module loader
        // (same approach as the in-kernel QuickJS smoke test).
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
    let is_module = looks_like_module_src(source);
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
