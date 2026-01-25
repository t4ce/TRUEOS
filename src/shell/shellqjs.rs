use alloc::{string::String, vec::Vec};
use core::ffi::{c_char, c_int, CStr};

use crate::shell::{ShellBackend, ShellIo};
use crate::disc::files::Fs;

static mut QJS_SHELL_IO: Option<&'static dyn ShellBackend> = None;

const QJS_FS_BASE_DIR: &str = "/qjs";

fn has_extension(path: &str) -> bool {
    path.rsplit('/').next().map(|s| s.contains('.')).unwrap_or(false)
}

fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(i) => &path[..i],
        None => QJS_FS_BASE_DIR,
    }
}

fn normalize_join(base_dir: &str, rel: &str) -> String {
    // Build normalized absolute path by resolving '.' and '..'.
    let mut parts: Vec<&str> = Vec::new();

    for p in base_dir.split('/') {
        if !p.is_empty() {
            parts.push(p);
        }
    }
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                let _ = parts.pop();
            }
            _ => parts.push(p),
        }
    }

    let mut out = String::new();
    out.push('/');
    for (i, p) in parts.iter().enumerate() {
        out.push_str(p);
        if i + 1 != parts.len() {
            out.push('/');
        }
    }
    out
}

unsafe fn js_alloc_cstring(ctx: *mut trueos_qjs::JSContext, s: &str) -> *mut c_char {
    let bytes = s.as_bytes();
    let buf = trueos_qjs::js_malloc(ctx, bytes.len() + 1) as *mut u8;
    if buf.is_null() {
        return core::ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
    *buf.add(bytes.len()) = 0;
    buf as *mut c_char
}

unsafe extern "C" fn trueos_module_normalize(
    ctx: *mut trueos_qjs::JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut c_char {
    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let name_bytes = CStr::from_ptr(module_name).to_bytes();
    let Ok(name) = core::str::from_utf8(name_bytes) else {
        return core::ptr::null_mut();
    };

    // Keep TRUEOS-provided native modules as-is.
    if name == "complex" || name == "fs" {
        return js_alloc_cstring(ctx, name);
    }

    // Map bare specifiers to /qjs/<name>.mjs
    // Map relative specifiers against the base module path.
    let normalized = if name.starts_with('/') {
        String::from(name)
    } else if name.starts_with("./") || name.starts_with("../") {
        let base_dir = if module_base_name.is_null() {
            QJS_FS_BASE_DIR
        } else {
            let base_bytes = CStr::from_ptr(module_base_name).to_bytes();
            match core::str::from_utf8(base_bytes) {
                Ok(base) if base.starts_with('/') => dir_of(base),
                _ => QJS_FS_BASE_DIR,
            }
        };
        normalize_join(base_dir, name)
    } else {
        let mut p = String::new();
        p.push_str(QJS_FS_BASE_DIR);
        p.push('/');
        p.push_str(name);
        p
    };

    let normalized = if has_extension(&normalized) {
        normalized
    } else {
        let mut p = normalized;
        p.push_str(".mjs");
        p
    };

    js_alloc_cstring(ctx, normalized.as_str())
}

unsafe extern "C" fn trueos_module_loader(
    ctx: *mut trueos_qjs::JSContext,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut trueos_qjs::JSModuleDef {
    // First, try built-in native modules (e.g. "complex").
    let native = trueos_qjs::trueos_modules::load_native_module(ctx, module_name);
    if !native.is_null() {
        return native;
    }

    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let name_bytes = CStr::from_ptr(module_name).to_bytes();
    let Ok(path) = core::str::from_utf8(name_bytes) else {
        return core::ptr::null_mut();
    };
    if !path.starts_with('/') {
        return core::ptr::null_mut();
    }

    let bytes = match Fs::read_file(path) {
        Ok(b) => b,
        Err(_) => return core::ptr::null_mut(),
    };

    let val = trueos_qjs::JS_Eval(
        ctx,
        bytes.as_ptr() as *const c_char,
        bytes.len(),
        module_name,
        trueos_qjs::JS_EVAL_TYPE_MODULE | trueos_qjs::JS_EVAL_FLAG_COMPILE_ONLY,
    );
    if val.is_exception() {
        return core::ptr::null_mut();
    }

    let m = val.u.ptr as *mut trueos_qjs::JSModuleDef;
    trueos_qjs::js_free_value(ctx, val);
    m
}

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

        // Enable ES module loading (imports): native modules + filesystem modules under /qjs.
        trueos_qjs::JS_SetModuleLoaderFunc(
            rt,
            Some(trueos_module_normalize),
            Some(trueos_module_loader),
            core::ptr::null_mut(),
        );

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

pub(crate) fn eval_module(io: &'static dyn ShellBackend, source: &str) {
    let filename = b"<shell-module>\0".as_ptr() as *const c_char;
    eval_bytes(io, filename, source.as_bytes(), trueos_qjs::JS_EVAL_TYPE_MODULE);
}
