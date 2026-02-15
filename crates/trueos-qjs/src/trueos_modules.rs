extern crate alloc;

use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use core::ffi::{c_char, c_int, CStr};
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_time_driver::{now, TICK_HZ};
use spin::Mutex;

use crate as qjs;

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_poll_once();
    // NOTE: The WebGL shim targets the stable gfx layer (trueos_gfx_core), not a concrete GPU.
    // Backends (software / virtio-gpu / Xe) sit behind this ABI.
    fn trueos_cabi_gfx_draw_rgb_triangles(clear_rgb: u32, vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
}

include!("../../../src/surface/cabi_codes.rs");

static PROCESS_ARGV: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static PROCESS_CWD: Mutex<Vec<u8>> = Mutex::new(Vec::new());

// --- Minimal WebGL-ish shim state ---

static WEBGL_NEXT_ID: AtomicU32 = AtomicU32::new(1);
static WEBGL_DID_LOG_DRAW: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_UNIFORM_LOOKUPS: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_UNIFORM_UPLOADS: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_DRAW_MODE: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_DRAW_DROPS: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_GET_CONTEXT: AtomicU32 = AtomicU32::new(0);
static WEBGL_LOG_CREATE_HANDLE: AtomicU32 = AtomicU32::new(0);

#[inline]
fn webgl_log_draw_drop(where_: &str, why: &str) {
    if WEBGL_LOG_DRAW_DROPS.fetch_add(1, Ordering::Relaxed) < 24 {
        log_str("qjs-webgl: drop ");
        log_str(where_);
        log_str(" reason=");
        log_str(why);
        log_str("\n");
    }
}

#[derive(Clone, Copy, Default)]
struct WebGlVertexAttrib {
    enabled: bool,
    size: i32,
    ty: u32,
    normalized: bool,
    stride: i32,
    offset: usize,
    buffer: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WebGlUniformKind {
    Other,
    TranslationMatrix,
    ProjectionMatrix,
}

struct WebGlState {
    array_buffer: u32,
    element_array_buffer: u32,
    buffers: BTreeMap<u32, Vec<u8>>,
    attribs: BTreeMap<u32, WebGlVertexAttrib>,
    uniform_locs: BTreeMap<u32, WebGlUniformKind>,
    uniform_name_to_loc: BTreeMap<Vec<u8>, u32>,
    next_uniform_loc: u32,
    translation_matrix: [f32; 9],
    projection_matrix: [f32; 9],
    has_translation_matrix: bool,
    has_projection_matrix: bool,
    clear_rgb: u32,
    viewport_w: i32,
    viewport_h: i32,
}

static WEBGL_STATE: Mutex<WebGlState> = Mutex::new(WebGlState {
    array_buffer: 0,
    element_array_buffer: 0,
    buffers: BTreeMap::new(),
    attribs: BTreeMap::new(),
    uniform_locs: BTreeMap::new(),
    uniform_name_to_loc: BTreeMap::new(),
    next_uniform_loc: 1,
    translation_matrix: [
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
    ],
    projection_matrix: [
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
    ],
    has_translation_matrix: false,
    has_projection_matrix: false,
    // TRUEOS default console blue.
    clear_rgb: 0x00_08_18_30,
    viewport_w: 0,
    viewport_h: 0,
});

pub fn set_process_argv(args: &[&str]) {
    let mut out: Vec<Vec<u8>> = Vec::new();
    for arg in args {
        if arg.is_empty() {
            continue;
        }
        out.push(arg.as_bytes().to_vec());
    }
    if out.is_empty() {
        out.push(b"qjs".to_vec());
    }
    *PROCESS_ARGV.lock() = out;
}

fn process_argv_snapshot() -> Vec<Vec<u8>> {
    let snapshot = PROCESS_ARGV.lock().clone();
    if snapshot.is_empty() {
        vec![b"qjs".to_vec()]
    } else {
        snapshot
    }
}

fn process_cwd_snapshot() -> Vec<u8> {
    let cwd = PROCESS_CWD.lock().clone();
    if cwd.is_empty() {
        b"/".to_vec()
    } else {
        cwd
    }
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
fn trace_bytes(bytes: &[u8]) {
    if qjs_loader_trace_enabled() {
        log_bytes(bytes);
    }
}

#[inline]
fn trace_str(s: &str) {
    if qjs_loader_trace_enabled() {
        log_str(s);
    }
}

#[inline]
fn trace_nl() {
    if qjs_loader_trace_enabled() {
        log_nl();
    }
}

#[inline]
fn trace_usize_dec(v: usize) {
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

#[inline]
fn js_int32(v: i32) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: v },
        tag: qjs::JS_TAG_INT,
    }
}

#[inline]
fn js_null() -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion { int32: 0 },
        tag: qjs::JS_TAG_NULL,
    }
}

unsafe extern "C" fn qjs_process_next_tick(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let func = args[0];

    let call_argc = (argc - 1).max(0);
    let call_argv = if argc > 1 {
        unsafe { argv.add(1) }
    } else {
        core::ptr::null()
    };

    // Best-effort: call immediately. Later this can be wired to a real microtask/"nextTick" queue.
    unsafe { qjs::JS_Call(ctx, func, qjs::JSValue::undefined(), call_argc, call_argv) };
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_process_cwd(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let cwd = process_cwd_snapshot();
    qjs::JS_NewStringLen(ctx, cwd.as_ptr() as *const c_char, cwd.len())
}

unsafe extern "C" fn qjs_process_chdir(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        throw_error(ctx, b"process.chdir requires path");
        return qjs::JSValue::exception();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0);
    if cstr.is_null() {
        return qjs::JSValue::exception();
    }
    let input = core::slice::from_raw_parts(cstr as *const u8, len);
    if input.is_empty() {
        qjs::JS_FreeCString(ctx, cstr);
        throw_error(ctx, b"process.chdir empty path");
        return qjs::JSValue::exception();
    }

    let normalized = if input.starts_with(b"/") {
        normalize_path_bytes(input)
    } else {
        let mut joined = process_cwd_snapshot();
        if !joined.ends_with(b"/") {
            joined.push(b'/');
        }
        joined.extend_from_slice(input);
        normalize_path_bytes(joined.as_slice())
    };
    qjs::JS_FreeCString(ctx, cstr);

    let mut final_cwd = if normalized.is_empty() {
        b"/".to_vec()
    } else if normalized.starts_with(b"/") {
        normalized
    } else {
        let mut p = Vec::with_capacity(normalized.len() + 1);
        p.push(b'/');
        p.extend_from_slice(&normalized);
        p
    };
    if final_cwd.len() > 1 && final_cwd.ends_with(b"/") {
        final_cwd.pop();
    }
    *PROCESS_CWD.lock() = final_cwd;
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_process_uptime(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // TODO: wire to kernel monotonic time.
    unsafe { qjs::JS_NewFloat64(core::ptr::null_mut(), 0.0) }
}

unsafe extern "C" fn qjs_process_hrtime(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // Node-style: [seconds, nanoseconds]. Best-effort zeros for now.
    let arr = unsafe { qjs::JS_NewArray(ctx) };
    if arr.is_exception() {
        return arr;
    }
    let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, 0, js_int32(0)) };
    let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, 1, js_int32(0)) };
    arr
}

unsafe extern "C" fn qjs_path_join(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return unsafe { qjs::JS_NewStringLen(ctx, b".\0".as_ptr() as *const c_char, 1) };
    }

    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let mut out: Vec<u8> = Vec::new();

    for &val in args {
        let mut len: usize = 0;
        let cstr = unsafe { qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, val, 0) };
        if cstr.is_null() {
            return qjs::JSValue::exception();
        }

        let bytes = unsafe { core::slice::from_raw_parts(cstr as *const u8, len) };
        if !bytes.is_empty() {
            if !out.is_empty() && out[out.len() - 1] != b'/' && bytes[0] != b'/' {
                out.push(b'/');
            }
            out.extend_from_slice(bytes);
        }

        unsafe { qjs::JS_FreeCString(ctx, cstr) };
    }

    if out.is_empty() {
        out.push(b'.');
    }

    unsafe { qjs::JS_NewStringLen(ctx, out.as_ptr() as *const c_char, out.len()) }
}

fn path_trim_trailing_slashes(mut p: &[u8]) -> &[u8] {
    while p.len() > 1 && p.ends_with(b"/") {
        p = &p[..p.len() - 1];
    }
    p
}

unsafe fn js_arg_to_bytes(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<Vec<u8>> {
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, v, 0);
    if cstr.is_null() {
        return None;
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len).to_vec();
    qjs::JS_FreeCString(ctx, cstr);
    Some(bytes)
}

unsafe extern "C" fn qjs_path_dirname(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return qjs::JS_NewStringLen(ctx, b".\0".as_ptr() as *const c_char, 1);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(mut path) = js_arg_to_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };
    if path.is_empty() {
        return qjs::JS_NewStringLen(ctx, b".\0".as_ptr() as *const c_char, 1);
    }
    let trimmed = path_trim_trailing_slashes(path.as_slice());
    if let Some(pos) = trimmed.iter().rposition(|&b| b == b'/') {
        if pos == 0 {
            return qjs::JS_NewStringLen(ctx, b"/\0".as_ptr() as *const c_char, 1);
        }
        path.truncate(pos);
        return qjs::JS_NewStringLen(ctx, path.as_ptr() as *const c_char, path.len());
    }
    qjs::JS_NewStringLen(ctx, b".\0".as_ptr() as *const c_char, 1)
}

unsafe extern "C" fn qjs_path_basename(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(path) = js_arg_to_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };
    if path.is_empty() {
        return qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0);
    }
    let trimmed = path_trim_trailing_slashes(path.as_slice());
    if trimmed == b"/" {
        return qjs::JS_NewStringLen(ctx, b"/\0".as_ptr() as *const c_char, 1);
    }
    let base = if let Some(pos) = trimmed.iter().rposition(|&b| b == b'/') {
        &trimmed[pos + 1..]
    } else {
        trimmed
    };
    qjs::JS_NewStringLen(ctx, base.as_ptr() as *const c_char, base.len())
}

unsafe extern "C" fn qjs_path_extname(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some(path) = js_arg_to_bytes(ctx, args[0]) else {
        return qjs::JSValue::exception();
    };
    let trimmed = path_trim_trailing_slashes(path.as_slice());
    let base = if let Some(pos) = trimmed.iter().rposition(|&b| b == b'/') {
        &trimmed[pos + 1..]
    } else {
        trimmed
    };
    if base.is_empty() {
        return qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0);
    }
    let Some(dot) = base.iter().rposition(|&b| b == b'.') else {
        return qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0);
    };
    if dot == 0 || base == b"." || base == b".." {
        return qjs::JS_NewStringLen(ctx, b"\0".as_ptr() as *const c_char, 0);
    }
    let ext = &base[dot..];
    qjs::JS_NewStringLen(ctx, ext.as_ptr() as *const c_char, ext.len())
}

unsafe extern "C" fn qjs_path_resolve(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc <= 0 {
        return qjs::JS_NewStringLen(ctx, b"/\0".as_ptr() as *const c_char, 1);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut acc: Vec<u8> = Vec::new();
    let mut seen_abs = false;
    for &v in args {
        let Some(part) = js_arg_to_bytes(ctx, v) else {
            return qjs::JSValue::exception();
        };
        if part.is_empty() {
            continue;
        }
        if part.starts_with(b"/") {
            acc.clear();
            acc.extend_from_slice(&part);
            seen_abs = true;
            continue;
        }
        if !acc.is_empty() && !acc.ends_with(b"/") {
            acc.push(b'/');
        }
        acc.extend_from_slice(&part);
    }
    if !seen_abs {
        let mut prefixed = Vec::with_capacity(acc.len() + 1);
        prefixed.push(b'/');
        prefixed.extend_from_slice(&acc);
        acc = prefixed;
    }
    let normalized = normalize_path_bytes(acc.as_slice());
    let out = if normalized.is_empty() { b"/".as_slice() } else { normalized.as_slice() };
    qjs::JS_NewStringLen(ctx, out.as_ptr() as *const c_char, out.len())
}

unsafe extern "C" fn qjs_path_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    if ctx.is_null() || m.is_null() {
        return -1;
    }

    let join_name = b"join\0";
    let join_fn = unsafe {
        qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_path_join),
            join_name.as_ptr() as *const c_char,
            2,
            qjs::JS_CFUNC_GENERIC,
            0,
        )
    };
    if unsafe { qjs::JS_SetModuleExport(ctx, m, join_name.as_ptr() as *const c_char, join_fn) } < 0 {
        return -1;
    }

    let dirname_name = b"dirname\0";
    let dirname_fn = unsafe {
        qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_path_dirname),
            dirname_name.as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        )
    };
    if unsafe { qjs::JS_SetModuleExport(ctx, m, dirname_name.as_ptr() as *const c_char, dirname_fn) } < 0 {
        return -1;
    }

    let basename_name = b"basename\0";
    let basename_fn = unsafe {
        qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_path_basename),
            basename_name.as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        )
    };
    if unsafe { qjs::JS_SetModuleExport(ctx, m, basename_name.as_ptr() as *const c_char, basename_fn) } < 0 {
        return -1;
    }

    let extname_name = b"extname\0";
    let extname_fn = unsafe {
        qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_path_extname),
            extname_name.as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        )
    };
    if unsafe { qjs::JS_SetModuleExport(ctx, m, extname_name.as_ptr() as *const c_char, extname_fn) } < 0 {
        return -1;
    }

    let resolve_name = b"resolve\0";
    let resolve_fn = unsafe {
        qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_path_resolve),
            resolve_name.as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        )
    };
    if unsafe { qjs::JS_SetModuleExport(ctx, m, resolve_name.as_ptr() as *const c_char, resolve_fn) } < 0 {
        return -1;
    }

    0
}

unsafe extern "C" fn qjs_worker_threads_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    qjs::workers::install_worker_threads_exports(ctx, m)
}

unsafe fn ensure_global_process(ctx: *mut qjs::JSContext) -> Result<qjs::JSValue, qjs::JSValue> {
    let global = qjs::JS_GetGlobalObject(ctx);
    let name = b"process\0";
    let mut proc = qjs::JS_GetPropertyStr(ctx, global, name.as_ptr() as *const c_char);
    if proc.is_exception() {
        qjs::js_free_value(ctx, global);
        return Err(qjs::JSValue::exception());
    }

    if proc.tag == qjs::JS_TAG_UNDEFINED || proc.tag == qjs::JS_TAG_NULL {
        // Drop the undefined/null handle and create a fresh process object.
        qjs::js_free_value(ctx, proc);

        let obj = qjs::JS_NewObject(ctx);
        if obj.is_exception() {
            qjs::js_free_value(ctx, global);
            return Err(qjs::JSValue::exception());
        }

        // env: plain object (userland can mutate)
        let env = qjs::JS_NewObject(ctx);
        if env.is_exception() {
            qjs::js_free_value(ctx, global);
            qjs::js_free_value(ctx, obj);
            return Err(qjs::JSValue::exception());
        }
        let node_env = qjs::JS_NewStringLen(ctx, b"production".as_ptr() as *const c_char, 10);
        let home = qjs::JS_NewStringLen(ctx, b"/".as_ptr() as *const c_char, 1);
        let pwd = qjs::JS_NewStringLen(ctx, b"/".as_ptr() as *const c_char, 1);
        let tmpdir = qjs::JS_NewStringLen(ctx, b"/tmp".as_ptr() as *const c_char, 4);
        let term = qjs::JS_NewStringLen(ctx, b"xterm-256color".as_ptr() as *const c_char, 14);
        let path = qjs::JS_NewStringLen(ctx, b"/bin:/usr/bin".as_ptr() as *const c_char, 13);
        let _ = qjs::JS_SetPropertyStr(ctx, env, b"NODE_ENV\0".as_ptr() as *const c_char, node_env);
        let _ = qjs::JS_SetPropertyStr(ctx, env, b"HOME\0".as_ptr() as *const c_char, home);
        let _ = qjs::JS_SetPropertyStr(ctx, env, b"PWD\0".as_ptr() as *const c_char, pwd);
        let _ = qjs::JS_SetPropertyStr(ctx, env, b"TMPDIR\0".as_ptr() as *const c_char, tmpdir);
        let _ = qjs::JS_SetPropertyStr(ctx, env, b"TERM\0".as_ptr() as *const c_char, term);
        let _ = qjs::JS_SetPropertyStr(ctx, env, b"PATH\0".as_ptr() as *const c_char, path);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"env\0".as_ptr() as *const c_char, env);

        // argv: shell-provided (fallback to ["qjs"]).
        let argv = qjs::JS_NewArray(ctx);
        if argv.is_exception() {
            qjs::js_free_value(ctx, global);
            qjs::js_free_value(ctx, obj);
            return Err(qjs::JSValue::exception());
        }
        let argv_items = process_argv_snapshot();
        for (idx, item) in argv_items.iter().enumerate() {
            let v = qjs::JS_NewStringLen(ctx, item.as_ptr() as *const c_char, item.len());
            let _ = qjs::JS_SetPropertyUint32(ctx, argv, idx as u32, v);
        }
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"argv\0".as_ptr() as *const c_char, argv);

        // platform/arch/version
        let platform = qjs::JS_NewStringLen(ctx, b"trueos".as_ptr() as *const c_char, 6);
        let arch = qjs::JS_NewStringLen(ctx, b"x64".as_ptr() as *const c_char, 3);
        let version = qjs::JS_NewStringLen(ctx, b"0.0.0-trueos".as_ptr() as *const c_char, 12);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"platform\0".as_ptr() as *const c_char, platform);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"arch\0".as_ptr() as *const c_char, arch);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"version\0".as_ptr() as *const c_char, version);
        let exec_path = qjs::JS_NewStringLen(ctx, b"/bin/qjs".as_ptr() as *const c_char, 8);
        let title = qjs::JS_NewStringLen(ctx, b"trueos-qjs".as_ptr() as *const c_char, 10);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"execPath\0".as_ptr() as *const c_char, exec_path);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"title\0".as_ptr() as *const c_char, title);
        let release = qjs::JS_NewObject(ctx);
        if release.is_exception() {
            qjs::js_free_value(ctx, global);
            qjs::js_free_value(ctx, obj);
            return Err(qjs::JSValue::exception());
        }
        let rel_name = qjs::JS_NewStringLen(ctx, b"node".as_ptr() as *const c_char, 4);
        let rel_lts = qjs::JS_NewStringLen(ctx, b"trueos".as_ptr() as *const c_char, 6);
        let _ = qjs::JS_SetPropertyStr(ctx, release, b"name\0".as_ptr() as *const c_char, rel_name);
        let _ = qjs::JS_SetPropertyStr(ctx, release, b"lts\0".as_ptr() as *const c_char, rel_lts);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"release\0".as_ptr() as *const c_char, release);
        let versions = qjs::JS_NewObject(ctx);
        if versions.is_exception() {
            qjs::js_free_value(ctx, global);
            qjs::js_free_value(ctx, obj);
            return Err(qjs::JSValue::exception());
        }
        let node_v = qjs::JS_NewStringLen(ctx, b"18.0.0-trueos".as_ptr() as *const c_char, 13);
        let v8_v = qjs::JS_NewStringLen(ctx, b"quickjs".as_ptr() as *const c_char, 7);
        let uv_v = qjs::JS_NewStringLen(ctx, b"0.0.0".as_ptr() as *const c_char, 5);
        let modules_v = qjs::JS_NewStringLen(ctx, b"108".as_ptr() as *const c_char, 3);
        let openssl_v = qjs::JS_NewStringLen(ctx, b"0.0.0".as_ptr() as *const c_char, 5);
        let _ = qjs::JS_SetPropertyStr(ctx, versions, b"node\0".as_ptr() as *const c_char, node_v);
        let _ = qjs::JS_SetPropertyStr(ctx, versions, b"v8\0".as_ptr() as *const c_char, v8_v);
        let _ = qjs::JS_SetPropertyStr(ctx, versions, b"uv\0".as_ptr() as *const c_char, uv_v);
        let _ = qjs::JS_SetPropertyStr(ctx, versions, b"modules\0".as_ptr() as *const c_char, modules_v);
        let _ = qjs::JS_SetPropertyStr(ctx, versions, b"openssl\0".as_ptr() as *const c_char, openssl_v);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"versions\0".as_ptr() as *const c_char, versions);

        // pid (placeholder)
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"pid\0".as_ptr() as *const c_char, js_int32(1));

        // Functions: nextTick/cwd/chdir/hrtime/uptime
        let next_tick = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_next_tick),
            b"nextTick\0".as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let cwd = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_cwd),
            b"cwd\0".as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let chdir = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_chdir),
            b"chdir\0".as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let hrtime = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_hrtime),
            b"hrtime\0".as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let uptime = qjs::JS_NewCFunction2(
            ctx,
            Some(qjs_process_uptime),
            b"uptime\0".as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"nextTick\0".as_ptr() as *const c_char, next_tick);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"cwd\0".as_ptr() as *const c_char, cwd);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"chdir\0".as_ptr() as *const c_char, chdir);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"hrtime\0".as_ptr() as *const c_char, hrtime);
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"uptime\0".as_ptr() as *const c_char, uptime);

        // Attach to globalThis.process (JS_SetPropertyStr consumes `obj` on success).
        let _ = qjs::JS_SetPropertyStr(ctx, global, name.as_ptr() as *const c_char, obj);

        // Re-read the process object so we have a handle we can return/inspect.
        proc = qjs::JS_GetPropertyStr(ctx, global, name.as_ptr() as *const c_char);
        if proc.is_exception() {
            qjs::js_free_value(ctx, global);
            return Err(qjs::JSValue::exception());
        }
    }

    qjs::js_free_value(ctx, global);
    Ok(proc)
}

unsafe extern "C" fn qjs_process_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    let proc = match ensure_global_process(ctx) {
        Ok(v) => v,
        Err(_) => return -1,
    };

    // Named exports: env/argv/cwd/chdir/nextTick/hrtime/uptime + compatibility fields.
    let env = qjs::JS_GetPropertyStr(ctx, proc, b"env\0".as_ptr() as *const c_char);
    let argv = qjs::JS_GetPropertyStr(ctx, proc, b"argv\0".as_ptr() as *const c_char);
    let cwd = qjs::JS_GetPropertyStr(ctx, proc, b"cwd\0".as_ptr() as *const c_char);
    let chdir = qjs::JS_GetPropertyStr(ctx, proc, b"chdir\0".as_ptr() as *const c_char);
    let next_tick = qjs::JS_GetPropertyStr(ctx, proc, b"nextTick\0".as_ptr() as *const c_char);
    let hrtime = qjs::JS_GetPropertyStr(ctx, proc, b"hrtime\0".as_ptr() as *const c_char);
    let uptime = qjs::JS_GetPropertyStr(ctx, proc, b"uptime\0".as_ptr() as *const c_char);
    let versions = qjs::JS_GetPropertyStr(ctx, proc, b"versions\0".as_ptr() as *const c_char);
    let platform = qjs::JS_GetPropertyStr(ctx, proc, b"platform\0".as_ptr() as *const c_char);
    let arch = qjs::JS_GetPropertyStr(ctx, proc, b"arch\0".as_ptr() as *const c_char);
    let release = qjs::JS_GetPropertyStr(ctx, proc, b"release\0".as_ptr() as *const c_char);

    // Exporting consumes the values, so don't free them after.
    let _ = qjs::JS_SetModuleExport(ctx, m, b"env\0".as_ptr() as *const c_char, env);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"argv\0".as_ptr() as *const c_char, argv);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"cwd\0".as_ptr() as *const c_char, cwd);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"chdir\0".as_ptr() as *const c_char, chdir);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"nextTick\0".as_ptr() as *const c_char, next_tick);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"hrtime\0".as_ptr() as *const c_char, hrtime);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"uptime\0".as_ptr() as *const c_char, uptime);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"versions\0".as_ptr() as *const c_char, versions);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"platform\0".as_ptr() as *const c_char, platform);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"arch\0".as_ptr() as *const c_char, arch);
    let _ = qjs::JS_SetModuleExport(ctx, m, b"release\0".as_ptr() as *const c_char, release);

    let _ = qjs::JS_SetModuleExport(ctx, m, b"default\0".as_ptr() as *const c_char, proc);
    0
}

/// Install per-context globals that many npm packages assume exist.
///
/// Currently provides:
/// - `globalThis.process`
pub unsafe fn install_globals(ctx: *mut qjs::JSContext) {
    let Ok(proc) = ensure_global_process(ctx) else {
        return;
    };
    // Keep globalThis.process installed; drop our local handle.
    qjs::js_free_value(ctx, proc);

    // Minimal browser-ish globals for early library bring-up.
    // This is intentionally tiny: enough for feature-detection and import-time code paths.
    let _ = ensure_global_window_document_webgl(ctx);
}

unsafe fn ensure_global_window_document_webgl(ctx: *mut qjs::JSContext) -> Result<(), ()> {
    if ctx.is_null() {
        return Err(());
    }

    // globalThis
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return Err(());
    }

    // window/self -> globalThis
    {
        let win_key = b"window\0";
        let self_key = b"self\0";
        let win_val = qjs::js_dup_value(ctx, global);
        let self_val = qjs::js_dup_value(ctx, global);
        let _ = qjs::JS_SetPropertyStr(ctx, global, win_key.as_ptr() as *const c_char, win_val);
        let _ = qjs::JS_SetPropertyStr(ctx, global, self_key.as_ptr() as *const c_char, self_val);
    }

    // navigator
    {
        let nav_key = b"navigator\0";
        let nav = qjs::JS_NewObject(ctx);
        if !nav.is_exception() {
            let ua_key = b"userAgent\0";
            let ua = b"TRUEOS-QJS\0";
            let ua_val = qjs::JS_NewStringLen(ctx, ua.as_ptr() as *const c_char, ua.len() - 1);
            let _ = qjs::JS_SetPropertyStr(ctx, nav, ua_key.as_ptr() as *const c_char, ua_val);
            let _ = qjs::JS_SetPropertyStr(ctx, global, nav_key.as_ptr() as *const c_char, nav);
        } else {
            // nav is exception; drop it
            qjs::js_free_value(ctx, nav);
        }
    }

    // Install a singleton `__trueos_gl` object and a `document.createElement('canvas')` that
    // returns a canvas object whose getContext() returns the singleton.
    let gl_obj = ensure_global_trueos_webgl_singleton(ctx, global);
    let doc_obj = ensure_global_document(ctx, global, gl_obj);
    qjs::js_free_value(ctx, doc_obj);
    qjs::js_free_value(ctx, gl_obj);
    qjs::js_free_value(ctx, global);
    Ok(())
}

unsafe fn ensure_global_trueos_webgl_singleton(
    ctx: *mut qjs::JSContext,
    global: qjs::JSValue,
) -> qjs::JSValue {
    let key = b"__trueos_gl\0";
    let existing = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    if !existing.is_exception() && existing.tag != qjs::JS_TAG_UNDEFINED {
        return existing;
    }
    qjs::js_free_value(ctx, existing);

    // --- WebGL shim functions (minimal) ---
    // These implement just enough flow to bridge a triangle/rect draw into the kernel gfx layer.

    unsafe extern "C" fn gl_noop(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_return_null(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_null()
    }

    unsafe extern "C" fn gl_return_true(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue {
            u: qjs::JSValueUnion { int32: 1 },
            tag: qjs::JS_TAG_BOOL,
        }
    }

    unsafe extern "C" fn gl_return_false(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue {
            u: qjs::JSValueUnion { int32: 0 },
            tag: qjs::JS_TAG_BOOL,
        }
    }

    unsafe extern "C" fn gl_create_handle(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let id = WEBGL_NEXT_ID.fetch_add(1, Ordering::Relaxed);
        {
            let mut st = WEBGL_STATE.lock();
            st.buffers.entry(id).or_insert_with(Vec::new);
        }
        if WEBGL_LOG_CREATE_HANDLE.fetch_add(1, Ordering::Relaxed) < 12 {
            log_str("qjs-webgl: createHandle id=");
            log_usize_dec(id as usize);
            log_str("\n");
        }
        js_int32(id as i32)
    }

    fn classify_uniform_name(name: &[u8]) -> WebGlUniformKind {
        let raw = if let Some(base) = name.strip_suffix(b"[0]") {
            base
        } else {
            name
        };
        if raw.eq_ignore_ascii_case(b"translationMatrix") {
            WebGlUniformKind::TranslationMatrix
        } else if raw.eq_ignore_ascii_case(b"projectionMatrix") {
            WebGlUniformKind::ProjectionMatrix
        } else {
            WebGlUniformKind::Other
        }
    }

    fn mat3_mul_vec3(m: &[f32; 9], x: f32, y: f32, z: f32) -> (f32, f32, f32) {
        // Column-major mat3, matching GLSL/WebGL uniform layout.
        let ox = m[0] * x + m[3] * y + m[6] * z;
        let oy = m[1] * x + m[4] * y + m[7] * z;
        let oz = m[2] * x + m[5] * y + m[8] * z;
        (ox, oy, oz)
    }

    fn mat3_transpose(m: [f32; 9]) -> [f32; 9] {
        [
            m[0], m[3], m[6],
            m[1], m[4], m[7],
            m[2], m[5], m[8],
        ]
    }

    unsafe extern "C" fn gl_get_uniform_location(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return js_null();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let cstr = qjs::js_to_cstring(ctx, args[1]);
        if cstr.is_null() {
            return js_null();
        }
        let name = CStr::from_ptr(cstr).to_bytes().to_vec();
        qjs::JS_FreeCString(ctx, cstr);
        if name.is_empty() {
            return js_null();
        }

        let mut st = WEBGL_STATE.lock();
        if let Some(&loc) = st.uniform_name_to_loc.get(&name) {
            return js_int32(loc as i32);
        }
        let loc = st.next_uniform_loc.max(1);
        st.next_uniform_loc = loc.saturating_add(1);
        st.uniform_name_to_loc.insert(name.clone(), loc);
        let kind = classify_uniform_name(name.as_slice());
        st.uniform_locs.insert(loc, kind);
        if WEBGL_LOG_UNIFORM_LOOKUPS.fetch_add(1, Ordering::Relaxed) < 8 {
            log_str("qjs-webgl: getUniformLocation name=");
            log_bytes(name.as_slice());
            log_str(" loc=");
            log_usize_dec(loc as usize);
            log_str(" kind=");
            match kind {
                WebGlUniformKind::TranslationMatrix => log_str("translation"),
                WebGlUniformKind::ProjectionMatrix => log_str("projection"),
                WebGlUniformKind::Other => log_str("other"),
            }
            log_str("\n");
        }
        js_int32(loc as i32)
    }

    unsafe extern "C" fn gl_uniform_matrix3fv(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut loc_f: f64 = 0.0;
        let mut transpose_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut loc_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut transpose_f as *mut f64, args[1]);
        let loc = (loc_f as i32).max(0) as u32;
        if loc == 0 {
            return qjs::JSValue::undefined();
        }

        let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[2]) else {
            return qjs::JSValue::undefined();
        };
        let bytes = core::slice::from_raw_parts(ptr, len);
        if bytes.len() < 9 * 4 {
            return qjs::JSValue::undefined();
        }

        let mut mat = [0.0f32; 9];
        for (i, slot) in mat.iter_mut().enumerate() {
            let Some(v) = read_f32_le(bytes, i * 4) else {
                return qjs::JSValue::undefined();
            };
            *slot = v;
        }
        if transpose_f != 0.0 {
            mat = mat3_transpose(mat);
        }

        let mut st = WEBGL_STATE.lock();
        match st.uniform_locs.get(&loc).copied().unwrap_or(WebGlUniformKind::Other) {
            WebGlUniformKind::TranslationMatrix => {
                st.translation_matrix = mat;
                st.has_translation_matrix = true;
            }
            WebGlUniformKind::ProjectionMatrix => {
                st.projection_matrix = mat;
                st.has_projection_matrix = true;
            }
            WebGlUniformKind::Other => {}
        }
        if WEBGL_LOG_UNIFORM_UPLOADS.fetch_add(1, Ordering::Relaxed) < 16 {
            log_str("qjs-webgl: uniformMatrix3fv loc=");
            log_usize_dec(loc as usize);
            log_str(" transpose=");
            log_usize_dec((transpose_f != 0.0) as usize);
            log_str("\n");
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_bind_buffer(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 2 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut target_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]);
        let target = target_f as i32;
        let mut buf_id_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut buf_id_f as *mut f64, args[1]) != 0 {
            return qjs::JSValue::undefined();
        }
        let buf_id = buf_id_f as i32;
        let mut st = WEBGL_STATE.lock();
        let buf_id = if buf_id > 0 { buf_id as u32 } else { 0 };
        match target as u32 {
            0x8892 => st.array_buffer = buf_id,         // ARRAY_BUFFER
            0x8893 => st.element_array_buffer = buf_id, // ELEMENT_ARRAY_BUFFER
            _ => {}
        }
        qjs::JSValue::undefined()
    }

    unsafe fn js_get_arraybuffer_view(
        ctx: *mut qjs::JSContext,
        val: qjs::JSValueConst,
    ) -> Option<(*const u8, usize)> {
        // Try TypedArray first.
        let mut byte_off: usize = 0;
        let mut byte_len: usize = 0;
        let mut bpe: usize = 0;
        let ab = qjs::JS_GetTypedArrayBuffer(
            ctx,
            val,
            &mut byte_off as *mut usize,
            &mut byte_len as *mut usize,
            &mut bpe as *mut usize,
        );
        if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED {
            let mut total: usize = 0;
            let ptr = qjs::JS_GetArrayBuffer(ctx, &mut total as *mut usize, ab);
            qjs::js_free_value(ctx, ab);
            if !ptr.is_null() {
                let start = byte_off.min(total);
                let end = start.saturating_add(byte_len).min(total);
                return Some((unsafe { ptr.add(start) } as *const u8, end.saturating_sub(start)));
            }
        } else {
            qjs::js_free_value(ctx, ab);
        }

        // Then plain ArrayBuffer.
        let mut total: usize = 0;
        let ptr = qjs::JS_GetArrayBuffer(ctx, &mut total as *mut usize, val);
        if ptr.is_null() {
            return None;
        }
        Some((ptr as *const u8, total))
    }

    unsafe extern "C" fn gl_buffer_data(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut target_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]);
        let target = target_f as i32;

        // WebGL allows bufferData(target, size, usage) as well as bufferData(target, data, usage).
        let mut numeric_size: f64 = 0.0;
        let data_opt = if qjs::JS_ToFloat64(ctx, &mut numeric_size as *mut f64, args[1]) == 0 {
            None
        } else {
            js_get_arraybuffer_view(ctx, args[1])
        };

        let mut st = WEBGL_STATE.lock();
        let buf_id = match target as u32 {
            0x8892 => st.array_buffer,         // ARRAY_BUFFER
            0x8893 => st.element_array_buffer, // ELEMENT_ARRAY_BUFFER
            _ => 0,
        };
        if buf_id == 0 {
            return qjs::JSValue::undefined();
        }
        if let Some((ptr, len)) = data_opt {
            let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
            st.buffers.insert(buf_id, bytes.to_vec());
        } else {
            let sz = (numeric_size as i64).max(0) as usize;
            st.buffers.insert(buf_id, vec![0u8; sz]);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_buffer_sub_data(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);

        let mut target_f: f64 = 0.0;
        let mut offset_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut target_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut offset_f as *mut f64, args[1]);
        let target = target_f as i32;
        let offset = (offset_f as i64).max(0) as usize;

        let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[2]) else {
            return qjs::JSValue::undefined();
        };
        let src = unsafe { core::slice::from_raw_parts(ptr, len) };

        let mut st = WEBGL_STATE.lock();
        let buf_id = match target as u32 {
            0x8892 => st.array_buffer,         // ARRAY_BUFFER
            0x8893 => st.element_array_buffer, // ELEMENT_ARRAY_BUFFER
            _ => 0,
        };
        if buf_id == 0 {
            return qjs::JSValue::undefined();
        }
        let dst = st.buffers.entry(buf_id).or_insert_with(Vec::new);
        let needed = offset.saturating_add(src.len());
        if needed > dst.len() {
            dst.resize(needed, 0);
        }
        dst[offset..offset + src.len()].copy_from_slice(src);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_enable_vertex_attrib_array(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut idx_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut idx_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let idx = (idx_f as i32).max(0) as u32;
        let mut st = WEBGL_STATE.lock();
        let entry = st.attribs.entry(idx).or_default();
        entry.enabled = true;
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_vertex_attrib_pointer(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 6 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);

        let mut idx_f: f64 = 0.0;
        let mut size_f: f64 = 0.0;
        let mut ty_f: f64 = 0.0;
        let mut stride_f: f64 = 0.0;
        let mut offset_f: f64 = 0.0;
        let mut normalized_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut idx_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut size_f as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut ty_f as *mut f64, args[2]);
        let _ = qjs::JS_ToFloat64(ctx, &mut normalized_f as *mut f64, args[3]);
        let normalized = normalized_f != 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut stride_f as *mut f64, args[4]);
        let _ = qjs::JS_ToFloat64(ctx, &mut offset_f as *mut f64, args[5]);

        let idx = (idx_f as i32).max(0) as u32;
        let size = (size_f as i32).max(0);
        let ty = (ty_f as i32).max(0) as u32;
        let stride = (stride_f as i32).max(0);
        let offset = (offset_f as i64).max(0) as usize;

        let mut st = WEBGL_STATE.lock();
        let array_buffer = st.array_buffer;
        let entry = st.attribs.entry(idx).or_default();
        entry.size = size;
        entry.ty = ty;
        entry.normalized = normalized;
        entry.stride = stride;
        entry.offset = offset;
        entry.buffer = array_buffer;
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_viewport(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 4 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut w_f: f64 = 0.0;
        let mut h_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut w_f as *mut f64, args[2]);
        let _ = qjs::JS_ToFloat64(ctx, &mut h_f as *mut f64, args[3]);
        let w = (w_f as i32).max(0);
        let h = (h_f as i32).max(0);
        let mut st = WEBGL_STATE.lock();
        st.viewport_w = w;
        st.viewport_h = h;
        qjs::JSValue::undefined()
    }

    fn ty_size_bytes(ty: u32) -> Option<usize> {
        match ty {
            0x1406 => Some(4), // FLOAT
            0x1401 => Some(1), // UNSIGNED_BYTE
            0x1403 => Some(2), // UNSIGNED_SHORT
            _ => None,
        }
    }

    fn read_u16_le(bytes: &[u8], off: usize) -> Option<u16> {
        let b0 = *bytes.get(off)?;
        let b1 = *bytes.get(off + 1)?;
        Some(u16::from_le_bytes([b0, b1]))
    }

    fn read_f32_le(bytes: &[u8], off: usize) -> Option<f32> {
        let b0 = *bytes.get(off)?;
        let b1 = *bytes.get(off + 1)?;
        let b2 = *bytes.get(off + 2)?;
        let b3 = *bytes.get(off + 3)?;
        Some(f32::from_le_bytes([b0, b1, b2, b3]))
    }

    unsafe extern "C" fn gl_draw_elements(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 4 {
            webgl_log_draw_drop("drawElements", "bad-args");
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut mode_f: f64 = 0.0;
        let mut count_f: f64 = 0.0;
        let mut ty_f: f64 = 0.0;
        let mut offset_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut count_f as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut ty_f as *mut f64, args[2]);
        let _ = qjs::JS_ToFloat64(ctx, &mut offset_f as *mut f64, args[3]);
        let mode = mode_f as i32;
        if mode != 0x0004 {
            webgl_log_draw_drop("drawElements", "mode!=TRIANGLES");
            return qjs::JSValue::undefined();
        }
        let mut count = (count_f as i32).max(0) as usize;
        count -= count % 3;
        if count == 0 {
            webgl_log_draw_drop("drawElements", "count==0");
            return qjs::JSValue::undefined();
        }
        let ty = (ty_f as i32).max(0) as u32;
        if ty != 0x1403 {
            // UNSIGNED_SHORT only for now
            webgl_log_draw_drop("drawElements", "index-type!=UNSIGNED_SHORT");
            return qjs::JSValue::undefined();
        }
        let index_off = (offset_f as i64).max(0) as usize;

        const VTX_SIZE: usize = 12;

        // Snapshot everything we need while holding the lock.
        let (
            clear_rgb,
            viewport_w,
            viewport_h,
            buffers,
            elem_bytes,
            attribs,
            has_translation_matrix,
            has_projection_matrix,
            translation_matrix,
            projection_matrix,
        ) = {
            let st = WEBGL_STATE.lock();
            let Some(elem_bytes) = st.buffers.get(&st.element_array_buffer) else {
                webgl_log_draw_drop("drawElements", "no-element-array-buffer");
                return qjs::JSValue::undefined();
            };
            (
                st.clear_rgb,
                st.viewport_w,
                st.viewport_h,
                st.buffers.clone(),
                elem_bytes.clone(),
                st.attribs.clone(),
                st.has_translation_matrix,
                st.has_projection_matrix,
                st.translation_matrix,
                st.projection_matrix,
            )
        };

        let viewport_w = (viewport_w.max(1)) as f32;
        let viewport_h = (viewport_h.max(1)) as f32;

        // Heuristic: pick first enabled vec2 float attrib as position.
        let mut pos_attr: Option<(u32, WebGlVertexAttrib)> = None;
        let mut col_attr: Option<(u32, WebGlVertexAttrib)> = None;
        for (idx, a) in attribs.iter() {
            if !a.enabled {
                continue;
            }
            if pos_attr.is_none() && a.size == 2 && a.ty == 0x1406 {
                pos_attr = Some((*idx, *a));
            }
            if col_attr.is_none() && a.size == 4 && a.ty == 0x1401 {
                col_attr = Some((*idx, *a));
            }
        }
        let Some((_pos_idx, pos)) = pos_attr else {
            webgl_log_draw_drop("drawElements", "no-pos-attrib");
            return qjs::JSValue::undefined();
        };

        let pos_ty_sz = match ty_size_bytes(pos.ty) {
            Some(v) => v,
            None => return qjs::JSValue::undefined(),
        };
        let pos_stride = if pos.stride == 0 {
            (pos.size as usize).saturating_mul(pos_ty_sz)
        } else {
            pos.stride as usize
        };
        if pos_stride == 0 {
            webgl_log_draw_drop("drawElements", "pos-stride==0");
            return qjs::JSValue::undefined();
        }

        let (col, col_stride) = if let Some((_col_idx, col)) = col_attr {
            let col_ty_sz = match ty_size_bytes(col.ty) {
                Some(v) => v,
                None => 0,
            };
            let stride = if col.stride == 0 {
                (col.size as usize).saturating_mul(col_ty_sz)
            } else {
                col.stride as usize
            };
            (Some(col), stride)
        } else {
            (None, 0)
        };

        let Some(pos_bytes) = buffers.get(&pos.buffer) else {
            webgl_log_draw_drop("drawElements", "pos-buffer-missing");
            return qjs::JSValue::undefined();
        };
        let col_bytes_opt = col.and_then(|c| buffers.get(&c.buffer));

        let mut out = Vec::with_capacity(count.saturating_mul(VTX_SIZE));
        for i in 0..count {
            let idx_off = index_off.saturating_add(i.saturating_mul(2));
            let Some(vtx_idx) = read_u16_le(&elem_bytes, idx_off) else {
                break;
            };
            let vtx_idx = vtx_idx as usize;

            let base = vtx_idx.saturating_mul(pos_stride).saturating_add(pos.offset);
            let Some(x_px) = read_f32_le(pos_bytes, base) else {
                continue;
            };
            let Some(y_px) = read_f32_le(pos_bytes, base.saturating_add(4)) else {
                continue;
            };

            // If Pixi fed us the transform uniforms, emulate:
            //   gl_Position = projectionMatrix * translationMatrix * vec3(aVertexPosition, 1.0)
            // Otherwise keep legacy viewport mapping so older content still works.
            let (x, y) = if has_translation_matrix && has_projection_matrix {
                let (tx, ty, tz) = mat3_mul_vec3(&translation_matrix, x_px, y_px, 1.0);
                let (cx, cy, _cz) = mat3_mul_vec3(&projection_matrix, tx, ty, tz);
                (cx, cy)
            } else {
                let x = (2.0 * (x_px / viewport_w)) - 1.0;
                let y = 1.0 - (2.0 * (y_px / viewport_h));
                (x, y)
            };

            let (r, g, b) = if let (Some(col), Some(col_bytes)) = (col, col_bytes_opt) {
                let base = vtx_idx
                    .saturating_mul(col_stride)
                    .saturating_add(col.offset);
                let r = *col_bytes.get(base).unwrap_or(&255);
                let g = *col_bytes.get(base + 1).unwrap_or(&255);
                let b = *col_bytes.get(base + 2).unwrap_or(&255);
                (r, g, b)
            } else {
                (255, 255, 255)
            };

            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
            out.push(r);
            out.push(g);
            out.push(b);
            out.push(0);
        }

        if out.is_empty() {
            webgl_log_draw_drop("drawElements", "out-empty");
            return qjs::JSValue::undefined();
        }

        if WEBGL_LOG_DRAW_MODE.fetch_add(1, Ordering::Relaxed) < 12 {
            log_str("qjs-webgl: drawElements matrix_path=");
            log_usize_dec((has_translation_matrix && has_projection_matrix) as usize);
            log_str(" count=");
            log_usize_dec(count);
            log_str("\n");
        }

        if WEBGL_DID_LOG_DRAW
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            log_str("qjs-webgl: drawElements -> gfx vtx_bytes=");
            log_usize_dec(out.len());
            log_str("\n");
        }
        unsafe {
            let _ = trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, out.as_ptr(), out.len());
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_get_supported_extensions(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // Return an empty list rather than `null`.
        qjs::JS_NewArray(ctx)
    }

    unsafe extern "C" fn gl_get_shader_precision_format(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // Best-effort, plausible defaults.
        let obj = qjs::JS_NewObject(ctx);
        if obj.is_exception() {
            return obj;
        }
        let k_range_min = b"rangeMin\0";
        let k_range_max = b"rangeMax\0";
        let k_prec = b"precision\0";
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            k_range_min.as_ptr() as *const c_char,
            js_int32(127),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            k_range_max.as_ptr() as *const c_char,
            js_int32(127),
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            k_prec.as_ptr() as *const c_char,
            js_int32(23),
        );
        obj
    }

    unsafe extern "C" fn gl_clear_color(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut rf: f64 = 0.0;
        let mut gf: f64 = 0.0;
        let mut bf: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut rf as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut gf as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut bf as *mut f64, args[2]);
        let clamp = |v: f64| -> u8 {
            let x = if v.is_nan() { 0.0 } else { v };
            let x = x.max(0.0).min(1.0);
            (x * 255.0 + 0.5) as u8
        };
        let r = clamp(rf);
        let g = clamp(gf);
        let b = clamp(bf);
        let rgb = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        WEBGL_STATE.lock().clear_rgb = rgb;
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_draw_arrays(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 3 {
            webgl_log_draw_drop("drawArrays", "bad-args");
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut mode_f: f64 = 0.0;
        let mut first_f: f64 = 0.0;
        let mut count_f: f64 = 0.0;
        let _ = qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[0]);
        let _ = qjs::JS_ToFloat64(ctx, &mut first_f as *mut f64, args[1]);
        let _ = qjs::JS_ToFloat64(ctx, &mut count_f as *mut f64, args[2]);
        let mode = mode_f as i32;
        // TRIANGLES only.
        if mode != 0x0004 {
            webgl_log_draw_drop("drawArrays", "mode!=TRIANGLES");
            return qjs::JSValue::undefined();
        }
        let first = (first_f as i32).max(0) as usize;
        let mut count = (count_f as i32).max(0) as usize;
        count -= count % 3;
        if count == 0 {
            webgl_log_draw_drop("drawArrays", "count==0");
            return qjs::JSValue::undefined();
        }

        const VTX_SIZE: usize = 12;

        // Snapshot everything we need while holding the lock.
        let (
            clear_rgb,
            viewport_w,
            viewport_h,
            buffers,
            attribs,
            has_translation_matrix,
            has_projection_matrix,
            translation_matrix,
            projection_matrix,
        ) = {
            let st = WEBGL_STATE.lock();
            (
                st.clear_rgb,
                st.viewport_w,
                st.viewport_h,
                st.buffers.clone(),
                st.attribs.clone(),
                st.has_translation_matrix,
                st.has_projection_matrix,
                st.translation_matrix,
                st.projection_matrix,
            )
        };

        let viewport_w = (viewport_w.max(1)) as f32;
        let viewport_h = (viewport_h.max(1)) as f32;

        // Heuristic: pick first enabled vec2 float attrib as position.
        let mut pos_attr: Option<(u32, WebGlVertexAttrib)> = None;
        let mut col_attr: Option<(u32, WebGlVertexAttrib)> = None;
        for (idx, a) in attribs.iter() {
            if !a.enabled {
                continue;
            }
            if pos_attr.is_none() && a.size == 2 && a.ty == 0x1406 {
                pos_attr = Some((*idx, *a));
            }
            if col_attr.is_none() && a.size == 4 && a.ty == 0x1401 {
                col_attr = Some((*idx, *a));
            }
        }
        let Some((_pos_idx, pos)) = pos_attr else {
            webgl_log_draw_drop("drawArrays", "no-pos-attrib");
            return qjs::JSValue::undefined();
        };

        let pos_ty_sz = match ty_size_bytes(pos.ty) {
            Some(v) => v,
            None => {
                webgl_log_draw_drop("drawArrays", "bad-pos-type");
                return qjs::JSValue::undefined();
            }
        };
        let pos_stride = if pos.stride == 0 {
            (pos.size as usize).saturating_mul(pos_ty_sz)
        } else {
            pos.stride as usize
        };
        if pos_stride == 0 {
            webgl_log_draw_drop("drawArrays", "pos-stride==0");
            return qjs::JSValue::undefined();
        }

        let (col, col_stride) = if let Some((_col_idx, col)) = col_attr {
            let col_ty_sz = match ty_size_bytes(col.ty) {
                Some(v) => v,
                None => 0,
            };
            let stride = if col.stride == 0 {
                (col.size as usize).saturating_mul(col_ty_sz)
            } else {
                col.stride as usize
            };
            (Some(col), stride)
        } else {
            (None, 0)
        };

        let Some(pos_bytes) = buffers.get(&pos.buffer) else {
            webgl_log_draw_drop("drawArrays", "pos-buffer-missing");
            return qjs::JSValue::undefined();
        };
        let col_bytes_opt = col.and_then(|c| buffers.get(&c.buffer));

        let mut out = Vec::with_capacity(count.saturating_mul(VTX_SIZE));
        for i in 0..count {
            let vtx_idx = first.saturating_add(i);

            let base = vtx_idx.saturating_mul(pos_stride).saturating_add(pos.offset);
            let Some(x_px) = read_f32_le(pos_bytes, base) else {
                continue;
            };
            let Some(y_px) = read_f32_le(pos_bytes, base.saturating_add(4)) else {
                continue;
            };

            let (x, y) = if has_translation_matrix && has_projection_matrix {
                let (tx, ty, tz) = mat3_mul_vec3(&translation_matrix, x_px, y_px, 1.0);
                let (cx, cy, _cz) = mat3_mul_vec3(&projection_matrix, tx, ty, tz);
                (cx, cy)
            } else {
                let x = (2.0 * (x_px / viewport_w)) - 1.0;
                let y = 1.0 - (2.0 * (y_px / viewport_h));
                (x, y)
            };

            let (r, g, b) = if let (Some(col), Some(col_bytes)) = (col, col_bytes_opt) {
                let base = vtx_idx
                    .saturating_mul(col_stride)
                    .saturating_add(col.offset);
                let r = *col_bytes.get(base).unwrap_or(&255);
                let g = *col_bytes.get(base + 1).unwrap_or(&255);
                let b = *col_bytes.get(base + 2).unwrap_or(&255);
                (r, g, b)
            } else {
                (255, 255, 255)
            };

            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
            out.push(r);
            out.push(g);
            out.push(b);
            out.push(0);
        }

        if out.is_empty() {
            webgl_log_draw_drop("drawArrays", "out-empty");
            return qjs::JSValue::undefined();
        }

        if WEBGL_LOG_DRAW_MODE.fetch_add(1, Ordering::Relaxed) < 12 {
            log_str("qjs-webgl: drawArrays matrix_path=");
            log_usize_dec((has_translation_matrix && has_projection_matrix) as usize);
            log_str(" count=");
            log_usize_dec(count);
            log_str("\n");
        }

        unsafe {
            if WEBGL_DID_LOG_DRAW
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                log_str("qjs-webgl: drawArrays -> gfx vtx_bytes=");
                log_usize_dec(out.len());
                log_str("\n");
            }
            let _ = trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, out.as_ptr(), out.len());
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn gl_get_parameter(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // Return safe defaults for common queries.
        if argv.is_null() || argc < 1 {
            return qjs::JSValue::undefined();
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut pname_f: f64 = 0.0;
        if qjs::JS_ToFloat64(ctx, &mut pname_f as *mut f64, args[0]) != 0 {
            return qjs::JSValue::undefined();
        }
        let pname = pname_f as i32;
        match pname as u32 {
            // MAX_TEXTURE_SIZE
            0x0D33 => js_int32(4096),
            // MAX_TEXTURE_IMAGE_UNITS
            0x8872 => js_int32(8),
            // MAX_VERTEX_ATTRIBS
            0x8869 => js_int32(8),
            // VERSION
            0x1F02 => {
                let s = b"WebGL 1.0 (TRUEOS shim)\0";
                qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len() - 1)
            }
            // VENDOR
            0x1F00 => {
                let s = b"TRUEOS\0";
                qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len() - 1)
            }
            // RENDERER
            0x1F01 => {
                let s = b"software\0";
                qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len() - 1)
            }
            _ => js_null(),
        }
    }

    unsafe extern "C" fn gl_get_error(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        // NO_ERROR
        js_int32(0)
    }

    // Build the gl object.
    let gl = qjs::JS_NewObject(ctx);
    if gl.is_exception() {
        return gl;
    }

    // A small set of WebGL constants Pixi-like stacks commonly touch.
    macro_rules! gl_const {
        ($name:literal, $val:expr) => {{
            let k = concat!($name, "\0");
            let _ = qjs::JS_SetPropertyStr(ctx, gl, k.as_ptr() as *const c_char, js_int32($val));
        }};
    }

    gl_const!("NO_ERROR", 0);
    gl_const!("INVALID_ENUM", 0x0500);
    gl_const!("INVALID_VALUE", 0x0501);
    gl_const!("INVALID_OPERATION", 0x0502);
    gl_const!("OUT_OF_MEMORY", 0x0505);

    gl_const!("ARRAY_BUFFER", 0x8892);
    gl_const!("ELEMENT_ARRAY_BUFFER", 0x8893);
    gl_const!("STATIC_DRAW", 0x88E4);
    gl_const!("DYNAMIC_DRAW", 0x88E8);

    gl_const!("TEXTURE_2D", 0x0DE1);
    gl_const!("TEXTURE0", 0x84C0);
    gl_const!("RGBA", 0x1908);
    gl_const!("RGB", 0x1907);
    gl_const!("UNSIGNED_BYTE", 0x1401);
    gl_const!("UNSIGNED_SHORT", 0x1403);
    gl_const!("FLOAT", 0x1406);

    gl_const!("VERTEX_SHADER", 0x8B31);
    gl_const!("FRAGMENT_SHADER", 0x8B30);
    gl_const!("COMPILE_STATUS", 0x8B81);
    gl_const!("LINK_STATUS", 0x8B82);

    gl_const!("TRIANGLES", 0x0004);
    gl_const!("BLEND", 0x0BE2);
    gl_const!("SCISSOR_TEST", 0x0C11);
    gl_const!("CULL_FACE", 0x0B44);

    gl_const!("COLOR_BUFFER_BIT", 0x4000);

    gl_const!("ONE", 1);
    gl_const!("ONE_MINUS_SRC_ALPHA", 0x0303);
    gl_const!("SRC_ALPHA", 0x0302);

    gl_const!("MAX_TEXTURE_SIZE", 0x0D33);
    gl_const!("MAX_TEXTURE_IMAGE_UNITS", 0x8872);
    gl_const!("MAX_VERTEX_ATTRIBS", 0x8869);
    gl_const!("VERSION", 0x1F02);
    gl_const!("VENDOR", 0x1F00);
    gl_const!("RENDERER", 0x1F01);

    // Methods: mostly no-op, but creation returns handles and getParameter/getError return useful values.
    macro_rules! gl_fn {
        ($name:literal, $func:expr, $argc:expr) => {{
            let k = concat!($name, "\0");
            let f = qjs::JS_NewCFunction2(
                ctx,
                Some($func),
                k.as_ptr() as *const c_char,
                $argc,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ = qjs::JS_SetPropertyStr(ctx, gl, k.as_ptr() as *const c_char, f);
        }};
    }

    gl_fn!("getError", gl_get_error, 0);
    gl_fn!("getParameter", gl_get_parameter, 1);
    gl_fn!("getExtension", gl_return_null, 1);
    gl_fn!("getSupportedExtensions", gl_get_supported_extensions, 0);
    gl_fn!("getShaderPrecisionFormat", gl_get_shader_precision_format, 2);
    gl_fn!("isContextLost", gl_return_false, 0);

    // Object creation helpers
    gl_fn!("createBuffer", gl_create_handle, 0);
    gl_fn!("createTexture", gl_create_handle, 0);
    gl_fn!("createShader", gl_create_handle, 1);
    gl_fn!("createProgram", gl_create_handle, 0);
    gl_fn!("getUniformLocation", gl_get_uniform_location, 2);

    // Generic no-ops (extend as Pixi tells us what it needs)
    gl_fn!("bindBuffer", gl_bind_buffer, 2);
    gl_fn!("bufferData", gl_buffer_data, 3);
    gl_fn!("bufferSubData", gl_buffer_sub_data, 3);
    gl_fn!("bindTexture", gl_noop, 2);
    gl_fn!("activeTexture", gl_noop, 1);
    gl_fn!("texParameteri", gl_noop, 3);
    gl_fn!("texImage2D", gl_noop, 9);
    gl_fn!("texSubImage2D", gl_noop, 9);
    gl_fn!("pixelStorei", gl_noop, 2);
    gl_fn!("shaderSource", gl_noop, 2);
    gl_fn!("compileShader", gl_noop, 1);
    gl_fn!("attachShader", gl_noop, 2);
    gl_fn!("linkProgram", gl_noop, 1);
    gl_fn!("useProgram", gl_noop, 1);
    gl_fn!("enableVertexAttribArray", gl_enable_vertex_attrib_array, 1);
    gl_fn!("vertexAttribPointer", gl_vertex_attrib_pointer, 6);
    gl_fn!("uniform1i", gl_noop, 2);
    gl_fn!("uniform1f", gl_noop, 2);
    gl_fn!("uniform2f", gl_noop, 3);
    gl_fn!("uniform4f", gl_noop, 5);
    gl_fn!("uniformMatrix3fv", gl_uniform_matrix3fv, 3);
    gl_fn!("uniformMatrix4fv", gl_noop, 3);
    gl_fn!("viewport", gl_viewport, 4);
    gl_fn!("scissor", gl_noop, 4);
    gl_fn!("enable", gl_noop, 1);
    gl_fn!("disable", gl_noop, 1);
    gl_fn!("blendFunc", gl_noop, 2);
    gl_fn!("blendFuncSeparate", gl_noop, 4);
    gl_fn!("clearColor", gl_clear_color, 4);
    gl_fn!("clear", gl_noop, 1);
    gl_fn!("drawElements", gl_draw_elements, 4);
    gl_fn!("drawArrays", gl_draw_arrays, 3);
    gl_fn!("flush", gl_noop, 0);

    // Minimal success-y queries
    gl_fn!("getShaderParameter", gl_return_true, 2);
    gl_fn!("getProgramParameter", gl_return_true, 2);
    gl_fn!("getAttribLocation", gl_create_handle, 2);
    gl_fn!("getShaderInfoLog", gl_return_null, 1);
    gl_fn!("getProgramInfoLog", gl_return_null, 1);

    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, gl);
    // Return a borrowed handle from global
    qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char)
}

unsafe fn ensure_global_document(
    ctx: *mut qjs::JSContext,
    global: qjs::JSValue,
    gl_obj: qjs::JSValue,
) -> qjs::JSValue {
    let key = b"document\0";
    let existing = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    if !existing.is_exception() && existing.tag != qjs::JS_TAG_UNDEFINED {
        return existing;
    }
    qjs::js_free_value(ctx, existing);

    unsafe extern "C" fn doc_create_element(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return qjs::JS_NewObject(ctx);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let cstr = qjs::js_to_cstring(ctx, args[0]);
        if cstr.is_null() {
            return qjs::JS_NewObject(ctx);
        }
        let tag = CStr::from_ptr(cstr).to_bytes();
        qjs::JS_FreeCString(ctx, cstr);

        // We only special-case canvas for now.
        if tag.eq_ignore_ascii_case(b"canvas") {
            // Canvas object with getContext().
            unsafe extern "C" fn canvas_get_context(
                ctx: *mut qjs::JSContext,
                this_val: qjs::JSValueConst,
                argc: c_int,
                argv: *const qjs::JSValueConst,
            ) -> qjs::JSValue {
                // If the caller asked for "2d", return null for now (explicitly not supported).
                if !argv.is_null() && argc >= 1 {
                    let args = core::slice::from_raw_parts(argv, argc as usize);
                    let cstr = qjs::js_to_cstring(ctx, args[0]);
                    if !cstr.is_null() {
                        let kind = CStr::from_ptr(cstr).to_bytes();
                        qjs::JS_FreeCString(ctx, cstr);
                        if WEBGL_LOG_GET_CONTEXT.fetch_add(1, Ordering::Relaxed) < 12 {
                            log_str("qjs-webgl: canvas.getContext kind=");
                            log_bytes(kind);
                            log_str(" argc=");
                            log_usize_dec(argc.max(0) as usize);
                            log_str("\n");
                        }
                        if kind.eq_ignore_ascii_case(b"2d") {
                            return js_null();
                        }

                        // We currently only model a very small WebGL 1-ish subset.
                        // Returning a non-null object for "webgl2" causes libraries like Pixi
                        // to take WebGL2 code paths (VAOs, UBOs, etc.) that our shim does not
                        // implement, often resulting in a blank scene.
                        if kind.eq_ignore_ascii_case(b"webgl2") {
                            return js_null();
                        }

                        // Pixi (and friends) do feature probes with a one-arg
                        // getContext("webgl") call and then await media events.
                        // We don't model that event loop, so treat one-arg webgl/webgl2
                        // as "unsupported" to avoid stalling module initialization.
                        if (kind.eq_ignore_ascii_case(b"webgl")
                            || kind.eq_ignore_ascii_case(b"webgl2"))
                            && argc < 2
                        {
                            // Escape hatch for explicit smokes/tests.
                            let global = qjs::JS_GetGlobalObject(ctx);
                            let force = qjs::JS_GetPropertyStr(
                                ctx,
                                global,
                                b"__trueos_webgl_force\0".as_ptr() as *const c_char,
                            );
                            qjs::js_free_value(ctx, global);
                            let mut f: f64 = 0.0;
                            let forced = (!force.is_exception())
                                && (qjs::JS_ToFloat64(ctx, &mut f as *mut f64, force) == 0)
                                && (f != 0.0);
                            qjs::js_free_value(ctx, force);
                            if forced {
                                // allow
                            } else {
                            return js_null();
                            }
                        }
                    }
                }

                // For actual renderer creation paths (usually passing options),
                // return the shared singleton.
                let global = qjs::JS_GetGlobalObject(ctx);
                let gl = qjs::JS_GetPropertyStr(
                    ctx,
                    global,
                    b"__trueos_gl\0".as_ptr() as *const c_char,
                );
                qjs::js_free_value(ctx, global);
                if gl.is_exception() {
                    return js_null();
                }

                // Store last_context on the canvas for debugging.
                let _ = qjs::JS_SetPropertyStr(
                    ctx,
                    this_val,
                    b"__trueos_last_context\0".as_ptr() as *const c_char,
                    qjs::js_dup_value(ctx, gl),
                );

                gl
            }

            let canvas = qjs::JS_NewObject(ctx);
            if canvas.is_exception() {
                return canvas;
            }

            // width/height default
            let _ = qjs::JS_SetPropertyStr(ctx, canvas, b"width\0".as_ptr() as *const c_char, js_int32(1));
            let _ = qjs::JS_SetPropertyStr(ctx, canvas, b"height\0".as_ptr() as *const c_char, js_int32(1));

            let name = b"getContext\0";
            let f = qjs::JS_NewCFunction2(
                ctx,
                Some(canvas_get_context),
                name.as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ = qjs::JS_SetPropertyStr(ctx, canvas, name.as_ptr() as *const c_char, f);

            return canvas;
        }

        qjs::JS_NewObject(ctx)
    }

    // Create document object
    let doc = qjs::JS_NewObject(ctx);
    if doc.is_exception() {
        return doc;
    }

    // document.body placeholder
    let body = qjs::JS_NewObject(ctx);
    if !body.is_exception() {
        let _ = qjs::JS_SetPropertyStr(ctx, doc, b"body\0".as_ptr() as *const c_char, body);
    } else {
        qjs::js_free_value(ctx, body);
    }

    // document.createElement
    let name = b"createElement\0";
    let f = qjs::JS_NewCFunction2(
        ctx,
        Some(doc_create_element),
        name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, doc, name.as_ptr() as *const c_char, f);

    // Also expose the gl object in case libraries probe for it.
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        doc,
        b"__trueos_gl\0".as_ptr() as *const c_char,
        qjs::js_dup_value(ctx, gl_obj),
    );

    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, doc);
    qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char)
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

unsafe extern "C" fn qjs_fs_read_file_text_async(
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

    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);
    let path = core::slice::from_raw_parts(path_ptr, path_len);
    let op_id = qjs::async_ops::start_read_file(path);

    js_free_cstring(ctx, path_cstr);

    match op_id {
        Ok(id) => {
            qjs::async_ops::register_promise(
                ctx,
                id,
                qjs::async_ops::OpKind::ReadText,
                resolve,
                reject,
            );
        }
        Err(code) => {
            let arg = js_int32(code);
            let _ = qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &arg as *const qjs::JSValue);
        }
    }

    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn qjs_fs_read_file_bytes_async(
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

    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);
    let path = core::slice::from_raw_parts(path_ptr, path_len);
    let op_id = qjs::async_ops::start_read_file(path);

    js_free_cstring(ctx, path_cstr);

    match op_id {
        Ok(id) => {
            qjs::async_ops::register_promise(
                ctx,
                id,
                qjs::async_ops::OpKind::ReadBytes,
                resolve,
                reject,
            );
        }
        Err(code) => {
            let arg = js_int32(code);
            let _ = qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &arg as *const qjs::JSValue);
        }
    }

    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn qjs_fs_write_file_text_async(
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
        js_free_cstring(ctx, path_cstr);
        return qjs::JSValue::exception();
    };

    let (promise, resolve, reject) = qjs::async_ops::new_promise(ctx);

    let path = core::slice::from_raw_parts(path_ptr, path_len);
    let data = core::slice::from_raw_parts(data_ptr, data_len);
    let op_id = qjs::async_ops::start_write_file(path, data);

    js_free_cstring(ctx, path_cstr);
    js_free_cstring(ctx, data_cstr);

    match op_id {
        Ok(id) => {
            qjs::async_ops::register_promise(
                ctx,
                id,
                qjs::async_ops::OpKind::WriteText,
                resolve,
                reject,
            );
        }
        Err(code) => {
            let arg = js_int32(code);
            let _ = qjs::JS_Call(ctx, reject, qjs::JSValue::undefined(), 1, &arg as *const qjs::JSValue);
        }
    }

    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn qjs_fs_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> c_int {
    let read_text_async_name = b"readFileAsync\0";
    let read_bytes_async_name = b"readFileBytesAsync\0";
    let write_text_async_name = b"writeFileAsync\0";

    let read_text_async_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_read_file_text_async),
        read_text_async_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, read_text_async_name.as_ptr() as *const c_char, read_text_async_fn) < 0 {
        return -1;
    }

    let read_bytes_async_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_read_file_bytes_async),
        read_bytes_async_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, read_bytes_async_name.as_ptr() as *const c_char, read_bytes_async_fn) < 0 {
        return -1;
    }

    let write_text_async_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_fs_write_file_text_async),
        write_text_async_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, write_text_async_name.as_ptr() as *const c_char, write_text_async_fn) < 0 {
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
    } else if name == b"worker_threads" || name == b"node:worker_threads" {
        (
            qjs_worker_threads_module_init,
            &[
                b"Worker\0",
                b"isMainThread\0",
                b"parentPort\0",
                b"threadId\0",
                b"workerData\0",
            ],
        )
    } else if name == b"fs" {
        (
            qjs_fs_module_init,
            &[
                b"readFileAsync\0",
                b"readFileBytesAsync\0",
                b"writeFileAsync\0",
            ],
        )
    } else if name == b"process" || name == b"node:process" {
        (
            qjs_process_module_init,
            &[
                b"default\0",
                b"env\0",
                b"argv\0",
                b"cwd\0",
                b"chdir\0",
                b"nextTick\0",
                b"hrtime\0",
                b"uptime\0",
                b"versions\0",
                b"platform\0",
                b"arch\0",
                b"release\0",
            ],
        )
    } else if name == b"path" || name == b"node:path" {
        (
            qjs_path_module_init,
            &[b"join\0", b"dirname\0", b"basename\0", b"extname\0", b"resolve\0"],
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

pub(crate) unsafe extern "C" fn trueos_module_loader(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
    _opaque: *mut core::ffi::c_void,
) -> *mut qjs::JSModuleDef {
    if !module_name.is_null() {
        let spec = CStr::from_ptr(module_name).to_bytes();
        trace_str("qjs: loader spec=");
        trace_bytes(spec);
        trace_nl();
    } else {
        trace_str("qjs: loader spec=<null>\n");
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
        trace_str("qjs: loader url\n");
        return load_url_module(ctx, module_name, spec);
    }

    trace_str("qjs: loader fs\n");

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

unsafe fn throw_error(ctx: *mut qjs::JSContext, msg: &[u8]) {
    let err = qjs::JS_NewError(ctx);
    if err.is_exception() {
        return;
    }
    let m = qjs::JS_NewStringLen(ctx, msg.as_ptr() as *const c_char, msg.len());
    let _ = qjs::JS_SetPropertyStr(ctx, err, b"message\0".as_ptr() as *const c_char, m);
    let _ = qjs::JS_Throw(ctx, err);
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

unsafe fn fetch_to_cache_rc_async(url: &[u8], cache_path: &[u8], timeout_ms: u64) -> Result<(), i32> {
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

    // `JS_Eval(..., COMPILE_ONLY|MODULE)` returns a `JSValue` (tagged as MODULE)
    // which must be released after extracting the module pointer, otherwise the
    // runtime will retain an extra reference and assert in `JS_FreeRuntime`.
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
        let normalized = normalize_path_bytes(spec);
        log_normalized(&normalized);
        return js_strdup(ctx, &normalized);
    }

    // Bare specifiers.
    if !path_is_relative(spec) {
        // Always keep known TRUEOS native modules.
        if spec == b"complex"
            || spec == b"fs"
            || spec == b"worker_threads"
            || spec == b"process"
            || spec == b"node:process"
            || spec == b"node:worker_threads"
            || spec == b"path"
            || spec == b"node:path"
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

    trace_str("qjs: url cache=");
    trace_bytes(&cache_path);
    trace_nl();

    // Fast-path: if the cached module is already present, avoid refetching.
    match read_file_js_malloc_rc(ctx, &cache_path) {
        Ok((buf, len)) => {
            trace_str("qjs: url cache hit len=");
            trace_usize_dec(len);
            trace_nl();
            trace_str("qjs: url compile start\n");
            let m = compile_module_from_buf(ctx, module_name, buf, len);
            trace_str("qjs: url compile done\n");
            qjs::js_free(ctx, buf as *mut core::ffi::c_void);
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
    let m = compile_module_from_buf(ctx, module_name, buf, len);
    trace_str("qjs: url compile done\n");
    qjs::js_free(ctx, buf as *mut core::ffi::c_void);
    m
}

unsafe fn load_fs_module(ctx: *mut qjs::JSContext, module_name: *const c_char) -> *mut qjs::JSModuleDef {
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
