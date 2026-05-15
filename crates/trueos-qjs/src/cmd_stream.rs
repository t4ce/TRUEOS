#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::vec::Vec;
use core::ffi::{c_char, CStr};
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_gfx_upload_texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_png(tex_id: u32, data_ptr: *const u8, data_len: usize)
        -> i32;
    fn trueos_cabi_gfx_upload_texture_png_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_jpeg(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_jpeg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_svg(tex_id: u32, data_ptr: *const u8, data_len: usize)
        -> i32;
    fn trueos_cabi_gfx_upload_texture_svg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32;
}

static CMD_STREAM_NEXT_TEX_ID: AtomicU32 = AtomicU32::new(16);
static CMD_STREAM_TEX_IDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

#[inline]
fn cmd_stream_alloc_tex_id() -> u32 {
    let id = CMD_STREAM_NEXT_TEX_ID.fetch_add(1, Ordering::AcqRel);
    CMD_STREAM_TEX_IDS.lock().push(id);
    id
}

#[inline]
pub fn alloc_managed_tex_id() -> u32 {
    cmd_stream_alloc_tex_id()
}

#[inline]
fn cmd_stream_is_managed_tex(id: u32) -> bool {
    id != 0 && CMD_STREAM_TEX_IDS.lock().iter().copied().any(|v| v == id)
}

#[inline]
fn cmd_stream_release_tex_id(id: u32) {
    let mut ids = CMD_STREAM_TEX_IDS.lock();
    if let Some(pos) = ids.iter().position(|v| *v == id) {
        ids.swap_remove(pos);
    }
}

#[inline]
pub fn release_managed_tex_id(id: u32) {
    if cmd_stream_is_managed_tex(id) {
        cmd_stream_release_tex_id(id);
    }
}

#[inline]
unsafe fn cmd_stream_args<'a>(
    argv: *const qjs::JSValueConst,
    argc: i32,
    min_args: usize,
) -> Option<&'a [qjs::JSValueConst]> {
    if argv.is_null() || argc < min_args as i32 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(argv, argc as usize) })
}

#[inline]
unsafe fn cmd_stream_arg_f64(
    ctx: *mut qjs::JSContext,
    args: &[qjs::JSValueConst],
    index: usize,
) -> Option<f64> {
    let value = *args.get(index)?;
    let mut out = 0.0;
    if unsafe { qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) } != 0 {
        return None;
    }
    Some(out)
}

#[inline]
unsafe fn cmd_stream_with_u8_buffer<R>(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValueConst,
    f: impl FnOnce(*const u8, usize) -> R,
) -> Option<R> {
    let mut byte_off: usize = 0;
    let mut byte_len: usize = 0;
    let mut bpe: usize = 0;
    let ab = unsafe {
        qjs::JS_GetTypedArrayBuffer(
            ctx,
            value,
            &mut byte_off as *mut usize,
            &mut byte_len as *mut usize,
            &mut bpe as *mut usize,
        )
    };

    if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED && ab.tag != qjs::JS_TAG_NULL {
        let mut buf_len: usize = 0;
        let ptr = unsafe { qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab) };
        let out = if ptr.is_null() {
            None
        } else {
            let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
            Some(f(unsafe { ptr.add(byte_off) } as *const u8, usable))
        };
        unsafe { qjs::js_free_value(ctx, ab) };
        return out;
    }
    if !ab.is_exception() {
        unsafe { qjs::js_free_value(ctx, ab) };
    }

    let mut len: usize = 0;
    let ptr = unsafe { qjs::JS_GetArrayBuffer(ctx, &mut len as *mut usize, value) };
    if ptr.is_null() {
        return None;
    }
    Some(f(ptr as *const u8, len))
}

type CmdStreamEncodedUploadFn = unsafe extern "C" fn(u32, *const u8, usize) -> i32;

#[inline]
unsafe fn cmd_stream_create_texture_from_encoded(
    ctx: *mut qjs::JSContext,
    argc: i32,
    argv: *const qjs::JSValueConst,
    upload: CmdStreamEncodedUploadFn,
) -> qjs::JSValue {
    let Some(args) = (unsafe { cmd_stream_args(argv, argc, 1) }) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = cmd_stream_alloc_tex_id();

    let mut uploaded = false;
    let _ = unsafe {
        cmd_stream_with_u8_buffer(ctx, args[0], |ptr, len| {
            if len > 0 && upload(tex_id, ptr, len) == 0 {
                uploaded = true;
            }
        })
    };
    if !uploaded {
        cmd_stream_release_tex_id(tex_id);
        return qjs::JSValue::undefined();
    }
    unsafe { qjs::JS_NewFloat64(ctx, tex_id as f64) }
}

#[inline]
unsafe fn cmd_stream_update_texture_from_encoded(
    ctx: *mut qjs::JSContext,
    argc: i32,
    argv: *const qjs::JSValueConst,
    upload: CmdStreamEncodedUploadFn,
) -> qjs::JSValue {
    let Some(args) = (unsafe { cmd_stream_args(argv, argc, 2) }) else {
        return qjs::JSValue::undefined();
    };
    let Some(tex_id_f) = (unsafe { cmd_stream_arg_f64(ctx, args, 0) }) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    if !cmd_stream_is_managed_tex(tex_id) {
        return qjs::JSValue::undefined();
    }

    let _ = unsafe {
        cmd_stream_with_u8_buffer(ctx, args[1], |ptr, len| {
            if len > 0 {
                let _ = upload(tex_id, ptr, len);
            }
        })
    };
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_rgba(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = (unsafe { cmd_stream_args(argv, argc, 3) }) else {
        return qjs::JSValue::undefined();
    };
    let Some(w_f) = (unsafe { cmd_stream_arg_f64(ctx, args, 0) }) else {
        return qjs::JSValue::undefined();
    };
    let Some(h_f) = (unsafe { cmd_stream_arg_f64(ctx, args, 1) }) else {
        return qjs::JSValue::undefined();
    };
    let w = (w_f as i64).max(1) as u32;
    let h = (h_f as i64).max(1) as u32;
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if need == 0 {
        return qjs::JSValue::undefined();
    }
    let tex_id = cmd_stream_alloc_tex_id();

    let mut uploaded = false;
    let _ = unsafe {
        cmd_stream_with_u8_buffer(ctx, args[2], |ptr, len| {
            if len >= need && trueos_cabi_gfx_upload_texture_rgba(tex_id, w, h, ptr, need) == 0 {
                uploaded = true;
            }
        })
    };
    if !uploaded {
        cmd_stream_release_tex_id(tex_id);
        return qjs::JSValue::undefined();
    }
    unsafe { qjs::JS_NewFloat64(ctx, tex_id as f64) }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_png(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_png)
    }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_png_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(
            ctx,
            argc,
            argv,
            trueos_cabi_gfx_upload_texture_png_async,
        )
    }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_jpeg(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_jpeg)
    }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_jpeg_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(
            ctx,
            argc,
            argv,
            trueos_cabi_gfx_upload_texture_jpeg_async,
        )
    }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_svg(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_svg)
    }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_svg_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(
            ctx,
            argc,
            argv,
            trueos_cabi_gfx_upload_texture_svg_async,
        )
    }
}

unsafe extern "C" fn qjs_cmd_stream_update_texture_rgba(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = (unsafe { cmd_stream_args(argv, argc, 4) }) else {
        return qjs::JSValue::undefined();
    };
    let Some(tex_id_f) = (unsafe { cmd_stream_arg_f64(ctx, args, 0) }) else {
        return qjs::JSValue::undefined();
    };
    let Some(w_f) = (unsafe { cmd_stream_arg_f64(ctx, args, 1) }) else {
        return qjs::JSValue::undefined();
    };
    let Some(h_f) = (unsafe { cmd_stream_arg_f64(ctx, args, 2) }) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    let w = (w_f as i64).max(1) as u32;
    let h = (h_f as i64).max(1) as u32;
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if !cmd_stream_is_managed_tex(tex_id) || need == 0 {
        return qjs::JSValue::undefined();
    }

    let _ = unsafe {
        cmd_stream_with_u8_buffer(ctx, args[3], |ptr, len| {
            if len >= need {
                let _ = trueos_cabi_gfx_upload_texture_rgba(tex_id, w, h, ptr, need);
            }
        })
    };
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_update_texture_png(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_update_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_png)
    }
}

unsafe extern "C" fn qjs_cmd_stream_update_texture_jpeg(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_update_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_jpeg)
    }
}

unsafe extern "C" fn qjs_cmd_stream_update_texture_svg(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_update_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_svg)
    }
}

unsafe extern "C" fn qjs_cmd_stream_destroy_texture(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = (unsafe { cmd_stream_args(argv, argc, 1) }) else {
        return qjs::JSValue::undefined();
    };
    let Some(tex_id_f) = (unsafe { cmd_stream_arg_f64(ctx, args, 0) }) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    if !cmd_stream_is_managed_tex(tex_id) {
        return qjs::JSValue::undefined();
    }
    let clear = [0u8, 0, 0, 0];
    let _ =
        unsafe { trueos_cabi_gfx_upload_texture_rgba(tex_id, 1, 1, clear.as_ptr(), clear.len()) };
    cmd_stream_release_tex_id(tex_id);
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_get_texture_status(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = (unsafe { cmd_stream_args(argv, argc, 1) }) else {
        return unsafe { qjs::JS_NewFloat64(ctx, 0.0) };
    };
    let Some(tex_id_f) = (unsafe { cmd_stream_arg_f64(ctx, args, 0) }) else {
        return unsafe { qjs::JS_NewFloat64(ctx, 0.0) };
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    unsafe { qjs::JS_NewFloat64(ctx, trueos_cabi_gfx_texture_status(tex_id) as f64) }
}

type CmdStreamQjsCallback = unsafe extern "C" fn(
    *mut qjs::JSContext,
    qjs::JSValueConst,
    i32,
    *const qjs::JSValueConst,
) -> qjs::JSValue;

type CmdStreamModuleExport = (&'static [u8], CmdStreamQjsCallback, i32);

const CMD_STREAM_MODULE_EXPORTS: &[CmdStreamModuleExport] = &[
    (b"createTextureRgba\0", qjs_cmd_stream_create_texture_rgba, 3),
    (b"createTexturePng\0", qjs_cmd_stream_create_texture_png, 1),
    (b"createTextureJpeg\0", qjs_cmd_stream_create_texture_jpeg, 1),
    (b"createTextureSvg\0", qjs_cmd_stream_create_texture_svg, 1),
    (b"createTexturePngAsync\0", qjs_cmd_stream_create_texture_png_async, 1),
    (b"createTextureJpegAsync\0", qjs_cmd_stream_create_texture_jpeg_async, 1),
    (b"createTextureSvgAsync\0", qjs_cmd_stream_create_texture_svg_async, 1),
    (b"updateTextureRgba\0", qjs_cmd_stream_update_texture_rgba, 4),
    (b"updateTexturePng\0", qjs_cmd_stream_update_texture_png, 2),
    (b"updateTextureJpeg\0", qjs_cmd_stream_update_texture_jpeg, 2),
    (b"updateTextureSvg\0", qjs_cmd_stream_update_texture_svg, 2),
    (b"getTextureStatus\0", qjs_cmd_stream_get_texture_status, 1),
    (b"destroyTexture\0", qjs_cmd_stream_destroy_texture, 1),
];

#[inline]
unsafe fn cmd_stream_register_module_exports(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
    exports: &[CmdStreamModuleExport],
) {
    for &(name, func, argc) in exports {
        let f = unsafe {
            qjs::JS_NewCFunction2(
                ctx,
                Some(func),
                name.as_ptr() as *const c_char,
                argc,
                qjs::JS_CFUNC_GENERIC,
                0,
            )
        };
        let _ = unsafe { qjs::JS_SetModuleExport(ctx, m, name.as_ptr() as *const c_char, f) };
    }
}

#[inline]
unsafe fn cmd_stream_add_module_exports(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
    exports: &[CmdStreamModuleExport],
) {
    for &(name, _, _) in exports {
        let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, name.as_ptr() as *const c_char) };
    }
}

unsafe extern "C" fn qjs_cmd_stream_module_init(
    ctx: *mut qjs::JSContext,
    m: *mut qjs::JSModuleDef,
) -> i32 {
    unsafe { cmd_stream_register_module_exports(ctx, m, CMD_STREAM_MODULE_EXPORTS) };
    0
}

#[inline]
pub(crate) unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(module_name) }.to_bytes();
    if name == b"cmd_stream" || name == b"trueos:cmd_stream" {
        let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(qjs_cmd_stream_module_init)) };
        if m.is_null() {
            return core::ptr::null_mut();
        }
        unsafe { cmd_stream_add_module_exports(ctx, m, CMD_STREAM_MODULE_EXPORTS) };
        return m;
    }

    core::ptr::null_mut()
}
