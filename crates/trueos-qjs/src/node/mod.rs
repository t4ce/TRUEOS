#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::ffi::{c_char, c_int, c_void};
use core::sync::atomic::{AtomicU32, Ordering};

use crate as qjs;

static FETCH_TMP_SEQ: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeProfile {
    Default,
    Shell,
    Worker,
    Browser,
}

impl RuntimeProfile {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Shell => "shell",
            Self::Worker => "worker",
            Self::Browser => "browser",
        }
    }
}

unsafe extern "C" fn trueos_node_module_normalize(
    ctx: *mut qjs::JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    _opaque: *mut c_void,
) -> *mut c_char {
    // Delegate to the shared TRUEOS normalizer in Node mode.
    qjs::trueos_module_loader::normalize_with_mode(
        ctx,
        module_base_name,
        module_name,
        qjs::trueos_module_loader::NormalizeMode::Node,
    )
}

/// Install the TRUEOS module loader with Node-ish specifier resolution.
///
/// This composes the existing TRUEOS loader (`trueos_modules::trueos_module_loader`) but
/// upgrades normalization rules:
/// - Some Node builtins are provided natively (e.g. `process`, `path`).
/// - Other common Node builtins (e.g. `events`, `util`, ...) are routed to pinned polyfill
///   packages on esm.sh (since esm.sh does not serve `node:*` specifiers directly).
/// - Unknown `node:*` specifiers are routed through esm.sh by stripping the `node:` prefix.
pub unsafe fn install(rt: *mut qjs::JSRuntime) {
    if rt.is_null() {
        return;
    }

    qjs::JS_SetModuleLoaderFunc(
        rt,
        Some(trueos_node_module_normalize),
        Some(qjs::trueos_module_loader::trueos_module_loader),
        core::ptr::null_mut(),
    );
}

/// Convenience wrapper: Node mode currently reuses the same globals as the base loader.
pub unsafe fn install_globals(ctx: *mut qjs::JSContext) {
    install_globals_with_profile(ctx, RuntimeProfile::Default);
}

pub unsafe fn install_globals_with_profile(ctx: *mut qjs::JSContext, profile: RuntimeProfile) {
    ensure_global_env(ctx);
    ensure_global_web_platform(ctx);
    ensure_global_console(ctx);
    ensure_global_timers(ctx);
    ensure_global_intl(ctx);
    ensure_global_fetch(ctx);
    ensure_global_kernel_time(ctx);
    ensure_runtime_profile_marker(ctx, profile);
}

unsafe fn ensure_runtime_profile_marker(ctx: *mut qjs::JSContext, profile: RuntimeProfile) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }
    let _ = qjs::jsbind::set_str_prop(ctx, global, b"__trueosRuntimeProfile\0", profile.as_str());
    qjs::js_free_value(ctx, global);
}

unsafe fn ensure_global_env(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    let shim_src = br#"
(function (G) {
    if (!G) return;
    if (!G.__env__ || typeof G.__env__ !== 'object') {
        G.__env__ = Object.create(null);
    }
})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

    let shim = qjs::js_eval_bytes(
        ctx,
        shim_src,
        b"<node-env-shim>\0".as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );
    if shim.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "node env shim");
    }
    qjs::js_free_value(ctx, shim);
}

unsafe fn ensure_global_web_platform(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    let shim_src = br#"
(function (G) {
    if (!G) return;

    if (typeof G.Blob !== 'function') {
        function Blob(parts, opts) {
            this.size = 0;
            this.type = (opts && opts.type) ? String(opts.type) : '';

            if (parts && typeof parts.length === 'number') {
                for (let i = 0; i < parts.length; i += 1) {
                    const p = parts[i];
                    if (typeof p === 'string') {
                        this.size += p.length;
                    } else if (p && typeof p.byteLength === 'number') {
                        this.size += Number(p.byteLength) || 0;
                    } else if (p && typeof p.length === 'number') {
                        this.size += Number(p.length) || 0;
                    }
                }
            }
        }
        Blob.prototype.arrayBuffer = function () { return Promise.resolve(new ArrayBuffer(0)); };
        Blob.prototype.text = function () { return Promise.resolve(''); };
        Blob.prototype.slice = function () { return this; };
        G.Blob = Blob;
    }

    if (typeof G.File !== 'function') {
        function File(parts, name, opts) {
            G.Blob.call(this, parts, opts);
            this.name = String(name || 'file');
            this.lastModified = (opts && opts.lastModified) ? Number(opts.lastModified) : 0;
        }
        File.prototype = Object.create(G.Blob.prototype);
        File.prototype.constructor = File;
        G.File = File;
    }

    if (typeof G.btoa !== 'function') {
        G.btoa = function (_s) { return ''; };
    }
    if (typeof G.atob !== 'function') {
        G.atob = function (_s) { return ''; };
    }
})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

    let shim = qjs::js_eval_bytes(
        ctx,
        shim_src,
        b"<node-web-platform-shim>\0".as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );
    if shim.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "node web platform shim");
    }
    qjs::js_free_value(ctx, shim);
}

unsafe extern "C" fn trueos_ntp_unix_seconds_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let secs = v::vclock::ntp_current_unix_seconds();
    qjs::JS_NewFloat64(ctx, secs as f64)
}

unsafe extern "C" fn trueos_kernel_date_day_month_year_js(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let bytes: Vec<u8> = v::vclock::kernel_date_day_month_year()
        .map(|value| value.into_bytes())
        .unwrap_or_default();
    qjs::JS_NewStringLen(ctx, bytes.as_ptr() as *const c_char, bytes.len())
}

unsafe fn ensure_global_kernel_time(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    let _ = qjs::jsbind::install_fn(
        ctx,
        global,
        b"__trueosNtpUnixSeconds\0",
        0,
        Some(trueos_ntp_unix_seconds_js),
    );
    let _ = qjs::jsbind::install_fn(
        ctx,
        global,
        b"kernelDateDayMonthYear\0",
        0,
        Some(trueos_kernel_date_day_month_year_js),
    );

    qjs::js_free_value(ctx, global);
}

#[inline]
fn next_fetch_cache_path() -> String {
    let id = FETCH_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut path = String::from("/qjs/cdn/__net_cache_");
    push_u32_hex(&mut path, id);
    path.push_str(".txt");
    path
}

fn push_u32_hex(out: &mut String, mut v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 8];
    let mut i = buf.len();
    while v != 0 {
        i -= 1;
        let nib = (v & 0xF) as u8;
        buf[i] = if nib < 10 {
            b'0' + nib
        } else {
            b'a' + (nib - 10)
        };
        v >>= 4;
    }
    for b in &buf[i..] {
        out.push(*b as char);
    }
}

unsafe extern "C" fn trueos_fetch_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);
    if argv.is_null() || argc <= 0 {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut url_len: usize = 0;
    let url_c = qjs::JS_ToCStringLen2(ctx, &mut url_len as *mut usize, args[0], 0);
    if url_c.is_null() {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }
    let url = core::slice::from_raw_parts(url_c as *const u8, url_len);

    let mut method_len: usize = 0;
    let mut method_c: *const c_char = core::ptr::null();
    let method = if args.len() > 1 {
        method_c = qjs::JS_ToCStringLen2(ctx, &mut method_len as *mut usize, args[1], 0);
        if method_c.is_null() {
            b"GET".as_slice()
        } else {
            core::slice::from_raw_parts(method_c as *const u8, method_len)
        }
    } else {
        b"GET".as_slice()
    };
    let is_post = method.eq_ignore_ascii_case(b"POST");

    let mut body_len: usize = 0;
    let mut body_c: *const c_char = core::ptr::null();
    let body = if is_post && args.len() > 2 {
        body_c = qjs::JS_ToCStringLen2(ctx, &mut body_len as *mut usize, args[2], 0);
        if body_c.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(body_c as *const u8, body_len)
        }
    } else {
        &[][..]
    };

    let mut bearer_len: usize = 0;
    let mut bearer_c: *const c_char = core::ptr::null();
    let bearer = if is_post && args.len() > 3 {
        bearer_c = qjs::JS_ToCStringLen2(ctx, &mut bearer_len as *mut usize, args[3], 0);
        if bearer_c.is_null() {
            None
        } else {
            Some(core::slice::from_raw_parts(bearer_c as *const u8, bearer_len))
        }
    } else {
        None
    };

    if url.first().copied() == Some(b'/') {
        if is_post {
            let code = js_int32(-1);
            let _ = qjs::JS_Call(
                ctx,
                reject,
                qjs::JSValue::undefined(),
                1,
                &code as *const qjs::JSValue,
            );
            if !bearer_c.is_null() {
                qjs::JS_FreeCString(ctx, bearer_c);
            }
            if !body_c.is_null() {
                qjs::JS_FreeCString(ctx, body_c);
            }
            if !method_c.is_null() {
                qjs::JS_FreeCString(ctx, method_c);
            }
            qjs::JS_FreeCString(ctx, url_c);
            qjs::js_free_value(ctx, resolve);
            qjs::js_free_value(ctx, reject);
            return promise;
        }
        match qjs::async_ops::start_read_file(url) {
            Ok(op_id) => {
                qjs::async_ops::register_promise(
                    ctx,
                    op_id,
                    qjs::async_ops::OpKind::ReadText,
                    resolve,
                    reject,
                    alloc::vec::Vec::new(),
                );
            }
            Err(code) => {
                let code_js = js_int32(code);
                let _ = qjs::JS_Call(
                    ctx,
                    reject,
                    qjs::JSValue::undefined(),
                    1,
                    &code_js as *const qjs::JSValue,
                );
            }
        }
    } else {
        if let Ok(url_str) = core::str::from_utf8(url) {
            if is_post {
                qjs::trueos_shims::log_info(
                    alloc::format!("qjs fetch: POST {}\n", url_str).as_str(),
                );
            } else {
                qjs::trueos_shims::log_info(
                    alloc::format!("qjs fetch: GET {}\n", url_str).as_str(),
                );
            }
        }

        let cache_path = if is_post {
            None
        } else {
            let path = next_fetch_cache_path();
            let _ = v::vfs::remove(path.as_bytes());
            Some(path)
        };
        let start_res = if is_post {
            qjs::async_ops::start_net_post_json_bytes(url, body, bearer)
        } else {
            qjs::async_ops::start_net_fetch_to_file(
                url,
                cache_path.as_deref().unwrap_or_default().as_bytes(),
            )
        };

        match start_res {
            Ok(op_id) => {
                if is_post {
                    qjs::trueos_shims::log_info(
                        alloc::format!("qjs fetch: queued request_id={} mode=memory\n", op_id)
                            .as_str(),
                    );
                } else {
                    qjs::trueos_shims::log_info(
                        alloc::format!(
                            "qjs fetch: queued request_id={} cache_path={}\n",
                            op_id,
                            cache_path.as_deref().unwrap_or_default()
                        )
                        .as_str(),
                    );
                }
                qjs::async_ops::register_promise(
                    ctx,
                    op_id,
                    if is_post {
                        qjs::async_ops::OpKind::NetPostJsonTextBytes
                    } else {
                        qjs::async_ops::OpKind::NetFetchTextFile
                    },
                    resolve,
                    reject,
                    cache_path
                        .as_deref()
                        .map(|v| v.as_bytes().to_vec())
                        .unwrap_or_default(),
                );
            }
            Err(code) => {
                qjs::trueos_shims::log_error(
                    alloc::format!("qjs fetch: start failed rc={}\n", code).as_str(),
                );
                let code_js = js_int32(code);
                let _ = qjs::JS_Call(
                    ctx,
                    reject,
                    qjs::JSValue::undefined(),
                    1,
                    &code_js as *const qjs::JSValue,
                );
            }
        }
    }

    if !bearer_c.is_null() {
        qjs::JS_FreeCString(ctx, bearer_c);
    }
    if !body_c.is_null() {
        qjs::JS_FreeCString(ctx, body_c);
    }
    if !method_c.is_null() {
        qjs::JS_FreeCString(ctx, method_c);
    }
    qjs::JS_FreeCString(ctx, url_c);
    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn trueos_fetch_bytes(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);
    if argv.is_null() || argc <= 0 {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut url_len: usize = 0;
    let url_c = qjs::JS_ToCStringLen2(ctx, &mut url_len as *mut usize, args[0], 0);
    if url_c.is_null() {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }
    let url = core::slice::from_raw_parts(url_c as *const u8, url_len);

    if url.first().copied() == Some(b'/') {
        match qjs::async_ops::start_read_file(url) {
            Ok(op_id) => {
                qjs::async_ops::register_promise(
                    ctx,
                    op_id,
                    qjs::async_ops::OpKind::ReadBytes,
                    resolve,
                    reject,
                    alloc::vec::Vec::new(),
                );
            }
            Err(code) => {
                let code_js = js_int32(code);
                let _ = qjs::JS_Call(
                    ctx,
                    reject,
                    qjs::JSValue::undefined(),
                    1,
                    &code_js as *const qjs::JSValue,
                );
            }
        }
    } else {
        match qjs::async_ops::start_net_fetch_bytes(url) {
            Ok(op_id) => {
                qjs::async_ops::register_promise(
                    ctx,
                    op_id,
                    qjs::async_ops::OpKind::NetFetchBytes,
                    resolve,
                    reject,
                    alloc::vec::Vec::new(),
                );
            }
            Err(code) => {
                let code_js = js_int32(code);
                let _ = qjs::JS_Call(
                    ctx,
                    reject,
                    qjs::JSValue::undefined(),
                    1,
                    &code_js as *const qjs::JSValue,
                );
            }
        }
    }

    qjs::JS_FreeCString(ctx, url_c);
    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn trueos_prewarm_url(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return js_int32(-1);
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut url_len: usize = 0;
    let url_c = qjs::JS_ToCStringLen2(ctx, &mut url_len as *mut usize, args[0], 0);
    if url_c.is_null() {
        return js_int32(-1);
    }
    let rc = crate::trueos_shims::trueos_cabi_net_prewarm_url_start(url_c as *const u8, url_len);
    qjs::JS_FreeCString(ctx, url_c);
    js_int32(rc)
}

unsafe extern "C" fn trueos_global_log_line(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    const MAX_LOG_LINE_BYTES: usize = 1024;

    if argv.is_null() || argc <= 0 {
        return js_int32(-1);
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut line_len: usize = 0;
    let line_c = qjs::JS_ToCStringLen2(ctx, &mut line_len as *mut usize, args[0], 0);
    if line_c.is_null() {
        return js_int32(-1);
    }

    let raw = core::slice::from_raw_parts(line_c as *const u8, line_len);
    let take_len = raw.len().min(MAX_LOG_LINE_BYTES);
    let mut line = Vec::with_capacity(take_len + 1);
    for b in raw.iter().take(take_len).copied() {
        match b {
            b'\n' | b'\r' | b'\t' => line.push(b' '),
            0x20..=0x7e => line.push(b),
            _ => line.push(b'?'),
        }
    }
    line.push(b'\n');

    qjs::platform::sys::write_stdout(&line);
    qjs::JS_FreeCString(ctx, line_c);
    js_int32(0)
}

unsafe extern "C" fn trueos_resolve_ready_image_texture(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);
    if argv.is_null() || argc <= 0 {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut url_len: usize = 0;
    let url_c = qjs::JS_ToCStringLen2(ctx, &mut url_len as *mut usize, args[0], 0);
    if url_c.is_null() {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    let url = core::slice::from_raw_parts(url_c as *const u8, url_len);
    let tex_id = qjs::cmd_stream::alloc_managed_tex_id();
    if tex_id == 0 {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::JS_FreeCString(ctx, url_c);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    if url.starts_with(b"data:") {
        qjs::cmd_stream::release_managed_tex_id(tex_id);
        let code_js = js_int32(-1);
        let _ = qjs::JS_Call(
            ctx,
            reject,
            qjs::JSValue::undefined(),
            1,
            &code_js as *const qjs::JSValue,
        );
    } else if url.first().copied() == Some(b'/') {
        match qjs::async_ops::start_read_file(url) {
            Ok(op_id) => {
                qjs::async_ops::register_ready_image_texture_request(
                    ctx,
                    op_id,
                    resolve,
                    reject,
                    url.to_vec(),
                    tex_id,
                    qjs::async_ops::ImageRequestSource::LocalPath,
                );
            }
            Err(code) => {
                qjs::cmd_stream::release_managed_tex_id(tex_id);
                let code_js = js_int32(code);
                let _ = qjs::JS_Call(
                    ctx,
                    reject,
                    qjs::JSValue::undefined(),
                    1,
                    &code_js as *const qjs::JSValue,
                );
            }
        }
    } else {
        match qjs::async_ops::start_net_fetch_bytes(url) {
            Ok(op_id) => {
                qjs::async_ops::register_ready_image_texture_request(
                    ctx,
                    op_id,
                    resolve,
                    reject,
                    url.to_vec(),
                    tex_id,
                    qjs::async_ops::ImageRequestSource::RemoteUrl,
                );
            }
            Err(code) => {
                qjs::cmd_stream::release_managed_tex_id(tex_id);
                let code_js = js_int32(code);
                let _ = qjs::JS_Call(
                    ctx,
                    reject,
                    qjs::JSValue::undefined(),
                    1,
                    &code_js as *const qjs::JSValue,
                );
            }
        }
    }

    qjs::JS_FreeCString(ctx, url_c);
    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

fn read_fs_len(path: &[u8]) -> isize {
    unsafe {
        qjs::trueos_shims::trueos_cabi_fs_read_file(
            path.as_ptr(),
            path.len(),
            core::ptr::null_mut(),
            0,
        )
    }
}

unsafe extern "C" fn trueos_prefetch_module(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);
    if argv.is_null() || argc <= 0 {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut spec_len: usize = 0;
    let spec_c = qjs::JS_ToCStringLen2(ctx, &mut spec_len as *mut usize, args[0], 0);
    if spec_c.is_null() {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    let mut base_len: usize = 0;
    let mut base_c: *const c_char = core::ptr::null();
    if args.len() > 1 {
        base_c = qjs::JS_ToCStringLen2(ctx, &mut base_len as *mut usize, args[1], 0);
    }

    let normalized_ptr = qjs::trueos_module_loader::normalize_with_mode(
        ctx,
        base_c,
        spec_c,
        qjs::trueos_module_loader::NormalizeMode::Node,
    );

    if !base_c.is_null() {
        qjs::JS_FreeCString(ctx, base_c);
    }
    qjs::JS_FreeCString(ctx, spec_c);

    if normalized_ptr.is_null() {
        let code = js_int32(-1);
        let _ =
            qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &code as *const qjs::JSValue);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    let normalized = core::ffi::CStr::from_ptr(normalized_ptr)
        .to_bytes()
        .to_vec();
    qjs::js_free(ctx, normalized_ptr as *mut c_void);

    if !qjs::trueos_module_loader::is_url_specifier(&normalized) {
        let out = qjs::JS_NewStringLen(ctx, normalized.as_ptr() as *const c_char, normalized.len());
        let _ =
            qjs::JS_Call(ctx, resolve, qjs::JSValue::undefined(), 1, &out as *const qjs::JSValue);
        qjs::js_free_value(ctx, out);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    let cache_path = qjs::trueos_module_loader::cache_path_for_url(&normalized);
    if qjs::trueos_module_loader::has_embedded_module(&cache_path) || read_fs_len(&cache_path) >= 0
    {
        let out = qjs::JS_NewStringLen(ctx, normalized.as_ptr() as *const c_char, normalized.len());
        let _ =
            qjs::JS_Call(ctx, resolve, qjs::JSValue::undefined(), 1, &out as *const qjs::JSValue);
        qjs::js_free_value(ctx, out);
        qjs::js_free_value(ctx, resolve);
        qjs::js_free_value(ctx, reject);
        return promise;
    }

    match qjs::async_ops::start_net_fetch_to_file(&normalized, &cache_path) {
        Ok(op_id) => {
            qjs::async_ops::register_promise(
                ctx,
                op_id,
                qjs::async_ops::OpKind::NetFetchModule,
                resolve,
                reject,
                normalized,
            );
        }
        Err(code) => {
            let code_js = js_int32(code);
            let _ = qjs::JS_Call(
                ctx,
                reject,
                qjs::JSValue::undefined(),
                1,
                &code_js as *const qjs::JSValue,
            );
        }
    }

    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe fn ensure_global_fetch(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    let helpers: &[(&[u8], qjs::JSCFunction, i32)] = &[
        (b"__trueosFetchText\0", trueos_fetch_text, 1),
        (b"__trueosFetchBytes\0", trueos_fetch_bytes, 1),
        (b"__trueosPrewarmUrl\0", trueos_prewarm_url, 1),
        (b"__trueosGlobalLogLine\0", trueos_global_log_line, 1),
        (b"__trueosResolveReadyImageTexture\0", trueos_resolve_ready_image_texture, 1),
        (b"__trueosPrefetchModule\0", trueos_prefetch_module, 2),
    ];
    for &(name, func, argc) in helpers {
        let _ = qjs::jsbind::install_fn(ctx, global, name, argc, Some(func));
    }

    let shim_src = br#"
(function (G) {
    if (!G || typeof G.__trueosFetchText !== 'function') return;

    if (typeof G.Headers !== 'function') {
        class Headers {
            constructor(init) {
                this._m = Object.create(null);
                if (init && typeof init === 'object') {
                    for (const k of Object.keys(init)) this.set(k, init[k]);
                }
            }
            _k(name) { return String(name || '').toLowerCase(); }
            get(name) {
                const v = this._m[this._k(name)];
                return typeof v === 'string' ? v : null;
            }
            set(name, value) { this._m[this._k(name)] = String(value); }
            has(name) { return this.get(name) !== null; }
            entries() { return Object.entries(this._m)[Symbol.iterator](); }
            forEach(cb, thisArg) {
                for (const [k, v] of Object.entries(this._m)) cb.call(thisArg, v, k, this);
            }
            [Symbol.iterator]() { return this.entries(); }
        }
        G.Headers = Headers;
    }

    if (typeof G.Request !== 'function') {
        class Request {
            constructor(input, init) {
                const src = (input && typeof input === 'object') ? input : null;
                this.url = src && typeof src.url === 'string' ? src.url : String(input || '');
                this.method = String((init && init.method) || (src && src.method) || 'GET').toUpperCase();
                this.headers = new G.Headers((init && init.headers) || (src && src.headers) || null);
                this.body = (init && init.body) !== undefined ? init.body : (src ? src.body : undefined);
            }
        }
        G.Request = Request;
    }

    if (typeof G.Response !== 'function') {
        class Response {
            constructor(body, init) {
                const opts = init || {};
                const isBinary = !!opts.__trueosBinary;
                this._binary = null;
                this._body = '';
                if (isBinary) {
                    if (body instanceof ArrayBuffer) {
                        this._binary = body.slice(0);
                    } else if (body && body.buffer instanceof ArrayBuffer && typeof body.byteLength === 'number') {
                        this._binary = body.buffer.slice(body.byteOffset || 0, (body.byteOffset || 0) + body.byteLength);
                    } else {
                        this._binary = new Uint8Array(0).buffer;
                    }
                } else {
                    this._body = String(body == null ? '' : body);
                }
                this.status = Number(opts.status || 200) | 0;
                this.statusText = String(opts.statusText || 'OK');
                this.headers = opts.headers instanceof G.Headers ? opts.headers : new G.Headers(opts.headers || null);
                this.ok = this.status >= 200 && this.status < 300;
            }
            async text() { return this._body; }
            async json() { return JSON.parse(this._body); }
            async arrayBuffer() {
                if (this._binary instanceof ArrayBuffer) {
                    return this._binary.slice(0);
                }
                const src = String(this._body || '');
                const out = new Uint8Array(src.length);
                for (let i = 0; i < src.length; i += 1) out[i] = src.charCodeAt(i) & 0xFF;
                return out.buffer;
            }
            clone() {
                return new Response(this._binary instanceof ArrayBuffer ? this._binary : this._body, {
                    status: this.status,
                    statusText: this.statusText,
                    headers: this.headers,
                    __trueosBinary: this._binary instanceof ArrayBuffer,
                });
            }
        }
        G.Response = Response;
    }

    if (typeof G.fetch !== 'function') {
        G.fetch = function fetch(input, init) {
            const req = input instanceof G.Request ? input : new G.Request(input, init);
            const method = String(req.method || 'GET').toUpperCase();
            const wantBinary = !!(init && init.__trueosBinary);
            if (method !== 'GET' && method !== 'POST') {
                return Promise.reject(new Error('trueos fetch shim supports GET and POST only'));
            }
            if (wantBinary && method !== 'GET') {
                return Promise.reject(new Error('trueos binary fetch shim supports GET only'));
            }
            let bodyArg = '';
            let bearer = '';
            if (method === 'POST') {
                bodyArg = req.body == null ? '' : (typeof req.body === 'string' ? req.body : String(req.body));
                const auth = req.headers && typeof req.headers.get === 'function'
                    ? req.headers.get('authorization')
                    : null;
                if (typeof auth === 'string') {
                    const m = auth.match(/^\s*Bearer\s+(.+)\s*$/i);
                    if (m && m[1]) bearer = m[1];
                }
            }
            const fetchPromise = wantBinary
                ? Promise.resolve((function () {
                    return G.__trueosFetchBytes(req.url);
                })())
                : Promise.resolve((function () {
                    return G.__trueosFetchText(req.url, method, bodyArg, bearer);
                })());
            return fetchPromise.then((body) => {
                const headers = new G.Headers();
                return new G.Response(body, { status: 200, statusText: 'OK', headers, __trueosBinary: wantBinary });
            });
        };
    }

    if (typeof G.prefetchModule !== 'function' && typeof G.__trueosPrefetchModule === 'function') {
        G.prefetchModule = function prefetchModule(specifier, base) {
            const spec = String(specifier);
            return Promise.resolve(
                base == null
                    ? G.__trueosPrefetchModule(spec)
                    : G.__trueosPrefetchModule(spec, String(base)),
            );
        };
    }

    if (typeof G.importModule !== 'function' && typeof G.__trueosPrefetchModule === 'function') {
        G.importModule = function importModule(specifier, base) {
            const spec = String(specifier);
            const prefetch = base == null
                ? G.__trueosPrefetchModule(spec)
                : G.__trueosPrefetchModule(spec, String(base));
            return Promise.resolve(prefetch).then((normalized) => import(normalized));
        };
    }

    if (typeof G.createImportHelpers !== 'function') {
        G.createImportHelpers = function createImportHelpers(base) {
            const baseUrl = base == null ? undefined : String(base);
            return {
                prefetch(specifier) {
                    if (typeof G.prefetchModule === 'function') {
                        return G.prefetchModule(specifier, baseUrl);
                    }
                    return Promise.resolve(String(specifier));
                },
                import(specifier) {
                    if (typeof G.importModule === 'function') {
                        return G.importModule(specifier, baseUrl);
                    }
                    return import(String(specifier));
                },
            };
        };
    }
})(typeof globalThis !== 'undefined' ? globalThis : this);
"#;

    let shim = qjs::js_eval_bytes(
        ctx,
        shim_src,
        b"<node-fetch-shim>\0".as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );
    if shim.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "node fetch shim");
    }
    qjs::js_free_value(ctx, shim);
    qjs::js_free_value(ctx, global);
}

unsafe fn ensure_global_timers(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    qjs::timers::install_globals(ctx, global);
    qjs::js_free_value(ctx, global);
}

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    qjs::platform::sys::write_stdout(bytes);
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

unsafe fn log_js_args(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
    prefix: &str,
) {
    log_str(prefix);
    if !argv.is_null() && argc > 0 {
        let args = core::slice::from_raw_parts(argv, argc as usize);
        for (idx, arg) in args.iter().enumerate() {
            if idx > 0 {
                log_str(" ");
            }
            let Some(text) = qjs::jsbind::JsStringRef::new(ctx, *arg) else {
                log_str("<toString failed>");
                continue;
            };
            log_bytes(strip_truesurfer_synthetic_markers(text.as_bytes()).as_bytes());
        }
    }
    log_str("\n");
}

fn strip_truesurfer_synthetic_markers(bytes: &[u8]) -> String {
    const MARKER: &str = "<truesurfer-";
    const KNOWN_MARKERS: [&str; 13] = [
        "<truesurfer-parse5-trueos-host-core>",
        "<truesurfer-parse5-trueos-host-core",
        "<truesurfer-parse5-trueos-host-cor",
        "<truesurfer-parse5-trueos-host-event>",
        "<truesurfer-parse5-trueos-host-canvas>",
        "<truesurfer-parse5-trueos-host-dom>",
        "<truesurfer-parse5-trueos-host-fetch>",
        "<truesurfer-parse5-trueos-host-capture>",
        "<truesurfer-parse5-trueos-app.js>",
        "<truesurfer-parse5-trueos-app",
        "<truesurfer-init>",
        "<truesurfer-pixi-host-prelude>",
        "<truesurfer-pixi-capture-adapter>",
    ];

    let Ok(text) = core::str::from_utf8(bytes) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    let mut cleaned = String::from(text);
    strip_trueos_bare_symbols(&mut cleaned);
    if !cleaned.contains(MARKER) {
        return cleaned;
    }

    for marker in KNOWN_MARKERS {
        while let Some(idx) = cleaned.find(marker) {
            cleaned.replace_range(idx..idx + marker.len(), "");
        }
    }

    if !cleaned.contains(MARKER) {
        return cleaned;
    }

    let mut out = String::with_capacity(cleaned.len());
    let mut rest = cleaned.as_str();
    while let Some(idx) = rest.find(MARKER) {
        out.push_str(&rest[..idx]);
        let marker_tail = &rest[idx..];
        if let Some(end_rel) = marker_tail.find('>') {
            let marker_candidate = &marker_tail[..=end_rel];
            let marker_body = &marker_candidate[1..marker_candidate.len().saturating_sub(1)];
            if marker_body
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' || ch == '_')
            {
                rest = &marker_tail[end_rel + 1..];
                continue;
            }
        }
        out.push_str(MARKER);
        rest = &marker_tail[MARKER.len()..];
    }
    out.push_str(rest);
    out
}

fn strip_trueos_bare_symbols(text: &mut String) {
    const SYMBOLS: [&str; 3] = ["__trueosNum", "__trueosNu", "__trueosN"];
    for symbol in SYMBOLS {
        while let Some(idx) = text.find(symbol) {
            text.replace_range(idx..idx + symbol.len(), "");
        }
    }
}

unsafe extern "C" fn console_log(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.log: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_info(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.info: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_debug(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.debug: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_warn(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.warn: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_error(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.error: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_trace(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.trace: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_assert(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut cond = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut cond as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    if cond == 0.0 {
        let rest_ptr = if argc > 1 {
            unsafe { argv.add(1) }
        } else {
            core::ptr::null()
        };
        let rest_argc = if argc > 1 { argc - 1 } else { 0 };
        log_js_args(ctx, rest_argc, rest_ptr, "console.assert: ");
    }
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_time(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.time: ");
    qjs::JSValue::undefined()
}

unsafe extern "C" fn console_time_end(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    log_js_args(ctx, argc, argv, "console.timeEnd: ");
    qjs::JSValue::undefined()
}

unsafe fn ensure_global_console(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }

    let existing = qjs::JS_GetPropertyStr(ctx, global, b"console\0".as_ptr() as *const c_char);
    let console = if existing.is_exception()
        || existing.tag == qjs::JS_TAG_UNDEFINED
        || existing.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, existing);
        qjs::JS_NewObject(ctx)
    } else {
        existing
    };
    if console.is_exception() {
        qjs::js_free_value(ctx, console);
        qjs::js_free_value(ctx, global);
        return;
    }

    macro_rules! set_console_fn {
        ($name:literal, $func:expr, $argc:expr) => {{
            let k = concat!($name, "\0");
            let _ = qjs::jsbind::install_fn(ctx, console, k.as_bytes(), $argc, Some($func));
        }};
    }

    set_console_fn!("log", console_log, 1);
    set_console_fn!("info", console_info, 1);
    set_console_fn!("debug", console_debug, 1);
    set_console_fn!("warn", console_warn, 1);
    set_console_fn!("error", console_error, 1);
    set_console_fn!("trace", console_trace, 1);
    set_console_fn!("assert", console_assert, 1);
    set_console_fn!("time", console_time, 1);
    set_console_fn!("timeEnd", console_time_end, 1);

    let _ = qjs::jsbind::set_prop(ctx, global, b"console\0", console);
    qjs::js_free_value(ctx, global);
}

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

unsafe fn locale_profile_from_arg0(
    ctx: *mut qjs::JSContext,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> &'static trueos_locale::IntlLocaleProfile {
    if argv.is_null() || argc <= 0 {
        return trueos_locale::intl_locale_profile(trueos_locale::DEFAULT_INTL_LOCALE);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(locale) = qjs::jsbind::JsStringRef::new(ctx, args[0]) else {
        return trueos_locale::intl_locale_profile(trueos_locale::DEFAULT_INTL_LOCALE);
    };
    match locale.as_str() {
        Some(s) => trueos_locale::intl_locale_profile(s),
        None => trueos_locale::intl_locale_profile(trueos_locale::DEFAULT_INTL_LOCALE),
    }
}

unsafe fn set_str_prop(ctx: *mut qjs::JSContext, obj: qjs::JSValue, key: &[u8], val: &str) {
    let _ = qjs::jsbind::set_str_prop(ctx, obj, key, val);
}

unsafe fn set_char_prop(ctx: *mut qjs::JSContext, obj: qjs::JSValue, key: &[u8], val: char) {
    let mut buf = [0u8; 4];
    let s = val.encode_utf8(&mut buf);
    set_str_prop(ctx, obj, key, s);
}

unsafe fn make_resolved_options(
    ctx: *mut qjs::JSContext,
    profile: &trueos_locale::IntlLocaleProfile,
    kind: &str,
) -> qjs::JSValue {
    let out = qjs::JS_NewObject(ctx);
    if out.is_exception() {
        return out;
    }
    set_str_prop(ctx, out, b"locale\0", profile.code);
    set_str_prop(ctx, out, b"kind\0", kind);
    set_str_prop(ctx, out, b"numberingSystem\0", "latn");
    set_str_prop(ctx, out, b"calendar\0", "gregory");
    set_char_prop(ctx, out, b"decimalSeparator\0", profile.decimal_separator);
    set_char_prop(ctx, out, b"groupingSeparator\0", profile.grouping_separator);
    set_char_prop(ctx, out, b"minusSign\0", profile.minus_sign);
    set_char_prop(ctx, out, b"percentSign\0", profile.percent_sign);
    set_str_prop(ctx, out, b"datePattern\0", profile.date_pattern);
    set_str_prop(ctx, out, b"timePattern\0", profile.time_pattern);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        out,
        b"firstDayOfWeek\0".as_ptr() as *const c_char,
        js_int32(profile.first_day_of_week as i32),
    );
    out
}

unsafe extern "C" fn intl_resolved_options(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let ro =
        qjs::JS_GetPropertyStr(ctx, this_val, b"__resolvedOptions\0".as_ptr() as *const c_char);
    if ro.is_exception() || ro.tag == qjs::JS_TAG_UNDEFINED || ro.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, ro);
        return qjs::JS_NewObject(ctx);
    }
    qjs::js_dup_value(ctx, ro)
}

unsafe extern "C" fn intl_format(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0);
    if cstr.is_null() {
        return qjs::JSValue::exception();
    }
    let out = qjs::JS_NewStringLen(ctx, cstr, len);
    qjs::JS_FreeCString(ctx, cstr);
    out
}

unsafe fn make_formatter_object(
    ctx: *mut qjs::JSContext,
    profile: &trueos_locale::IntlLocaleProfile,
    kind: &str,
) -> qjs::JSValue {
    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return obj;
    }
    let ro = make_resolved_options(ctx, profile, kind);
    let _ = qjs::jsbind::install_fn(ctx, obj, b"format\0", 1, Some(intl_format));
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"__resolvedOptions\0".as_ptr() as *const c_char, ro);
    let _ = qjs::jsbind::install_fn(ctx, obj, b"resolvedOptions\0", 0, Some(intl_resolved_options));
    obj
}

unsafe extern "C" fn intl_number_ctor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    make_formatter_object(ctx, profile, "number")
}

unsafe extern "C" fn intl_datetime_ctor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    make_formatter_object(ctx, profile, "dateTime")
}

unsafe extern "C" fn intl_simple_ctor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    make_formatter_object(ctx, profile, "generic")
}

unsafe extern "C" fn intl_get_canonical_locales(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let out = qjs::JS_NewArray(ctx);
    if out.is_exception() {
        return out;
    }
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    let locale =
        qjs::JS_NewStringLen(ctx, profile.code.as_ptr() as *const c_char, profile.code.len());
    let _ = qjs::JS_SetPropertyUint32(ctx, out, 0, locale);
    out
}

unsafe extern "C" fn intl_locale_ctor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return obj;
    }
    let profile = locale_profile_from_arg0(ctx, argc, argv);
    let val = qjs::JS_NewStringLen(ctx, profile.code.as_ptr() as *const c_char, profile.code.len());
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"baseName\0".as_ptr() as *const c_char, val);
    obj
}

unsafe fn ensure_global_intl(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return;
    }
    let key = b"Intl\0";
    let existing = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    let needs_install = existing.is_exception()
        || existing.tag == qjs::JS_TAG_UNDEFINED
        || existing.tag == qjs::JS_TAG_NULL;
    qjs::js_free_value(ctx, existing);
    if !needs_install {
        qjs::js_free_value(ctx, global);
        return;
    }

    let intl = qjs::JS_NewObject(ctx);
    if intl.is_exception() {
        qjs::js_free_value(ctx, global);
        return;
    }

    let ctors: &[(&[u8], qjs::JSCFunction)] = &[
        (b"NumberFormat\0", intl_number_ctor),
        (b"DateTimeFormat\0", intl_datetime_ctor),
        (b"Collator\0", intl_simple_ctor),
        (b"PluralRules\0", intl_simple_ctor),
        (b"RelativeTimeFormat\0", intl_simple_ctor),
        (b"ListFormat\0", intl_simple_ctor),
        (b"DisplayNames\0", intl_simple_ctor),
        (b"Locale\0", intl_locale_ctor),
    ];
    for &(name, func) in ctors {
        let _ =
            qjs::jsbind::install_fn_kind(ctx, intl, name, 1, qjs::JS_CFUNC_CONSTRUCTOR, Some(func));
    }
    let _ = qjs::jsbind::install_fn(
        ctx,
        intl,
        b"getCanonicalLocales\0",
        1,
        Some(intl_get_canonical_locales),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, intl);
    qjs::js_free_value(ctx, global);
}
