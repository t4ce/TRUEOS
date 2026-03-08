#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::vec::Vec;
use core::ffi::{CStr, c_char};

use embassy_time_driver::{TICK_HZ, now};

use crate as qjs;
mod compiled;
mod embedded;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_poll_once();
    fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
}

include!("../../../../src/surface/cabi_codes.rs");

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

#[inline]
fn qjs_loader_trace_enabled() -> bool {
    false
}

#[inline]
pub(super) fn trace_bytes(bytes: &[u8]) {
    if qjs_loader_trace_enabled() {
        log_bytes(bytes);
    }
}

#[inline]
pub(super) fn trace_str(s: &str) {
    if qjs_loader_trace_enabled() {
        log_str(s);
    }
}

#[inline]
pub(super) fn trace_nl() {
    if qjs_loader_trace_enabled() {
        log_nl();
    }
}

#[inline]
pub(super) fn trace_usize_dec(v: usize) {
    if qjs_loader_trace_enabled() {
        log_usize_dec(v);
    }
}

#[inline]
fn trace_cstr_or_null(ptr: *const c_char) {
    if qjs_loader_trace_enabled() {
        log_cstr_or_null(ptr);
    }
}

fn log_normalized(out: &[u8]) {
    trace_str("qjs: normalize out=");
    trace_bytes(out);
    trace_nl();
}

#[inline]
unsafe fn throw_error(ctx: *mut qjs::JSContext, msg: &[u8]) {
    if ctx.is_null() {
        return;
    }
    let s = qjs::JS_NewStringLen(ctx, msg.as_ptr() as *const c_char, msg.len());
    let _ = qjs::JS_Throw(ctx, s);
}

#[inline]
unsafe fn load_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }

    let browser_context_mod =
        unsafe { crate::browser_context::try_create_native_module(ctx, module_name) };
    if !browser_context_mod.is_null() {
        return browser_context_mod;
    }

    let browser_navigator_mod =
        unsafe { crate::browser_navigator::try_create_native_module(ctx, module_name) };
    if !browser_navigator_mod.is_null() {
        return browser_navigator_mod;
    }

    let browser_webgpu_mod =
        unsafe { crate::browser_webgpu::try_create_native_module(ctx, module_name) };
    if !browser_webgpu_mod.is_null() {
        return browser_webgpu_mod;
    }

    let yoga_mod = unsafe { crate::yoga::try_create_native_module(ctx, module_name) };
    if !yoga_mod.is_null() {
        return yoga_mod;
    }

    let three_mod = unsafe { crate::threejs::try_create_native_module(ctx, module_name) };
    if !three_mod.is_null() {
        return three_mod;
    }

    let workers_mod = unsafe { crate::workers::try_create_native_module(ctx, module_name) };
    if !workers_mod.is_null() {
        return workers_mod;
    }

    let lightningcss_mod = unsafe { crate::lightningcss_native::try_create_native_module(ctx, module_name) };
    if !lightningcss_mod.is_null() {
        return lightningcss_mod;
    }

    let lyon_mod = unsafe { crate::lyon_native::try_create_native_module(ctx, module_name) };
    if !lyon_mod.is_null() {
        return lyon_mod;
    }

    unsafe { crate::cmd_stream::try_create_native_module(ctx, module_name) }
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
// Use kernel fetch timeouts as the primary guard.
// An outer loader timeout can fire early while the async-fs service is still
// executing a non-cancellable in-flight fetch, which then starves following ops.
const BOOTSTRAP_NET_TIMEOUT_MS: u64 = 0;
const BOOTSTRAP_NET_FETCH_RETRIES: usize = 3;
const BOOTSTRAP_NET_RETRY_BACKOFF_MS: u64 = 250;

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

unsafe fn read_file_js_malloc_rc(
    ctx: *mut qjs::JSContext,
    path: &[u8],
) -> Result<(*mut u8, usize), isize> {
    let len = trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0);
    if len < 0 {
        return Err(len);
    }
    let len = len as usize;
    let buf = qjs::js_malloc(ctx, len + 1) as *mut u8;
    if buf.is_null() {
        return Err(FS_ERR_NO_SPACE as isize);
    }
    *buf.add(len) = 0;
    let got = trueos_cabi_fs_read_file(path.as_ptr(), path.len(), buf, len);
    if got < 0 {
        qjs::js_free(ctx, buf as *mut core::ffi::c_void);
        return Err(got);
    }
    Ok((buf, got as usize))
}

unsafe fn fetch_to_cache_rc_async(
    url: &[u8],
    cache_path: &[u8],
    timeout_ms: u64,
) -> Result<(), i32> {
    let hz = TICK_HZ as u64;
    let total_ticks = if timeout_ms == 0 || hz == 0 {
        0
    } else {
        (timeout_ms.saturating_mul(hz) + 999) / 1000
    };
    let deadline = if total_ticks == 0 {
        0
    } else {
        now().saturating_add(total_ticks)
    };

    let mut attempts = 0usize;
    loop {
        attempts = attempts.saturating_add(1);
        let op_id = match qjs::async_fs::start_net_fetch_to_file(url, cache_path) {
            Ok(id) => id,
            Err(code) => return Err(code),
        };

        loop {
            let len = qjs::async_fs::result_len(op_id);
            if len == FS_ERR_NOT_FOUND as isize && !qjs::async_fs::has_completion_result(op_id) {
                if total_ticks != 0 && now() >= deadline {
                    let _ = qjs::async_fs::discard(op_id);
                    return Err(FS_ERR_TIMEOUT);
                }
                let wait_ms = if total_ticks == 0 {
                    100
                } else {
                    let now_ticks = now();
                    if now_ticks >= deadline {
                        let _ = qjs::async_fs::discard(op_id);
                        return Err(FS_ERR_TIMEOUT);
                    }
                    let remain_ticks = deadline - now_ticks;
                    ((remain_ticks.saturating_mul(1000)) / hz).max(1)
                };
                let _ = qjs::async_fs::wait_net_fetch(op_id, core::cmp::min(wait_ms, 100));
                continue;
            }
            if len < 0 {
                let rc = len as i32;
                let _ = qjs::async_fs::discard(op_id);
                let retriable = rc == NET_ERR_TIMEOUT_DNS
                    || rc == NET_ERR_TIMEOUT_TLS
                    || rc == NET_ERR_TIMEOUT_CONNECT
                    || rc == NET_ERR_TIMEOUT_BODY;
                if retriable && attempts < BOOTSTRAP_NET_FETCH_RETRIES {
                    let backoff_ticks = if hz == 0 {
                        0
                    } else {
                        ((BOOTSTRAP_NET_RETRY_BACKOFF_MS.saturating_mul(hz) + 999) / 1000).max(1)
                    };
                    let retry_deadline = if backoff_ticks == 0 {
                        0
                    } else {
                        now().saturating_add(backoff_ticks)
                    };
                    while backoff_ticks == 0 || now() < retry_deadline {
                        if total_ticks != 0 && now() >= deadline {
                            return Err(FS_ERR_TIMEOUT);
                        }
                        trueos_cabi_poll_once();
                        if backoff_ticks == 0 {
                            break;
                        }
                    }
                    break;
                }
                return Err(rc);
            }
            let got = qjs::async_fs::read_result(op_id, core::ptr::null_mut(), 0);
            if got < 0 {
                return Err(got as i32);
            }
            return Ok(());
        }
    }
}

unsafe fn compile_module_value_from_buf(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
    buf: *const u8,
    len: usize,
) -> qjs::JSValue {
    let flags = qjs::JS_EVAL_TYPE_MODULE | qjs::JS_EVAL_FLAG_COMPILE_ONLY;
    let src = if len == 0 || buf.is_null() {
        &[][..]
    } else {
        core::slice::from_raw_parts(buf, len)
    };
    let val = qjs::js_eval_bytes(ctx, src, module_name, flags);

    if val.is_exception() {
        return val;
    }
    if val.tag != qjs::JS_TAG_MODULE {
        qjs::js_free_value(ctx, val);
        return qjs::JSValue::exception();
    }
    val
}

unsafe fn compile_module_from_buf(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
    buf: *const u8,
    len: usize,
) -> *mut qjs::JSModuleDef {
    let val = compile_module_value_from_buf(ctx, module_name, buf, len);
    if val.is_exception() {
        return core::ptr::null_mut();
    }
    if val.tag != qjs::JS_TAG_MODULE {
        return core::ptr::null_mut();
    }

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

fn qjs_vendor_specifier(spec: &[u8]) -> Option<&'static [u8]> {
    match spec {
        // Browser renderer stack: resolve to local embedded vendor modules
        // instead of network fetches via esm.sh.
        b"pixi.js" => Some(b"/qjs/vendor/pixi.mjs"),
        b"parse5" => Some(b"/qjs/vendor/parse5.mjs"),
        b"yoga-layout" => Some(b"/qjs/vendor/yoga.mjs"),
        _ => None,
    }
}

pub(crate) unsafe fn normalize_with_mode(
    ctx: *mut qjs::JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    mode: NormalizeMode,
) -> *mut c_char {
    trace_str("qjs: normalize mode=");
    match mode {
        NormalizeMode::Base => trace_str("base"),
        NormalizeMode::Node => trace_str("node"),
    }
    trace_str(" base=");
    trace_cstr_or_null(module_base_name);
    trace_str(" spec=");
    trace_cstr_or_null(module_name);
    trace_nl();

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
        // esm.sh sometimes emits absolute Node-shim paths like `/node/url.mjs`.
        // TRUEOS embeds modules under `/qjs/**` (see build.rs::to_qjs_specifier),
        // so rewrite these to `/qjs/node/...`.
        if spec.starts_with(b"/node/") {
            let mut rewritten = Vec::new();
            rewritten.extend_from_slice(b"/qjs");
            rewritten.extend_from_slice(spec);
            log_normalized(&rewritten);
            return js_strdup(ctx, &rewritten);
        }

        let normalized = normalize_path_bytes(spec);
        log_normalized(&normalized);
        return js_strdup(ctx, &normalized);
    }

    // Bare specifiers.
    if !path_is_relative(spec) {
        if let Some(vendor) = qjs_vendor_specifier(spec) {
            log_normalized(vendor);
            return js_strdup(ctx, vendor);
        }

        // Always keep known TRUEOS native modules.
        if spec == b"complex"
            || spec == b"fs"
            || spec == b"trueos:browser_context"
            || spec == b"trueos:browser_navigator"
            || spec == b"trueos:browser_webgpu"
            || spec == b"cmd_stream"
            || spec == b"trueos:cmd_stream"
            || spec == b"worker_threads"
            || spec == b"trueos:yoga"
            || spec == b"yoga-native"
            || spec == b"three"
            || spec == b"trueos:threejs"
            || spec == b"threejs-native"
            || spec == b"process"
            || spec == b"node:process"
            || spec == b"node:worker_threads"
            || spec == b"path"
            || spec == b"node:path"
            || spec == b"trueos:lightningcss"
            || spec == b"lightningcss-native"
            || spec == b"trueos:lyon"
            || spec == b"lyon-native"
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
    let compiled_cache_path = compiled::compiled_cache_path_for_source(&cache_path);

    trace_str("qjs: url cache=");
    trace_bytes(&cache_path);
    trace_nl();

    // If the URL cache file is provided as an embedded module (via crates/trueos-qjs/app/cdn/*),
    // prefer that over touching the filesystem or network.
    if let Some(em) = embedded::find(&cache_path) {
        // Fast-path: build-time bytecode.
        if !em.bytecode.is_empty() {
            let v = qjs::JS_ReadObject(
                ctx,
                em.bytecode.as_ptr(),
                em.bytecode.len(),
                qjs::JS_READ_OBJ_BYTECODE,
            );
            if !v.is_exception() {
                if v.tag == qjs::JS_TAG_MODULE {
                    let md = v.u.ptr as *mut qjs::JSModuleDef;
                    qjs::js_free_value(ctx, v);
                    return md;
                }
                qjs::js_free_value(ctx, v);
            }
        }

        // Fallback: compile embedded source.
        trace_str("qjs: url embedded compile start\n");
        let v = compile_module_value_from_buf(ctx, module_name, em.src.as_ptr(), em.src.len());
        trace_str("qjs: url embedded compile done\n");

        if v.is_exception() || v.tag != qjs::JS_TAG_MODULE {
            if !v.is_exception() {
                qjs::js_free_value(ctx, v);
            }
            return core::ptr::null_mut();
        }

        // Optional: persist compiled module alongside the normal cache naming.
        compiled::persist_compiled_module(ctx, &compiled_cache_path, v);
        let md = v.u.ptr as *mut qjs::JSModuleDef;
        qjs::js_free_value(ctx, v);
        return md;
    }

    match compiled::try_load_compiled_module(ctx, &compiled_cache_path) {
        Ok(m) => {
            trace_str("qjs: compiled cache hit path=");
            trace_bytes(&compiled_cache_path);
            trace_nl();
            return m;
        }
        Err(rc) => {
            trace_str("qjs: compiled cache miss rc=");
            let mut tmp = Vec::new();
            push_i32_dec(&mut tmp, rc);
            trace_bytes(&tmp);
            trace_str(" path=");
            trace_bytes(&compiled_cache_path);
            trace_nl();
        }
    }

    // Fast-path: if the cached module is already present, avoid refetching.
    match read_file_js_malloc_rc(ctx, &cache_path) {
        Ok((buf, len)) => {
            trace_str("qjs: url cache hit len=");
            trace_usize_dec(len);
            trace_nl();
            trace_str("qjs: url compile start\n");
            let v = compile_module_value_from_buf(ctx, module_name, buf, len);
            trace_str("qjs: url compile done\n");
            qjs::js_free(ctx, buf as *mut core::ffi::c_void);
            if v.is_exception() || v.tag != qjs::JS_TAG_MODULE {
                if !v.is_exception() {
                    qjs::js_free_value(ctx, v);
                }
                return core::ptr::null_mut();
            }
            compiled::persist_compiled_module(ctx, &compiled_cache_path, v);
            let m = v.u.ptr as *mut qjs::JSModuleDef;
            qjs::js_free_value(ctx, v);
            return m;
        }
        Err(rc) => {
            trace_str("qjs: url cache miss rc=");
            let mut tmp = Vec::new();
            push_i32_dec(&mut tmp, rc as i32);
            trace_bytes(&tmp);
            trace_nl();
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

    trace_str("qjs: url prefetch url=");
    trace_bytes(url);
    trace_nl();
    if let Err(rc) = fetch_to_cache_rc_async(url, &cache_path, BOOTSTRAP_NET_TIMEOUT_MS) {
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
    trace_str("qjs: url prefetch ok\n");

    let (buf, len) = match read_file_js_malloc_rc(ctx, &cache_path) {
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
    trace_str("qjs: url compile start\n");
    let v = compile_module_value_from_buf(ctx, module_name, buf, len);
    trace_str("qjs: url compile done\n");
    qjs::js_free(ctx, buf as *mut core::ffi::c_void);
    if v.is_exception() || v.tag != qjs::JS_TAG_MODULE {
        if !v.is_exception() {
            qjs::js_free_value(ctx, v);
        }
        return core::ptr::null_mut();
    }
    compiled::persist_compiled_module(ctx, &compiled_cache_path, v);
    let m = v.u.ptr as *mut qjs::JSModuleDef;
    qjs::js_free_value(ctx, v);
    m
}

unsafe fn load_fs_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if module_name.is_null() {
        return core::ptr::null_mut();
    }

    let path = CStr::from_ptr(module_name).to_bytes();
    trace_str("qjs: fs load path=");
    trace_bytes(path);
    trace_nl();
    // Only attempt filesystem loading for absolute paths or explicit relative paths.
    if !(path_is_absolute(path) || path_is_relative(path)) {
        return core::ptr::null_mut();
    }

    // Embedded modules: load from the embedded blob.
    // IMPORTANT: do not consult/persist the on-disk compiled cache for embedded modules.
    // Otherwise an old `/qjs/*.qjsc` left on disk can override freshly embedded updates.
    if path_is_absolute(path) {
        if let Some(m) = embedded::find(path) {
            // Fast-path: if we have build-time bytecode, load it directly.
            if !m.bytecode.is_empty() {
                let v = qjs::JS_ReadObject(
                    ctx,
                    m.bytecode.as_ptr(),
                    m.bytecode.len(),
                    qjs::JS_READ_OBJ_BYTECODE,
                );
                if !v.is_exception() {
                    if v.tag == qjs::JS_TAG_MODULE {
                        let md = v.u.ptr as *mut qjs::JSModuleDef;
                        qjs::js_free_value(ctx, v);
                        return md;
                    }
                    qjs::js_free_value(ctx, v);
                }
            }

            trace_str("qjs: embedded compile start\n");
            let v = compile_module_value_from_buf(ctx, module_name, m.src.as_ptr(), m.src.len());
            trace_str("qjs: embedded compile done\n");

            if v.is_exception() || v.tag != qjs::JS_TAG_MODULE {
                if !v.is_exception() {
                    qjs::js_free_value(ctx, v);
                }
                return core::ptr::null_mut();
            }
            let md = v.u.ptr as *mut qjs::JSModuleDef;
            qjs::js_free_value(ctx, v);
            return md;
        }
    }

    let (buf, len) = match read_file_js_malloc_rc(ctx, path) {
        Ok(v) => v,
        Err(_) => {
            let mut msg = Vec::new();
            msg.extend_from_slice(b"read module failed path=");
            msg.extend_from_slice(path);
            throw_error(ctx, &msg);
            return core::ptr::null_mut();
        }
    };

    trace_str("qjs: fs read len=");
    trace_usize_dec(len);
    trace_nl();
    trace_str("qjs: fs compile start\n");
    let m = compile_module_from_buf(ctx, module_name, buf, len);
    trace_str("qjs: fs compile done\n");
    qjs::js_free(ctx, buf as *mut core::ffi::c_void);
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
        trace_str("qjs: loader native ok\n");
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

    trace_str("qjs: loader fs\n");
    load_fs_module(ctx, module_name)
}

pub(crate) fn normalize_path_bytes(path: &[u8]) -> Vec<u8> {
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
            if parts.pop().is_some() {
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
    qjs::JS_SetModuleLoaderFunc(
        rt,
        Some(trueos_module_normalize),
        Some(trueos_module_loader),
        core::ptr::null_mut(),
    );
}
