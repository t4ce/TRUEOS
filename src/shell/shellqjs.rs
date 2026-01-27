use alloc::{string::String, vec::Vec};
use core::ffi::{c_char, c_int, CStr};

use crate::shell::{ShellBackend, ShellIo};
use crate::disc::files::Fs;

static mut QJS_SHELL_IO: Option<&'static dyn ShellBackend> = None;

const QJS_FS_BASE_DIR: &str = "/qjs";
const QJS_CDN_DIR: &str = "/qjs/cdn";
const ESM_SH_PREFIX: &str = "https://esm.sh/";

extern "C" {
    fn trueos_cabi_net_fetch_to_file(url_ptr: *const u8, url_len: usize, path_ptr: *const u8, path_len: usize) -> i32;
}

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

fn spec_is_url(s: &str) -> bool {
    s.starts_with("https://") || s.starts_with("http://")
}

fn split_url(s: &str) -> Option<(&str, &str, &str)> {
    let (scheme, rest) = if let Some(r) = s.strip_prefix("https://") {
        ("https://", r)
    } else if let Some(r) = s.strip_prefix("http://") {
        ("http://", r)
    } else {
        return None;
    };

    let (authority, path) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return None;
    }
    Some((scheme, authority, path))
}

fn url_origin_prefix(s: &str) -> Option<String> {
    let (scheme, authority, _path) = split_url(s)?;
    let mut out = String::new();
    out.push_str(scheme);
    out.push_str(authority);
    Some(out)
}

fn normalize_path_str(path: &str) -> String {
    let is_abs = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                let _ = parts.pop();
            }
            _ => parts.push(seg),
        }
    }
    let mut out = String::new();
    if is_abs {
        out.push('/');
    }
    for (i, seg) in parts.iter().enumerate() {
        if i != 0 {
            out.push('/');
        }
        out.push_str(seg);
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

fn normalize_url_str(url: &str) -> Option<String> {
    let (scheme, authority, path) = split_url(url)?;
    let norm_path = normalize_path_str(path);
    let mut out = String::new();
    out.push_str(scheme);
    out.push_str(authority);
    if !norm_path.starts_with('/') {
        out.push('/');
    }
    out.push_str(norm_path.as_str());
    Some(out)
}

fn resolve_relative_url_str(base_url: &str, rel: &str) -> Option<String> {
    let (scheme, authority, path) = split_url(base_url)?;
    let base_dir = match path.rfind('/') {
        Some(0) => "/",
        Some(i) => &path[..i],
        None => "/",
    };
    let mut combined = String::new();
    combined.push_str(scheme);
    combined.push_str(authority);
    if !base_dir.starts_with('/') {
        combined.push('/');
    }
    combined.push_str(base_dir);
    if !combined.ends_with('/') {
        combined.push('/');
    }
    combined.push_str(rel);
    normalize_url_str(combined.as_str())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn hex_u64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for i in (0..16).rev() {
        let nibble = ((v >> (i * 4)) & 0xF) as usize;
        out.push(HEX[nibble] as char);
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

    // URL imports: keep absolute URLs as-is; resolve relative URLs against URL base.
    if spec_is_url(name) {
        if let Some(normalized) = normalize_url_str(name) {
            return js_alloc_cstring(ctx, normalized.as_str());
        }
        return js_alloc_cstring(ctx, name);
    }

    // If base is a URL, treat "/foo" as an origin-relative URL path.
    if name.starts_with('/') && !module_base_name.is_null() {
        let base_bytes = CStr::from_ptr(module_base_name).to_bytes();
        if let Ok(base) = core::str::from_utf8(base_bytes) {
            if spec_is_url(base) {
                if let Some(mut origin) = url_origin_prefix(base) {
                    let norm_path = normalize_path_str(name);
                    origin.push_str(norm_path.as_str());
                    return js_alloc_cstring(ctx, origin.as_str());
                }
            }
        }
    }

    if (name.starts_with("./") || name.starts_with("../")) && !module_base_name.is_null() {
        let base_bytes = CStr::from_ptr(module_base_name).to_bytes();
        if let Ok(base) = core::str::from_utf8(base_bytes) {
            if spec_is_url(base) {
                if let Some(resolved) = resolve_relative_url_str(base, name) {
                    return js_alloc_cstring(ctx, resolved.as_str());
                }
            }
        }
    }

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
        // Bare specifiers resolve through esm.sh to avoid implementing npm resolution.
        let mut u = String::new();
        u.push_str(ESM_SH_PREFIX);
        u.push_str(name);
        u
    };

    // Only auto-append ".mjs" for filesystem paths.
    let normalized = if normalized.starts_with('/') {
        if has_extension(&normalized) {
            normalized
        } else {
            let mut p = normalized;
            p.push_str(".mjs");
            p
        }
    } else {
        normalized
    };

    js_alloc_cstring(ctx, normalized.as_str())
}

unsafe extern "C" fn trueos_module_loader(
    ctx: *mut trueos_qjs::JSContext,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut trueos_qjs::JSModuleDef {
    fn push_i32_dec(out: &mut String, v: i32) {
        if v == 0 {
            out.push('0');
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
            out.push('-');
        }
        for &b in &buf[i..] {
            out.push(b as char);
        }
    }

    unsafe fn throw_error(ctx: *mut trueos_qjs::JSContext, msg: &str) {
        let err = trueos_qjs::JS_NewError(ctx);
        if err.is_exception() {
            return;
        }

        let m = trueos_qjs::JS_NewStringLen(ctx, msg.as_ptr() as *const c_char, msg.len());
        let _ = trueos_qjs::JS_SetPropertyStr(ctx, err, b"message\0".as_ptr() as *const c_char, m);
        let _ = trueos_qjs::JS_Throw(ctx, err);
    }

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

    if spec_is_url(path) {
        let hash = fnv1a64(path.as_bytes());
        let mut cache_path = String::new();
        cache_path.push_str(QJS_CDN_DIR);
        cache_path.push('/');
        cache_path.push_str(hex_u64(hash).as_str());
        cache_path.push_str(".mjs");

        let rc = trueos_cabi_net_fetch_to_file(
            path.as_bytes().as_ptr(),
            path.as_bytes().len(),
            cache_path.as_bytes().as_ptr(),
            cache_path.as_bytes().len(),
        );
        if rc != 0 {
            let mut msg = String::new();
            msg.push_str("fetch-to-cache failed rc=");
            push_i32_dec(&mut msg, rc);
            msg.push_str(" url=");
            msg.push_str(path);
            msg.push_str(" cache=");
            msg.push_str(cache_path.as_str());
            throw_error(ctx, msg.as_str());
            return core::ptr::null_mut();
        }

        let bytes = match Fs::read_file(cache_path.as_str()) {
            Ok(b) => b,
            Err(_) => {
                let mut msg = String::new();
                msg.push_str("read cached module failed url=");
                msg.push_str(path);
                msg.push_str(" cache=");
                msg.push_str(cache_path.as_str());
                throw_error(ctx, msg.as_str());
                return core::ptr::null_mut();
            }
        };

        // Compile using the URL as the module "filename" so base URL resolution works.
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
        return m;
    }

    if !path.starts_with('/') {
        let mut msg = String::new();
        msg.push_str("unsupported module specifier: ");
        msg.push_str(path);
        throw_error(ctx, msg.as_str());
        return core::ptr::null_mut();
    }

    let bytes = match Fs::read_file(path) {
        Ok(b) => b,
        Err(_) => {
            let mut msg = String::new();
            msg.push_str("read module failed path=");
            msg.push_str(path);
            throw_error(ctx, msg.as_str());
            return core::ptr::null_mut();
        }
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
