extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use core::ffi::{c_char, c_int, CStr};
use spin::Mutex;

use crate as qjs;
use crate::cmd_stream;
use crate::webgl;

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_font_atlas_small(info: *mut FontAtlasInfo) -> bool;
    fn trueos_cabi_font_atlas_large(info: *mut FontAtlasInfo) -> bool;
}

static PROCESS_ARGV: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static PROCESS_CWD: Mutex<Vec<u8>> = Mutex::new(Vec::new());
#[derive(Copy, Clone)]
struct PackedJsValue {
    tag: i64,
    payload: i64,
}

#[repr(C)]
struct FontAtlasInfo {
    rgba_ptr: *const u8,
    rgba_len: usize,
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
    grid_w: u32,
    grid_h: u32,
    index_ptr: *const u16,
    index_len: usize,
    widths_ptr: *const u8,
    widths_len: usize,
}

#[inline]
fn pack_js_value(v: qjs::JSValue) -> PackedJsValue {
    PackedJsValue {
        tag: v.tag,
        payload: unsafe { v.u.short_big_int },
    }
}

#[inline]
fn unpack_js_value(v: PackedJsValue) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion {
            short_big_int: v.payload,
        },
        tag: v.tag,
    }
}

struct ProcessNextTickCall {
    ctx: usize,
    func: PackedJsValue,
    args: Vec<PackedJsValue>,
}
static PROCESS_NEXT_TICK_QUEUE: Mutex<Vec<ProcessNextTickCall>> = Mutex::new(Vec::new());

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

unsafe fn js_arg_to_utf8_bytes(
    ctx: *mut qjs::JSContext,
    val: qjs::JSValueConst,
) -> Option<(*const u8, usize, *const c_char)> {
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

fn u16_slice_to_le_bytes(src: &[u16]) -> Vec<u8> {
    let mut out = vec![0u8; src.len().saturating_mul(2)];
    for (i, v) in src.iter().copied().enumerate() {
        let off = i * 2;
        out[off] = (v & 0xFF) as u8;
        out[off + 1] = (v >> 8) as u8;
    }
    out
}

unsafe extern "C" fn qjs_text_get_atlas_small(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let mut info = FontAtlasInfo {
        rgba_ptr: core::ptr::null(),
        rgba_len: 0,
        width: 0,
        height: 0,
        cell_w: 0,
        cell_h: 0,
        grid_w: 0,
        grid_h: 0,
        index_ptr: core::ptr::null(),
        index_len: 0,
        widths_ptr: core::ptr::null(),
        widths_len: 0,
    };
    if !trueos_cabi_font_atlas_small(&mut info as *mut FontAtlasInfo) {
        return qjs::JSValue::undefined();
    }
    if info.rgba_ptr.is_null() || info.index_ptr.is_null() {
        return qjs::JSValue::undefined();
    }
    let rgba = core::slice::from_raw_parts(info.rgba_ptr, info.rgba_len);
    let index_slice = core::slice::from_raw_parts(info.index_ptr, info.index_len);
    let index_bytes = u16_slice_to_le_bytes(index_slice);

    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return obj;
    }

    let pixels = qjs::JS_NewArrayBufferCopy(ctx, rgba.as_ptr(), rgba.len());
    let index_ab = qjs::JS_NewArrayBufferCopy(ctx, index_bytes.as_ptr(), index_bytes.len());

    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"pixels\0".as_ptr() as *const c_char, pixels);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"width\0".as_ptr() as *const c_char,
        js_int32(info.width as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"height\0".as_ptr() as *const c_char,
        js_int32(info.height as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"cellW\0".as_ptr() as *const c_char,
        js_int32(info.cell_w as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"cellH\0".as_ptr() as *const c_char,
        js_int32(info.cell_h as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"gridW\0".as_ptr() as *const c_char,
        js_int32(info.grid_w as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"gridH\0".as_ptr() as *const c_char,
        js_int32(info.grid_h as i32),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"index\0".as_ptr() as *const c_char, index_ab);

    obj
}

unsafe extern "C" fn qjs_text_get_atlas_large(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let mut info = FontAtlasInfo {
        rgba_ptr: core::ptr::null(),
        rgba_len: 0,
        width: 0,
        height: 0,
        cell_w: 0,
        cell_h: 0,
        grid_w: 0,
        grid_h: 0,
        index_ptr: core::ptr::null(),
        index_len: 0,
        widths_ptr: core::ptr::null(),
        widths_len: 0,
    };
    if !trueos_cabi_font_atlas_large(&mut info as *mut FontAtlasInfo) {
        return qjs::JSValue::undefined();
    }
    if info.rgba_ptr.is_null() || info.index_ptr.is_null() {
        return qjs::JSValue::undefined();
    }
    let rgba = core::slice::from_raw_parts(info.rgba_ptr, info.rgba_len);
    let index_slice = core::slice::from_raw_parts(info.index_ptr, info.index_len);
    let index_bytes = u16_slice_to_le_bytes(index_slice);
    let widths = if info.widths_ptr.is_null() || info.widths_len == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(info.widths_ptr, info.widths_len)
    };

    let obj = qjs::JS_NewObject(ctx);
    if obj.is_exception() {
        return obj;
    }

    let pixels = qjs::JS_NewArrayBufferCopy(ctx, rgba.as_ptr(), rgba.len());
    let index_ab = qjs::JS_NewArrayBufferCopy(ctx, index_bytes.as_ptr(), index_bytes.len());
    let widths_ab = qjs::JS_NewArrayBufferCopy(ctx, widths.as_ptr(), widths.len());

    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"pixels\0".as_ptr() as *const c_char, pixels);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"width\0".as_ptr() as *const c_char,
        js_int32(info.width as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"height\0".as_ptr() as *const c_char,
        js_int32(info.height as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"cellW\0".as_ptr() as *const c_char,
        js_int32(info.cell_w as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"cellH\0".as_ptr() as *const c_char,
        js_int32(info.cell_h as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"gridW\0".as_ptr() as *const c_char,
        js_int32(info.grid_w as i32),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        b"gridH\0".as_ptr() as *const c_char,
        js_int32(info.grid_h as i32),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"index\0".as_ptr() as *const c_char, index_ab);
    let _ = qjs::JS_SetPropertyStr(ctx, obj, b"widths\0".as_ptr() as *const c_char, widths_ab);

    obj
}

unsafe extern "C" fn qjs_text_module_init(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
    let small_name = b"getFontAtlasSmall\0";
    let large_name = b"getFontAtlasLarge\0";

    let small_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_text_get_atlas_small),
        small_name.as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, small_name.as_ptr() as *const c_char, small_fn) < 0 {
        return -1;
    }

    let large_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_text_get_atlas_large),
        large_name.as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(ctx, m, large_name.as_ptr() as *const c_char, large_fn) < 0 {
        return -1;
    }

    0
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
    let mut copied_args: Vec<PackedJsValue> = Vec::new();
    if argc > 1 {
        copied_args.reserve((argc - 1) as usize);
        for i in 1..(argc as usize) {
            copied_args.push(pack_js_value(qjs::js_dup_value(ctx, args[i])));
        }
    }
    PROCESS_NEXT_TICK_QUEUE.lock().push(ProcessNextTickCall {
        ctx: ctx as usize,
        func: pack_js_value(qjs::js_dup_value(ctx, func)),
        args: copied_args,
    });
    qjs::JSValue::undefined()
}

pub(crate) fn has_process_next_tick_pending(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }
    let key = ctx as usize;
    PROCESS_NEXT_TICK_QUEUE.lock().iter().any(|c| c.ctx == key)
}

pub(crate) unsafe fn pump_process_next_tick(ctx: *mut qjs::JSContext) -> bool {
    if ctx.is_null() {
        return false;
    }
    let key = ctx as usize;
    let mut progress = false;
    // Bound per-pump work to keep fairness with async/net polling.
    for _ in 0..64 {
        let call = {
            let mut q = PROCESS_NEXT_TICK_QUEUE.lock();
            let Some(idx) = q.iter().position(|c| c.ctx == key) else {
                break;
            };
            q.remove(idx)
        };
        progress = true;
        let func = unpack_js_value(call.func);
        let args_len = call.args.len();
        let mut call_args: Vec<qjs::JSValue> = Vec::with_capacity(args_len);
        for arg in call.args {
            call_args.push(unpack_js_value(arg));
        }
        let argc = args_len as c_int;
        let argv = if call_args.is_empty() {
            core::ptr::null()
        } else {
            call_args.as_ptr() as *const qjs::JSValueConst
        };
        let ret = qjs::JS_Call(ctx, func, qjs::JSValue::undefined(), argc, argv);
        qjs::js_free_value(ctx, ret);
        qjs::js_free_value(ctx, func);
        for v in call_args {
            qjs::js_free_value(ctx, v);
        }
    }
    progress
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
        qjs::trueos_module_loader::normalize_path_bytes(input)
    } else {
        let mut joined = process_cwd_snapshot();
        if !joined.ends_with(b"/") {
            joined.push(b'/');
        }
        joined.extend_from_slice(input);
        qjs::trueos_module_loader::normalize_path_bytes(joined.as_slice())
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
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ as u64;
    if hz == 0 {
        return unsafe { qjs::JS_NewFloat64(ctx, 0.0) };
    }
    let secs = (ticks / hz) as f64;
    let rem = (ticks % hz) as f64;
    unsafe { qjs::JS_NewFloat64(ctx, secs + (rem / hz as f64)) }
}

unsafe extern "C" fn qjs_process_hrtime(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // Node-style: [seconds, nanoseconds], optionally diffed against prior [sec, nsec].
    let ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ as u64;
    let total_ns: u128 = if hz == 0 {
        0
    } else {
        (ticks as u128).saturating_mul(1_000_000_000u128) / (hz as u128)
    };
    let mut out_ns = total_ns;

    if !argv.is_null() && argc >= 1 {
        let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
        let prev = args[0];
        let s_v = unsafe { qjs::JS_GetPropertyUint32(ctx, prev, 0) };
        let ns_v = unsafe { qjs::JS_GetPropertyUint32(ctx, prev, 1) };
        if !s_v.is_exception() && !ns_v.is_exception() {
            let mut s_f: f64 = 0.0;
            let mut ns_f: f64 = 0.0;
            let s_ok = unsafe { qjs::JS_ToFloat64(ctx, &mut s_f as *mut f64, s_v) } == 0;
            let ns_ok = unsafe { qjs::JS_ToFloat64(ctx, &mut ns_f as *mut f64, ns_v) } == 0;
            if s_ok && ns_ok {
                let prev_ns = (s_f.max(0.0) as u128)
                    .saturating_mul(1_000_000_000u128)
                    .saturating_add(ns_f.max(0.0) as u128);
                out_ns = out_ns.saturating_sub(prev_ns);
            }
        }
        unsafe {
            qjs::js_free_value(ctx, s_v);
            qjs::js_free_value(ctx, ns_v);
        }
    }

    let out_s = (out_ns / 1_000_000_000u128).min(i32::MAX as u128) as i32;
    let out_n = (out_ns % 1_000_000_000u128) as i32;

    let arr = unsafe { qjs::JS_NewArray(ctx) };
    if arr.is_exception() {
        return arr;
    }
    let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, 0, js_int32(out_s)) };
    let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, 1, js_int32(out_n)) };
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
    let normalized = qjs::trueos_module_loader::normalize_path_bytes(acc.as_slice());
    let out = if normalized.is_empty() {
        b"/".as_slice()
    } else {
        normalized.as_slice()
    };
    qjs::JS_NewStringLen(ctx, out.as_ptr() as *const c_char, out.len())
}

unsafe extern "C" fn qjs_path_module_init(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
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
    if unsafe { qjs::JS_SetModuleExport(ctx, m, join_name.as_ptr() as *const c_char, join_fn) } < 0
    {
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
    if unsafe {
        qjs::JS_SetModuleExport(ctx, m, dirname_name.as_ptr() as *const c_char, dirname_fn)
    } < 0
    {
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
    if unsafe {
        qjs::JS_SetModuleExport(ctx, m, basename_name.as_ptr() as *const c_char, basename_fn)
    } < 0
    {
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
    if unsafe {
        qjs::JS_SetModuleExport(ctx, m, extname_name.as_ptr() as *const c_char, extname_fn)
    } < 0
    {
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
    if unsafe {
        qjs::JS_SetModuleExport(ctx, m, resolve_name.as_ptr() as *const c_char, resolve_fn)
    } < 0
    {
        return -1;
    }

    0
}

unsafe extern "C" fn qjs_worker_threads_module_init(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
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
        let _ =
            qjs::JS_SetPropertyStr(ctx, obj, b"execPath\0".as_ptr() as *const c_char, exec_path);
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
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            versions,
            b"modules\0".as_ptr() as *const c_char,
            modules_v,
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            versions,
            b"openssl\0".as_ptr() as *const c_char,
            openssl_v,
        );
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
        let _ =
            qjs::JS_SetPropertyStr(ctx, obj, b"nextTick\0".as_ptr() as *const c_char, next_tick);
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

unsafe extern "C" fn qjs_process_module_init(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
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
    qjs::qjs_diag::install_context(ctx);

    let Ok(proc) = ensure_global_process(ctx) else {
        return;
    };
    // Keep globalThis.process installed; drop our local handle.
    qjs::js_free_value(ctx, proc);

    // Minimal browser-ish globals for early library bring-up.
    // Includes a lean WebGL shim for Pixi Graphics-only scenarios.
    let _ = ensure_global_window_document(ctx);

    // Expose a browser-style global Worker constructor backed by the existing worker_threads shim.
    let global = qjs::JS_GetGlobalObject(ctx);
    if !global.is_exception() {
        let key = b"Worker\0";
        let existing = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
        let needs_install = existing.is_exception() || existing.tag == qjs::JS_TAG_UNDEFINED;
        qjs::js_free_value(ctx, existing);
        if needs_install {
            let worker_ctor = qjs::JS_NewCFunction2(
                ctx,
                Some(qjs::workers::js_worker_ctor),
                key.as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_CONSTRUCTOR,
                0,
            );
            if !worker_ctor.is_exception() {
                let proto = qjs::JS_NewObject(ctx);
                if !proto.is_exception() {
                    let pm = qjs::JS_NewCFunction2(
                        ctx,
                        Some(qjs::workers::js_worker_post_message),
                        b"postMessage\0".as_ptr() as *const c_char,
                        1,
                        qjs::JS_CFUNC_GENERIC,
                        0,
                    );
                    let term = qjs::JS_NewCFunction2(
                        ctx,
                        Some(qjs::workers::js_worker_terminate),
                        b"terminate\0".as_ptr() as *const c_char,
                        0,
                        qjs::JS_CFUNC_GENERIC,
                        0,
                    );
                    let onm = qjs::JS_NewCFunction2(
                        ctx,
                        Some(qjs::workers::js_worker_on_message),
                        b"onMessage\0".as_ptr() as *const c_char,
                        1,
                        qjs::JS_CFUNC_GENERIC,
                        0,
                    );
                    let add_evt = qjs::JS_NewCFunction2(
                        ctx,
                        Some(qjs::workers::js_worker_add_event_listener),
                        b"addEventListener\0".as_ptr() as *const c_char,
                        2,
                        qjs::JS_CFUNC_GENERIC,
                        0,
                    );
                    let rm_evt = qjs::JS_NewCFunction2(
                        ctx,
                        Some(qjs::workers::js_worker_remove_event_listener),
                        b"removeEventListener\0".as_ptr() as *const c_char,
                        2,
                        qjs::JS_CFUNC_GENERIC,
                        0,
                    );
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        proto,
                        b"postMessage\0".as_ptr() as *const c_char,
                        pm,
                    );
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        proto,
                        b"terminate\0".as_ptr() as *const c_char,
                        term,
                    );
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        proto,
                        b"onMessage\0".as_ptr() as *const c_char,
                        onm,
                    );
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        proto,
                        b"addEventListener\0".as_ptr() as *const c_char,
                        add_evt,
                    );
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        proto,
                        b"removeEventListener\0".as_ptr() as *const c_char,
                        rm_evt,
                    );
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        proto,
                        b"constructor\0".as_ptr() as *const c_char,
                        qjs::js_dup_value(ctx, worker_ctor),
                    );
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        worker_ctor,
                        b"prototype\0".as_ptr() as *const c_char,
                        proto,
                    );
                } else {
                    qjs::js_free_value(ctx, proto);
                }
            }
            let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, worker_ctor);
        }
        qjs::js_free_value(ctx, global);
    }
}

unsafe fn ensure_global_window_document(ctx: *mut qjs::JSContext) -> Result<(), ()> {
    if ctx.is_null() {
        return Err(());
    }

    // globalThis
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return Err(());
    }

    // Mouse API + event pump helpers.
    qjs::browser::install_mouse_api(ctx, global);

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

    // performance.now()
    {
        unsafe extern "C" fn perf_now(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: c_int,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            // Monotonic milliseconds since boot. Many JS libs (including Pixi)
            // assume `performance.now()` advances.
            let ticks = embassy_time_driver::now();
            let hz = embassy_time_driver::TICK_HZ as u64;
            let ms = if hz == 0 { 0 } else { (ticks.saturating_mul(1000)) / hz };
            qjs::JS_NewFloat64(ctx, ms as f64)
        }

        let perf_key = b"performance\0";
        let perf_existing = qjs::JS_GetPropertyStr(ctx, global, perf_key.as_ptr() as *const c_char);
        let needs_perf = perf_existing.is_exception()
            || perf_existing.tag == qjs::JS_TAG_UNDEFINED
            || perf_existing.tag == qjs::JS_TAG_NULL;
        qjs::js_free_value(ctx, perf_existing);
        if needs_perf {
            let perf = qjs::JS_NewObject(ctx);
            if !perf.is_exception() {
                let now_fn = qjs::JS_NewCFunction2(
                    ctx,
                    Some(perf_now),
                    b"now\0".as_ptr() as *const c_char,
                    0,
                    qjs::JS_CFUNC_GENERIC,
                    0,
                );
                let _ =
                    qjs::JS_SetPropertyStr(ctx, perf, b"now\0".as_ptr() as *const c_char, now_fn);
                let _ =
                    qjs::JS_SetPropertyStr(ctx, global, perf_key.as_ptr() as *const c_char, perf);
            } else {
                qjs::js_free_value(ctx, perf);
            }
        }
    }

    ensure_global_intl(ctx, global);
    ensure_global_console(ctx, global);
    qjs::browser::ensure_global_event_target_stubs(ctx, global);

    // Minimal timers used by many browser-style libs and for self-driven UI loops.
    qjs::timers::install_globals(ctx, global);

    // __trueos_poll_mouse_raw(): bridge to kernel mouse queue.
    {
        unsafe extern "C" fn poll_mouse_raw(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: c_int,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            let mut buttons: u8 = 0;
            let mut dx: i8 = 0;
            let mut dy: i8 = 0;
            let mut wheel: i8 = 0;
            let rc = unsafe {
                qjs::trueos_shims::trueos_cabi_input_pop_mouse(
                    &mut buttons as *mut u8,
                    &mut dx as *mut i8,
                    &mut dy as *mut i8,
                    &mut wheel as *mut i8,
                )
            };
            if rc <= 0 {
                return qjs::JSValue {
                    u: qjs::JSValueUnion { int32: 0 },
                    tag: qjs::JS_TAG_NULL,
                };
            }
            let o = unsafe { qjs::JS_NewObject(ctx) };
            if o.is_exception() {
                return o;
            }
            unsafe {
                let _ = qjs::JS_SetPropertyStr(
                    ctx,
                    o,
                    b"buttons\0".as_ptr() as *const c_char,
                    qjs::JS_NewFloat64(ctx, buttons as f64),
                );
                let _ = qjs::JS_SetPropertyStr(
                    ctx,
                    o,
                    b"dx\0".as_ptr() as *const c_char,
                    qjs::JS_NewFloat64(ctx, dx as f64),
                );
                let _ = qjs::JS_SetPropertyStr(
                    ctx,
                    o,
                    b"dy\0".as_ptr() as *const c_char,
                    qjs::JS_NewFloat64(ctx, dy as f64),
                );
                let _ = qjs::JS_SetPropertyStr(
                    ctx,
                    o,
                    b"wheel\0".as_ptr() as *const c_char,
                    qjs::JS_NewFloat64(ctx, wheel as f64),
                );
            }
            o
        }

        let f = unsafe {
            qjs::JS_NewCFunction2(
                ctx,
                Some(poll_mouse_raw),
                b"__trueos_poll_mouse_raw\0".as_ptr() as *const c_char,
                0,
                qjs::JS_CFUNC_GENERIC,
                0,
            )
        };
        let _ = unsafe {
            qjs::JS_SetPropertyStr(
                ctx,
                global,
                b"__trueos_poll_mouse_raw\0".as_ptr() as *const c_char,
                f,
            )
        };
    }
    ensure_global_browser_rendering_ctors(ctx, global);
    let doc_obj = ensure_global_document_stub(ctx, global);
    qjs::js_free_value(ctx, doc_obj);
    qjs::js_free_value(ctx, global);
    Ok(())
}

unsafe fn ensure_global_document_stub(
    ctx: *mut qjs::JSContext,
    global: qjs::JSValue,
) -> qjs::JSValue {
    unsafe extern "C" fn document_create_element(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let node = qjs::browser::make_dom_like_element(ctx);
        if node.is_exception() {
            return node;
        }
        if argv.is_null() || argc < 1 {
            return node;
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let tag_cstr = qjs::js_to_cstring(ctx, args[0]);
        if tag_cstr.is_null() {
            return node;
        }
        // ownerDocument linkage is required by Pixi's EventSystem setup.
        let g = qjs::JS_GetGlobalObject(ctx);
        if !g.is_exception() {
            let doc = qjs::JS_GetPropertyStr(ctx, g, b"document\0".as_ptr() as *const c_char);
            if !doc.is_exception()
                && doc.tag != qjs::JS_TAG_UNDEFINED
                && doc.tag != qjs::JS_TAG_NULL
            {
                let _ = qjs::JS_SetPropertyStr(
                    ctx,
                    node,
                    b"ownerDocument\0".as_ptr() as *const c_char,
                    qjs::js_dup_value(ctx, doc),
                );
            }
            qjs::js_free_value(ctx, doc);
        }
        qjs::js_free_value(ctx, g);

        let tag = CStr::from_ptr(tag_cstr).to_bytes();
        if tag.eq_ignore_ascii_case(b"canvas") {
            // Make this object pass `instanceof HTMLCanvasElement` checks used by Pixi resource detection.
            let g2 = qjs::JS_GetGlobalObject(ctx);
            if !g2.is_exception() {
                let ctor = qjs::JS_GetPropertyStr(
                    ctx,
                    g2,
                    b"HTMLCanvasElement\0".as_ptr() as *const c_char,
                );
                if !ctor.is_exception()
                    && ctor.tag != qjs::JS_TAG_UNDEFINED
                    && ctor.tag != qjs::JS_TAG_NULL
                {
                    let proto =
                        qjs::JS_GetPropertyStr(ctx, ctor, b"prototype\0".as_ptr() as *const c_char);
                    if !proto.is_exception()
                        && proto.tag != qjs::JS_TAG_UNDEFINED
                        && proto.tag != qjs::JS_TAG_NULL
                    {
                        let _ = qjs::JS_SetPropertyStr(
                            ctx,
                            node,
                            b"__proto__\0".as_ptr() as *const c_char,
                            qjs::js_dup_value(ctx, proto),
                        );
                    }
                    qjs::js_free_value(ctx, proto);
                }
                qjs::js_free_value(ctx, ctor);
            }
            qjs::js_free_value(ctx, g2);

            let _ = qjs::JS_SetPropertyStr(
                ctx,
                node,
                b"width\0".as_ptr() as *const c_char,
                qjs::JS_NewFloat64(ctx, 800.0),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                node,
                b"height\0".as_ptr() as *const c_char,
                qjs::JS_NewFloat64(ctx, 600.0),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                node,
                b"nodeName\0".as_ptr() as *const c_char,
                qjs::JS_NewStringLen(ctx, b"CANVAS\0".as_ptr() as *const c_char, 6),
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                node,
                b"tagName\0".as_ptr() as *const c_char,
                qjs::JS_NewStringLen(ctx, b"CANVAS\0".as_ptr() as *const c_char, 6),
            );
            let get_ctx_fn = qjs::JS_NewCFunction2(
                ctx,
                Some(webgl::canvas_get_context),
                b"getContext\0".as_ptr() as *const c_char,
                2,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ = qjs::JS_SetPropertyStr(
                ctx,
                node,
                b"getContext\0".as_ptr() as *const c_char,
                get_ctx_fn,
            );

            // Remember the first created canvas as the primary event target.
            let g3 = qjs::JS_GetGlobalObject(ctx);
            if !g3.is_exception() {
                let existing = qjs::JS_GetPropertyStr(
                    ctx,
                    g3,
                    b"__trueos_primary_canvas\0".as_ptr() as *const c_char,
                );
                let needs = existing.is_exception()
                    || existing.tag == qjs::JS_TAG_UNDEFINED
                    || existing.tag == qjs::JS_TAG_NULL;
                qjs::js_free_value(ctx, existing);
                if needs {
                    let _ = qjs::JS_SetPropertyStr(
                        ctx,
                        g3,
                        b"__trueos_primary_canvas\0".as_ptr() as *const c_char,
                        qjs::js_dup_value(ctx, node),
                    );
                }
                qjs::js_free_value(ctx, g3);
            }
        }
        qjs::JS_FreeCString(ctx, tag_cstr);
        node
    }

    let existing = qjs::JS_GetPropertyStr(ctx, global, b"document\0".as_ptr() as *const c_char);
    if !existing.is_exception()
        && existing.tag != qjs::JS_TAG_UNDEFINED
        && existing.tag != qjs::JS_TAG_NULL
    {
        return existing;
    }
    qjs::js_free_value(ctx, existing);

    let doc = qjs::JS_NewObject(ctx);
    if doc.is_exception() {
        return doc;
    }
    qjs::browser::ensure_global_event_target_stubs(ctx, doc);

    let doc_el = qjs::browser::make_dom_like_element(ctx);
    if !doc_el.is_exception() {
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            doc,
            b"documentElement\0".as_ptr() as *const c_char,
            qjs::js_dup_value(ctx, doc_el),
        );
        qjs::js_free_value(ctx, doc_el);
    }

    let body = qjs::browser::make_dom_like_element(ctx);
    if !body.is_exception() {
        let _ = qjs::JS_SetPropertyStr(ctx, doc, b"body\0".as_ptr() as *const c_char, body);
    } else {
        qjs::js_free_value(ctx, body);
    }
    let head = qjs::browser::make_dom_like_element(ctx);
    if !head.is_exception() {
        let _ = qjs::JS_SetPropertyStr(ctx, doc, b"head\0".as_ptr() as *const c_char, head);
    } else {
        qjs::js_free_value(ctx, head);
    }

    let create_el = qjs::JS_NewCFunction2(
        ctx,
        Some(document_create_element),
        b"createElement\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        doc,
        b"createElement\0".as_ptr() as *const c_char,
        create_el,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"document\0".as_ptr() as *const c_char,
        qjs::js_dup_value(ctx, doc),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        doc,
        b"defaultView\0".as_ptr() as *const c_char,
        qjs::js_dup_value(ctx, global),
    );
    doc
}

unsafe fn ensure_global_console(ctx: *mut qjs::JSContext, global: qjs::JSValue) {
    unsafe fn console_emit(
        ctx: *mut qjs::JSContext,
        level: &[u8],
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) {
        log_str("qjs: ");
        log_bytes(level);
        log_str(": ");
        if argv.is_null() || argc <= 0 {
            log_str("\n");
            return;
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        for (i, arg) in args.iter().enumerate() {
            if i != 0 {
                log_str(" ");
            }
            let mut len: usize = 0;
            let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, *arg, 0);
            if cstr.is_null() {
                log_str("<toString-error>");
                continue;
            }
            let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
            log_bytes(bytes);
            qjs::JS_FreeCString(ctx, cstr);
        }
        log_str("\n");
    }

    unsafe extern "C" fn console_log(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        console_emit(ctx, b"log", argc, argv);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn console_info(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        console_emit(ctx, b"info", argc, argv);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn console_debug(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        console_emit(ctx, b"debug", argc, argv);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn console_warn(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        console_emit(ctx, b"warn", argc, argv);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn console_error(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        console_emit(ctx, b"error", argc, argv);
        qjs::JSValue::undefined()
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
        return;
    }

    macro_rules! set_console_fn {
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
            let _ = qjs::JS_SetPropertyStr(ctx, console, k.as_ptr() as *const c_char, f);
        }};
    }

    set_console_fn!("log", console_log, 1);
    set_console_fn!("info", console_info, 1);
    set_console_fn!("debug", console_debug, 1);
    set_console_fn!("warn", console_warn, 1);
    set_console_fn!("error", console_error, 1);

    let _ = qjs::JS_SetPropertyStr(ctx, global, b"console\0".as_ptr() as *const c_char, console);
    let global_log_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(console_log),
        b"globalLog\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"globalLog\0".as_ptr() as *const c_char,
        global_log_fn,
    );
}

unsafe fn ensure_global_browser_rendering_ctors(ctx: *mut qjs::JSContext, global: qjs::JSValue) {
    unsafe extern "C" fn ctor_noop(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JS_NewObject(ctx)
    }

    let names: [&[u8]; 3] = [
        b"HTMLCanvasElement\0",
        b"WebGLRenderingContext\0",
        b"CanvasRenderingContext2D\0",
    ];
    for name in names {
        let existing = qjs::JS_GetPropertyStr(ctx, global, name.as_ptr() as *const c_char);
        let needs_install = existing.is_exception()
            || existing.tag == qjs::JS_TAG_UNDEFINED
            || existing.tag == qjs::JS_TAG_NULL;
        qjs::js_free_value(ctx, existing);
        if !needs_install {
            continue;
        }
        let ctor = qjs::JS_NewCFunction2(
            ctx,
            Some(ctor_noop),
            name.as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_CONSTRUCTOR,
            0,
        );
        // Give constructor a stable prototype object so instanceof checks can work.
        let proto = qjs::JS_NewObject(ctx);
        if !proto.is_exception() {
            let _ =
                qjs::JS_SetPropertyStr(ctx, ctor, b"prototype\0".as_ptr() as *const c_char, proto);
        } else {
            qjs::js_free_value(ctx, proto);
        }
        let _ = qjs::JS_SetPropertyStr(ctx, global, name.as_ptr() as *const c_char, ctor);
    }
}

unsafe fn ensure_global_intl(ctx: *mut qjs::JSContext, global: qjs::JSValue) {
    let key = b"Intl\0";
    let existing = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    let needs_install = existing.is_exception()
        || existing.tag == qjs::JS_TAG_UNDEFINED
        || existing.tag == qjs::JS_TAG_NULL;
    qjs::js_free_value(ctx, existing);
    if !needs_install {
        return;
    }

    unsafe extern "C" fn intl_resolved_options(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JS_NewObject(ctx)
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

    unsafe fn make_formatter_object(ctx: *mut qjs::JSContext) -> qjs::JSValue {
        let obj = qjs::JS_NewObject(ctx);
        if obj.is_exception() {
            return obj;
        }
        let format_fn = qjs::JS_NewCFunction2(
            ctx,
            Some(intl_format),
            b"format\0".as_ptr() as *const c_char,
            1,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let ro_fn = qjs::JS_NewCFunction2(
            ctx,
            Some(intl_resolved_options),
            b"resolvedOptions\0".as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"format\0".as_ptr() as *const c_char, format_fn);
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            b"resolvedOptions\0".as_ptr() as *const c_char,
            ro_fn,
        );
        obj
    }

    unsafe extern "C" fn intl_number_ctor(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        make_formatter_object(ctx)
    }

    unsafe extern "C" fn intl_simple_ctor(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let obj = qjs::JS_NewObject(ctx);
        if obj.is_exception() {
            return obj;
        }
        let ro_fn = qjs::JS_NewCFunction2(
            ctx,
            Some(intl_resolved_options),
            b"resolvedOptions\0".as_ptr() as *const c_char,
            0,
            qjs::JS_CFUNC_GENERIC,
            0,
        );
        let _ = qjs::JS_SetPropertyStr(
            ctx,
            obj,
            b"resolvedOptions\0".as_ptr() as *const c_char,
            ro_fn,
        );
        obj
    }

    unsafe extern "C" fn intl_get_canonical_locales(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: c_int,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JS_NewArray(ctx)
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
        let val = if !argv.is_null() && argc > 0 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut len: usize = 0;
            let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0);
            if cstr.is_null() {
                qjs::JS_NewStringLen(ctx, b"en-US\0".as_ptr() as *const c_char, 5)
            } else {
                let out = qjs::JS_NewStringLen(ctx, cstr, len);
                qjs::JS_FreeCString(ctx, cstr);
                out
            }
        } else {
            qjs::JS_NewStringLen(ctx, b"en-US\0".as_ptr() as *const c_char, 5)
        };
        let _ = qjs::JS_SetPropertyStr(ctx, obj, b"baseName\0".as_ptr() as *const c_char, val);
        obj
    }

    let intl = qjs::JS_NewObject(ctx);
    if intl.is_exception() {
        return;
    }
    let number_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_number_ctor),
        b"NumberFormat\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let dt_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_number_ctor),
        b"DateTimeFormat\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let collator_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"Collator\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let plural_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"PluralRules\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let rtf_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"RelativeTimeFormat\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let list_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"ListFormat\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let display_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_simple_ctor),
        b"DisplayNames\0".as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let locale_ctor = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_locale_ctor),
        b"Locale\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_CONSTRUCTOR,
        0,
    );
    let get_can = qjs::JS_NewCFunction2(
        ctx,
        Some(intl_get_canonical_locales),
        b"getCanonicalLocales\0".as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"NumberFormat\0".as_ptr() as *const c_char,
        number_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"DateTimeFormat\0".as_ptr() as *const c_char,
        dt_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"Collator\0".as_ptr() as *const c_char,
        collator_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"PluralRules\0".as_ptr() as *const c_char,
        plural_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"RelativeTimeFormat\0".as_ptr() as *const c_char,
        rtf_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"ListFormat\0".as_ptr() as *const c_char,
        list_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"DisplayNames\0".as_ptr() as *const c_char,
        display_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"Locale\0".as_ptr() as *const c_char,
        locale_ctor,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        intl,
        b"getCanonicalLocales\0".as_ptr() as *const c_char,
        get_can,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, intl);
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

unsafe extern "C" fn qjs_complex_module_init(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
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
            let _ = qjs::JS_Call(
                ctx,
                reject,
                qjs::JSValue::undefined(),
                1,
                &arg as *const qjs::JSValue,
            );
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
            let _ = qjs::JS_Call(
                ctx,
                reject,
                qjs::JSValue::undefined(),
                1,
                &arg as *const qjs::JSValue,
            );
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
            let _ = qjs::JS_Call(
                ctx,
                reject,
                qjs::JSValue::undefined(),
                1,
                &arg as *const qjs::JSValue,
            );
        }
    }

    qjs::js_free_value(ctx, resolve);
    qjs::js_free_value(ctx, reject);
    promise
}

unsafe extern "C" fn qjs_fs_module_init(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
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
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        read_text_async_name.as_ptr() as *const c_char,
        read_text_async_fn,
    ) < 0
    {
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
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        read_bytes_async_name.as_ptr() as *const c_char,
        read_bytes_async_fn,
    ) < 0
    {
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
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        write_text_async_name.as_ptr() as *const c_char,
        write_text_async_fn,
    ) < 0
    {
        return -1;
    }

    0
}

unsafe fn js_get_arraybuffer_view(
    ctx: *mut qjs::JSContext,
    val: qjs::JSValueConst,
) -> Option<(*const u8, usize)> {
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
            let end = byte_off.saturating_add(byte_len);
            if end <= total {
                return Some((ptr.add(byte_off) as *const u8, byte_len));
            }
        }
    } else {
        qjs::js_free_value(ctx, ab);
    }
    let mut total: usize = 0;
    let ptr = qjs::JS_GetArrayBuffer(ctx, &mut total as *mut usize, val);
    if !ptr.is_null() {
        Some((ptr as *const u8, total))
    } else {
        None
    }
}

unsafe extern "C" fn qjs_cmd_stream_begin_frame(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::BeginFrame);
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_end_frame(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: c_int,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::EndFrame);
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_set_clear_rgb(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut v_f: f64 = 0.0;
    if qjs::JS_ToFloat64(ctx, &mut v_f as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    let rgb = (v_f as i64).max(0) as u32 & 0x00FF_FFFF;
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetClearColor { clear_rgb: rgb });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_set_viewport(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut w_f: f64 = 0.0;
    let mut h_f: f64 = 0.0;
    if qjs::JS_ToFloat64(ctx, &mut w_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut h_f as *mut f64, args[1]) != 0
    {
        return qjs::JSValue::undefined();
    }
    let w = (w_f as i32).max(0);
    let h = (h_f as i32).max(0);
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetViewport { w, h });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_set_blend_enabled(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut enabled_f: f64 = 0.0;
    if qjs::JS_ToFloat64(ctx, &mut enabled_f as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    let enabled = enabled_f != 0.0;
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetBlendEnabled { enabled });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_set_blend_func(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 4 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut sr: f64 = 0.0;
    let mut dr: f64 = 0.0;
    let mut sa: f64 = 0.0;
    let mut da: f64 = 0.0;
    if qjs::JS_ToFloat64(ctx, &mut sr as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut dr as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut sa as *mut f64, args[2]) != 0
        || qjs::JS_ToFloat64(ctx, &mut da as *mut f64, args[3]) != 0
    {
        return qjs::JSValue::undefined();
    }
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetBlendFunc {
        src_rgb: (sr as i32).max(0) as u32,
        dst_rgb: (dr as i32).max(0) as u32,
        src_alpha: (sa as i32).max(0) as u32,
        dst_alpha: (da as i32).max(0) as u32,
    });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_set_blend_equation(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 2 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut rgb: f64 = 0.0;
    let mut alpha: f64 = 0.0;
    if qjs::JS_ToFloat64(ctx, &mut rgb as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut alpha as *mut f64, args[1]) != 0
    {
        return qjs::JSValue::undefined();
    }
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::SetBlendEquation {
        rgb: (rgb as i32).max(0) as u32,
        alpha: (alpha as i32).max(0) as u32,
    });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_draw_triangles_u8(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: c_int,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argv.is_null() || argc < 1 {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let Some((ptr, len)) = js_get_arraybuffer_view(ctx, args[0]) else {
        return qjs::JSValue::undefined();
    };
    let bytes = core::slice::from_raw_parts(ptr, len).to_vec();
    cmd_stream::enqueue(cmd_stream::CmdStreamCommand::DrawTriangles { vertices: bytes });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_module_init(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> c_int {
    let begin_frame_name = b"beginFrame\0";
    let end_frame_name = b"endFrame\0";
    let set_clear_rgb_name = b"setClearRgb\0";
    let set_viewport_name = b"setViewport\0";
    let set_blend_enabled_name = b"setBlendEnabled\0";
    let set_blend_func_name = b"setBlendFunc\0";
    let set_blend_equation_name = b"setBlendEquation\0";
    let draw_triangles_u8_name = b"drawTrianglesU8\0";

    let begin_frame_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_cmd_stream_begin_frame),
        begin_frame_name.as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        begin_frame_name.as_ptr() as *const c_char,
        begin_frame_fn,
    ) < 0
    {
        return -1;
    }
    let end_frame_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_cmd_stream_end_frame),
        end_frame_name.as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        end_frame_name.as_ptr() as *const c_char,
        end_frame_fn,
    ) < 0
    {
        return -1;
    }
    let set_clear_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_cmd_stream_set_clear_rgb),
        set_clear_rgb_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        set_clear_rgb_name.as_ptr() as *const c_char,
        set_clear_fn,
    ) < 0
    {
        return -1;
    }
    let set_viewport_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_cmd_stream_set_viewport),
        set_viewport_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        set_viewport_name.as_ptr() as *const c_char,
        set_viewport_fn,
    ) < 0
    {
        return -1;
    }
    let set_blend_enabled_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_cmd_stream_set_blend_enabled),
        set_blend_enabled_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        set_blend_enabled_name.as_ptr() as *const c_char,
        set_blend_enabled_fn,
    ) < 0
    {
        return -1;
    }
    let set_blend_func_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_cmd_stream_set_blend_func),
        set_blend_func_name.as_ptr() as *const c_char,
        4,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        set_blend_func_name.as_ptr() as *const c_char,
        set_blend_func_fn,
    ) < 0
    {
        return -1;
    }
    let set_blend_equation_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_cmd_stream_set_blend_equation),
        set_blend_equation_name.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        set_blend_equation_name.as_ptr() as *const c_char,
        set_blend_equation_fn,
    ) < 0
    {
        return -1;
    }
    let draw_triangles_u8_fn = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_cmd_stream_draw_triangles_u8),
        draw_triangles_u8_name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    if qjs::JS_SetModuleExport(
        ctx,
        m,
        draw_triangles_u8_name.as_ptr() as *const c_char,
        draw_triangles_u8_fn,
    ) < 0
    {
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
            &[
                b"join\0",
                b"dirname\0",
                b"basename\0",
                b"extname\0",
                b"resolve\0",
            ],
        )
    } else if name == b"cmd_stream" || name == b"trueos:cmd_stream" {
        (
            qjs_cmd_stream_module_init,
            &[
                b"beginFrame\0",
                b"endFrame\0",
                b"setClearRgb\0",
                b"setViewport\0",
                b"setBlendEnabled\0",
                b"setBlendFunc\0",
                b"setBlendEquation\0",
                b"drawTrianglesU8\0",
            ],
        )
    } else if name == b"text" || name == b"trueos:text" {
        (
            qjs_text_module_init,
            &[b"getFontAtlasSmall\0", b"getFontAtlasLarge\0"],
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

pub(crate) unsafe fn throw_error(ctx: *mut qjs::JSContext, msg: &[u8]) {
    let err = qjs::JS_NewError(ctx);
    if err.is_exception() {
        return;
    }
    let m = qjs::JS_NewStringLen(ctx, msg.as_ptr() as *const c_char, msg.len());
    let _ = qjs::JS_SetPropertyStr(ctx, err, b"message\0".as_ptr() as *const c_char, m);
    let _ = qjs::JS_Throw(ctx, err);
}

/// Compatibility wrapper: loader implementation moved to `trueos_module_loader.rs`.
pub unsafe fn install(rt: *mut qjs::JSRuntime) {
    qjs::trueos_module_loader::install(rt);
}
