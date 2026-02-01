use alloc::vec::Vec;
use core::ffi::{c_char, c_int, CStr};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::shell::{ShellBackend, ShellIo};

#[repr(C)]
struct QjsShellOpaque {
    io: &'static dyn ShellBackend,
}

unsafe extern "C" fn qjs_shell_print(
    ctx: *mut trueos_qjs::JSContext,
    _this_val: trueos_qjs::JSValueConst,
    argc: c_int,
    argv: *const trueos_qjs::JSValueConst,
) -> trueos_qjs::JSValue {
    let opaque = unsafe { trueos_qjs::JS_GetContextOpaque(ctx) } as *const QjsShellOpaque;
    let io = if opaque.is_null() {
        return trueos_qjs::JSValue::undefined();
    } else {
        unsafe { (*opaque).io }
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
    fn i64_to_dec(buf: &mut [u8; 24], v: i64) -> &str {
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

unsafe fn drain_pending_jobs(io: &dyn ShellIo, rt: *mut trueos_qjs::JSRuntime, fallback_ctx: *mut trueos_qjs::JSContext) -> bool {
    if rt.is_null() {
        return true;
    }

    loop {
        let mut job_ctx: *mut trueos_qjs::JSContext = core::ptr::null_mut();
        let rc = trueos_qjs::JS_ExecutePendingJob(rt, &mut job_ctx as *mut *mut trueos_qjs::JSContext);
        if rc > 0 {
            continue;
        }
        if rc < 0 {
            let ctx = if !job_ctx.is_null() { job_ctx } else { fallback_ctx };
            if !ctx.is_null() {
                dump_exception(io, ctx);
            } else {
                io.write_str("qjs: exception while executing pending job (no ctx)\r\n");
            }
            return false;
        }
        break;
    }

    true
}

async fn drain_jobs_and_promises(
    io: &dyn ShellIo,
    rt: *mut trueos_qjs::JSRuntime,
    ctx: *mut trueos_qjs::JSContext,
    max_wait_ms: u64,
) -> bool {
    if rt.is_null() || ctx.is_null() {
        return true;
    }

    let start = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ as u64;
    let max_ticks = if hz == 0 {
        0
    } else {
        (max_wait_ms.saturating_mul(hz) + 999) / 1000
    };
    let deadline = start.saturating_add(max_ticks);

    loop {
        let _progress = unsafe { trueos_qjs::async_ops::pump(ctx) };

        // Drain QuickJS microtasks (Promise continuations).
        if !unsafe { drain_pending_jobs(io, rt, ctx) } {
            return false;
        }

        let pending = unsafe { trueos_qjs::async_ops::has_pending(ctx) };
        let jobs_pending = unsafe { trueos_qjs::JS_IsJobPending(rt) } > 0;
        if !pending && !jobs_pending {
            break;
        }

        if max_ticks != 0 && embassy_time_driver::now() >= deadline {
            io.write_str("qjs: async wait timeout\r\n");
            break;
        }

        Timer::after(EmbassyDuration::from_millis(1)).await;
    }

    true
}

fn split_first_token(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    if s.is_empty() {
        return ("", "");
    }
    let mut it = s.splitn(2, char::is_whitespace);
    let first = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");
    (first, rest)
}

pub(crate) fn help(io: &dyn ShellIo) {
    io.write_str("qjs: usage\r\n");
    io.write_str("qjs <javascript>\r\n");
    io.write_str("qjs @<path>\r\n");
    io.write_str("qjs <path.js|path.mjs>\r\n");
    io.write_str("qjs -e <javascript>\r\n");
    io.write_str("qjs -m -e <module javascript>\r\n");
    io.write_str("qjs -p <expr>\r\n");
    io.write_str("qjs: notes\r\n");
    io.write_str("- auto-detects modules (import/export/import.meta) unless forced\r\n");
    io.write_str("- drains the QuickJS job queue (Promises) after eval\r\n");
    io.write_str("- does not print `=> ...` by default; use -p or print()\r\n");
    io.write_str("qjs: examples\r\n");
    io.write_str("qjs print(1+2)\r\n");
    io.write_str("qjs -m -e import leftPad from 'left-pad@1.3.0'; print(leftPad('a',3,'.'));\r\n");
    io.write_str("qjs -m -e import * as path from 'path'; print(path.join('a','b'));\r\n");
}

pub(crate) async fn run(io: &'static dyn ShellBackend, src: &str) {
    let src = src.trim();
    if src.is_empty() {
        help(io);
        return;
    }

    // Parse a small subset of upstream-like flags.
    // NOTE: Our shell receives the rest of the line as a single string; for -e/-p
    // we consume "the rest of the line" as the code/expression.
    let mut rest = src;
    let mut forced_flags: Option<c_int> = None;
    let mut suppress_result = false;
    let mut explicit_print = false;
    let mut code: Option<&str> = None;
    let mut file: Option<&str> = None;

    // Only treat leading '-' tokens as options.
    loop {
        let (tok, r) = split_first_token(rest);
        if tok.is_empty() {
            break;
        }
        if !tok.starts_with('-') {
            break;
        }
        match tok {
            "-h" | "--help" => {
                help(io);
                return;
            }
            "-m" => {
                forced_flags = Some(trueos_qjs::JS_EVAL_TYPE_MODULE);
                rest = r;
            }
            "-g" => {
                forced_flags = Some(trueos_qjs::JS_EVAL_TYPE_GLOBAL);
                rest = r;
            }
            "-q" => {
                suppress_result = true;
                rest = r;
            }
            "-e" => {
                code = Some(r.trim_start());
                rest = "";
                break;
            }
            "-p" => {
                code = Some(r.trim_start());
                explicit_print = true;
                rest = "";
                break;
            }
            "--" => {
                code = Some(r.trim_start());
                rest = "";
                break;
            }
            _ => {
                // Unknown option: fall back to treating the full input as JS source.
                break;
            }
        }
    }

    let remaining = if code.is_some() { "" } else { rest.trim_start() };

    if code.is_none() {
        if let Some(path) = remaining.strip_prefix('@') {
            let path = path.trim();
            if !path.is_empty() {
                file = Some(path);
            }
        } else {
            // Upstream `qjs` takes a filename argument. We support that too when it looks like a path.
            let single = !remaining.is_empty() && !remaining.contains(char::is_whitespace);
            let looks_like_path = remaining.starts_with('/') || remaining.starts_with("./") || remaining.starts_with("../") || remaining.ends_with(".js") || remaining.ends_with(".mjs");
            if single && looks_like_path {
                if matches!(crate::surface::io::kfs::exists_async(remaining).await, Ok(true)) {
                    file = Some(remaining);
                }
            }

            if file.is_none() {
                code = Some(remaining);
            }
        }
    }

    if let Some(path) = file {
        match crate::surface::io::kfs::read_file_async(path).await {
            Ok(bytes) => {
                let flags = forced_flags.unwrap_or_else(|| {
                    if path.ends_with(".mjs") || looks_like_module_bytes(&bytes) {
                        trueos_qjs::JS_EVAL_TYPE_MODULE
                    } else {
                        trueos_qjs::JS_EVAL_TYPE_GLOBAL
                    }
                });

                let print_result = if suppress_result { false } else { explicit_print };

                let mut filename_buf: Vec<u8> = Vec::with_capacity(path.len() + 1);
                filename_buf.extend_from_slice(path.as_bytes());
                filename_buf.push(0);
                eval_bytes_opts_async(io, filename_buf.as_ptr() as *const c_char, &bytes, flags, print_result).await;
            }
            Err(e) => io.write_fmt(format_args!("qjs: read_file failed ({:?})\r\n", e)),
        }
        return;
    }

    let Some(source) = code else {
        help(io);
        return;
    };

    let flags = forced_flags.unwrap_or_else(|| {
        if looks_like_module_src(source) {
            trueos_qjs::JS_EVAL_TYPE_MODULE
        } else {
            trueos_qjs::JS_EVAL_TYPE_GLOBAL
        }
    });

    let print_result = if suppress_result { false } else { explicit_print };

    let filename = if flags == trueos_qjs::JS_EVAL_TYPE_MODULE {
        b"<eval-module>\0".as_ptr() as *const c_char
    } else {
        b"<eval>\0".as_ptr() as *const c_char
    };
    eval_bytes_opts_async(io, filename, source.as_bytes(), flags, print_result).await;
}

pub(crate) fn eval_bytes(
    io: &'static dyn ShellBackend,
    filename: *const c_char,
    bytes: &[u8],
    eval_flags: c_int,
) {
    eval_bytes_opts(io, filename, bytes, eval_flags, true);
}

pub(crate) fn eval_bytes_opts(
    io: &'static dyn ShellBackend,
    filename: *const c_char,
    bytes: &[u8],
    eval_flags: c_int,
    print_result: bool,
) {
    unsafe {
        let rt = trueos_qjs::JS_NewRuntime();
        if rt.is_null() {
            io.write_str("qjs: JS_NewRuntime failed\r\n");
            return;
        }

        // Enable ES module loading (imports) using the TRUEOS Node-enhanced module loader.
        // This keeps the existing URL/FS/native loader behavior, but improves compatibility
        // with npm packages that expect Node-ish specifier resolution (e.g. `path`, `node:fs`).
        trueos_qjs::node::install(rt);

        let ctx = trueos_qjs::JS_NewContext(rt);
        if ctx.is_null() {
            trueos_qjs::JS_FreeRuntime(rt);
            io.write_str("qjs: JS_NewContext failed\r\n");
            return;
        }

        let opaque = Box::new(QjsShellOpaque { io });
        let opaque_ptr = Box::into_raw(opaque);
        trueos_qjs::JS_SetContextOpaque(ctx, opaque_ptr as *mut core::ffi::c_void);

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

        // Provide common globals expected by many npm/browserified packages.
        trueos_qjs::node::install_globals(ctx);

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
            // Modules often produce a Promise value from evaluation; upstream qjs drains the job queue.
            // Do this before printing any return value so side effects (e.g., `print`) happen.
            let _ = drain_pending_jobs(io, rt, ctx);

            // Print return value unless it's `undefined`.
            if print_result && val.tag != trueos_qjs::JS_TAG_UNDEFINED {
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

        let opaque_ptr = trueos_qjs::JS_GetContextOpaque(ctx) as *mut QjsShellOpaque;
        if !opaque_ptr.is_null() {
            trueos_qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
            drop(Box::from_raw(opaque_ptr));
        }

        trueos_qjs::JS_FreeContext(ctx);
        trueos_qjs::JS_FreeRuntime(rt);
    }
}

pub(crate) async fn eval_bytes_opts_async(
    io: &'static dyn ShellBackend,
    filename: *const c_char,
    bytes: &[u8],
    eval_flags: c_int,
    print_result: bool,
) {
    unsafe {
        let rt = trueos_qjs::JS_NewRuntime();
        if rt.is_null() {
            io.write_str("qjs: JS_NewRuntime failed\r\n");
            return;
        }

        trueos_qjs::node::install(rt);

        let ctx = trueos_qjs::JS_NewContext(rt);
        if ctx.is_null() {
            trueos_qjs::JS_FreeRuntime(rt);
            io.write_str("qjs: JS_NewContext failed\r\n");
            return;
        }

        let opaque = Box::new(QjsShellOpaque { io });
        let opaque_ptr = Box::into_raw(opaque);
        trueos_qjs::JS_SetContextOpaque(ctx, opaque_ptr as *mut core::ffi::c_void);

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

        trueos_qjs::node::install_globals(ctx);

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
            // Drain microtasks + pump kernel async FS completions into JS Promises.
            let _ = drain_jobs_and_promises(io, rt, ctx, 30_000).await;

            if print_result && val.tag != trueos_qjs::JS_TAG_UNDEFINED {
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

        // Best-effort: reject/cleanup any unresolved async fs ops.
        trueos_qjs::async_ops::drain_all_for_context(ctx);

        let opaque_ptr = trueos_qjs::JS_GetContextOpaque(ctx) as *mut QjsShellOpaque;
        if !opaque_ptr.is_null() {
            trueos_qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
            drop(Box::from_raw(opaque_ptr));
        }
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
    // Upstream-ish default: don't print return values unless asked (e.g. via -p).
    eval_bytes_opts(io, filename, bytes, flags, false);
}
