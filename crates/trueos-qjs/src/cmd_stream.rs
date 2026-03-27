#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use libm::{ceilf, floorf};
use spin::Mutex;
use trueos_gfx_core::{
    RGB_VERTEX_SIZE, Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_rgb_line_quad_px,
    push_rgb_quad_px, push_tex_quad_px,
};

use crate as qjs;

#[path = "cmd_stream/atlas_cmd_stream.rs"]
mod atlas_cmd_stream;
#[path = "cmd_stream/draw_cmd_stream.rs"]
mod draw_cmd_stream;
#[path = "cmd_stream/lyon_cmd_stream.rs"]
mod lyon_cmd_stream;

static FIRST_QJS_END_FRAME_SEEN: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_begin_frame_no_present(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
    fn trueos_cabi_signal_loadscreen_end();
    fn trueos_cabi_gfx_set_blend(
        enabled: u32,
        src_rgb: u32,
        dst_rgb: u32,
        src_alpha: u32,
        dst_alpha: u32,
        eq_rgb: u32,
        eq_alpha: u32,
    ) -> i32;
    fn trueos_cabi_gfx_draw_rgb_triangles_no_present(vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_gfx_draw_tex_triangles_no_present(
        tex_id: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_rgba_image(
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
    fn trueos_cabi_gfx_set_sampler(
        wrap_u: u32,
        wrap_v: u32,
        min_filter: u32,
        mag_filter: u32,
    ) -> i32;
    fn trueos_cabi_gfx_set_scissor(x: u32, y: u32, width: u32, height: u32) -> i32;
    fn trueos_cabi_gfx_clear_scissor() -> i32;
    fn trueos_cabi_gfx_set_render_target(tex_id: u32) -> i32;
    fn trueos_cabi_gfx_clear_render_target() -> i32;
    fn trueos_cabi_gfx_bake_lyon_icon_rgba(
        icon_id: u32,
        color_id: u32,
        small_set: u32,
        out_ptr: *mut u8,
        out_len: usize,
    ) -> i32;
}
static CMD_STREAM_CLEAR_RGB: AtomicU32 = AtomicU32::new(0xFFFFFF);
static CMD_STREAM_VIEW_W: AtomicU32 = AtomicU32::new(1280);
static CMD_STREAM_VIEW_H: AtomicU32 = AtomicU32::new(800);
static CMD_STREAM_BLEND_MODE: AtomicU32 = AtomicU32::new(0);
static CMD_STREAM_PMA: AtomicU32 = AtomicU32::new(0);
static CMD_STREAM_BLEND_ENABLED: AtomicU32 = AtomicU32::new(1);
static CMD_STREAM_NEXT_TEX_ID: AtomicU32 = AtomicU32::new(16);
static CMD_STREAM_ORIGIN_X_BITS: AtomicU32 = AtomicU32::new(0);
static CMD_STREAM_ORIGIN_Y_BITS: AtomicU32 = AtomicU32::new(0);
static CMD_STREAM_TEX_IDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static CMD_STREAM_ORIGIN_STACK: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());
static CMD_STREAM_CLIP_STACK: Mutex<Vec<Option<CmdStreamClipRect>>> = Mutex::new(Vec::new());
static CMD_STREAM_CUR_CLIP: Mutex<Option<CmdStreamClipRect>> = Mutex::new(None);

#[derive(Copy, Clone)]
struct CmdStreamClipRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

static CMD_STREAM_FRAME_OPEN: AtomicBool = AtomicBool::new(false);

const CMD_STREAM_DEFAULT_BLEND_MODE: u32 = 0;
const CMD_STREAM_DEFAULT_PMA: u32 = 0;
const CMD_STREAM_DEFAULT_BLEND_ENABLED: u32 = 0;
const CMD_STREAM_DEFAULT_WRAP_U: u32 = 0;
const CMD_STREAM_DEFAULT_WRAP_V: u32 = 0;
const CMD_STREAM_DEFAULT_MIN_FILTER: u32 = 1;
const CMD_STREAM_DEFAULT_MAG_FILTER: u32 = 1;

#[inline]
fn cmd_stream_apply_blend_mode(mode: u32, pma: bool) {
    match mode {
        // Add
        1 => {
            let _ = unsafe { trueos_cabi_gfx_set_blend(1, 0x0302, 1, 1, 1, 0, 0) };
        }
        // Multiply
        2 => {
            let _ = unsafe { trueos_cabi_gfx_set_blend(1, 0x0306, 0x0303, 0x0306, 0x0303, 0, 0) };
        }
        // Screen
        3 => {
            let _ = unsafe { trueos_cabi_gfx_set_blend(1, 1, 0x0301, 1, 0x0301, 0, 0) };
        }
        // Normal
        _ => {
            if pma {
                let _ = unsafe { trueos_cabi_gfx_set_blend(1, 1, 0x0303, 1, 0x0303, 0, 0) };
            } else {
                let _ =
                    unsafe { trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0) };
            }
        }
    }
}

#[inline]
fn cmd_stream_reset_frame_state_defaults() {
    CMD_STREAM_BLEND_MODE.store(CMD_STREAM_DEFAULT_BLEND_MODE, Ordering::Relaxed);
    CMD_STREAM_PMA.store(CMD_STREAM_DEFAULT_PMA, Ordering::Relaxed);
    CMD_STREAM_BLEND_ENABLED.store(CMD_STREAM_DEFAULT_BLEND_ENABLED, Ordering::Relaxed);

    let _ = unsafe {
        trueos_cabi_gfx_set_sampler(
            CMD_STREAM_DEFAULT_WRAP_U,
            CMD_STREAM_DEFAULT_WRAP_V,
            CMD_STREAM_DEFAULT_MIN_FILTER,
            CMD_STREAM_DEFAULT_MAG_FILTER,
        )
    };

    if CMD_STREAM_DEFAULT_BLEND_ENABLED != 0 {
        cmd_stream_apply_blend_mode(CMD_STREAM_DEFAULT_BLEND_MODE, CMD_STREAM_DEFAULT_PMA != 0);
    } else {
        let _ = unsafe { trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    }

    CMD_STREAM_ORIGIN_X_BITS.store(0.0f32.to_bits(), Ordering::Relaxed);
    CMD_STREAM_ORIGIN_Y_BITS.store(0.0f32.to_bits(), Ordering::Relaxed);
    CMD_STREAM_ORIGIN_STACK.lock().clear();
    CMD_STREAM_CLIP_STACK.lock().clear();
    *CMD_STREAM_CUR_CLIP.lock() = None;
    let _ = unsafe { trueos_cabi_gfx_clear_scissor() };
}

#[inline]
pub(crate) fn cmd_stream_origin_px() -> (f32, f32) {
    (
        f32::from_bits(CMD_STREAM_ORIGIN_X_BITS.load(Ordering::Relaxed)),
        f32::from_bits(CMD_STREAM_ORIGIN_Y_BITS.load(Ordering::Relaxed)),
    )
}

#[inline]
fn cmd_stream_set_origin_px(x: f32, y: f32) {
    CMD_STREAM_ORIGIN_X_BITS.store(x.to_bits(), Ordering::Relaxed);
    CMD_STREAM_ORIGIN_Y_BITS.store(y.to_bits(), Ordering::Relaxed);
}

#[inline]
fn cmd_stream_view_size() -> (u32, u32) {
    (
        CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1),
        CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1),
    )
}

#[inline]
fn cmd_stream_empty_clip_rect(view_w: u32, view_h: u32) -> CmdStreamClipRect {
    CmdStreamClipRect {
        x: view_w,
        y: view_h,
        width: 1,
        height: 1,
    }
}

#[inline]
fn cmd_stream_intersect_clip_rects(
    a: CmdStreamClipRect,
    b: CmdStreamClipRect,
    view_w: u32,
    view_h: u32,
) -> CmdStreamClipRect {
    let ax1 = a.x.saturating_add(a.width);
    let ay1 = a.y.saturating_add(a.height);
    let bx1 = b.x.saturating_add(b.width);
    let by1 = b.y.saturating_add(b.height);
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = ax1.min(bx1);
    let y1 = ay1.min(by1);
    if x1 <= x0 || y1 <= y0 {
        return cmd_stream_empty_clip_rect(view_w, view_h);
    }
    CmdStreamClipRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

#[inline]
fn cmd_stream_clip_rect_from_local(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Option<CmdStreamClipRect> {
    if !(width.is_finite() && height.is_finite() && x.is_finite() && y.is_finite()) {
        return None;
    }
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (origin_x, origin_y) = cmd_stream_origin_px();
    let x0 = floorf(x + origin_x);
    let y0 = floorf(y + origin_y);
    let x1 = ceilf(x + origin_x + width);
    let y1 = ceilf(y + origin_y + height);
    let left = x0.min(x1).max(0.0) as u32;
    let top = y0.min(y1).max(0.0) as u32;
    let right = x0.max(x1).max(0.0) as u32;
    let bottom = y0.max(y1).max(0.0) as u32;
    if right <= left || bottom <= top {
        return None;
    }
    Some(CmdStreamClipRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

#[inline]
fn cmd_stream_apply_clip_state(next: Option<CmdStreamClipRect>) {
    *CMD_STREAM_CUR_CLIP.lock() = next;
    match next {
        Some(rect) => {
            let _ = unsafe { trueos_cabi_gfx_set_scissor(rect.x, rect.y, rect.width, rect.height) };
        }
        None => {
            let _ = unsafe { trueos_cabi_gfx_clear_scissor() };
        }
    }
}

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
    if id == 0 {
        return false;
    }
    CMD_STREAM_TEX_IDS.lock().iter().copied().any(|v| v == id)
}

#[inline]
fn cmd_stream_release_tex_id(id: u32) {
    let mut ids = CMD_STREAM_TEX_IDS.lock();
    if let Some(pos) = ids.iter().position(|v| *v == id) {
        ids.swap_remove(pos);
    }
    atlas_cmd_stream::release_tex_id(id);
    lyon_cmd_stream::release_tex_id(id);
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
    Some(core::slice::from_raw_parts(argv, argc as usize))
}

#[inline]
unsafe fn cmd_stream_arg_f64(
    ctx: *mut qjs::JSContext,
    args: &[qjs::JSValueConst],
    index: usize,
) -> Option<f64> {
    let value = *args.get(index)?;
    let mut out = 0.0;
    if qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) != 0 {
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
    let ab = qjs::JS_GetTypedArrayBuffer(
        ctx,
        value,
        &mut byte_off as *mut usize,
        &mut byte_len as *mut usize,
        &mut bpe as *mut usize,
    );

    if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED && ab.tag != qjs::JS_TAG_NULL {
        let mut buf_len: usize = 0;
        let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab);
        let out = if ptr.is_null() {
            None
        } else {
            let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
            Some(f(ptr.add(byte_off) as *const u8, usable))
        };
        qjs::js_free_value(ctx, ab);
        return out;
    }
    if !ab.is_exception() {
        qjs::js_free_value(ctx, ab);
    }

    let mut len: usize = 0;
    let ptr = qjs::JS_GetArrayBuffer(ctx, &mut len as *mut usize, value);
    if ptr.is_null() {
        return None;
    }
    Some(f(ptr as *const u8, len))
}

#[inline]
unsafe fn cmd_stream_read_f32_slice_from_value(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValueConst,
) -> Option<(*mut f32, usize, qjs::JSValue)> {
    let mut byte_off: usize = 0;
    let mut byte_len: usize = 0;
    let mut bpe: usize = 0;
    let ab = qjs::JS_GetTypedArrayBuffer(
        ctx,
        value,
        &mut byte_off as *mut usize,
        &mut byte_len as *mut usize,
        &mut bpe as *mut usize,
    );

    if ab.is_exception() || ab.tag == qjs::JS_TAG_UNDEFINED || ab.tag == qjs::JS_TAG_NULL {
        if !ab.is_exception() {
            qjs::js_free_value(ctx, ab);
        }
        return None;
    }

    let mut buf_len: usize = 0;
    let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab);
    if ptr.is_null() {
        qjs::js_free_value(ctx, ab);
        return None;
    }

    let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
    if usable < 4 || (byte_off & 3) != 0 {
        qjs::js_free_value(ctx, ab);
        return None;
    }
    let f32_len = usable / 4;
    if f32_len == 0 {
        qjs::js_free_value(ctx, ab);
        return None;
    }

    Some((ptr.add(byte_off) as *mut f32, f32_len, ab))
}

type CmdStreamEncodedUploadFn = unsafe extern "C" fn(u32, *const u8, usize) -> i32;

#[inline]
unsafe fn cmd_stream_create_texture_from_encoded(
    ctx: *mut qjs::JSContext,
    argc: i32,
    argv: *const qjs::JSValueConst,
    upload: CmdStreamEncodedUploadFn,
) -> qjs::JSValue {
    let Some(args) = cmd_stream_args(argv, argc, 1) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = cmd_stream_alloc_tex_id();

    let mut uploaded = false;
    let _ = cmd_stream_with_u8_buffer(ctx, args[0], |ptr, len| {
        if len > 0 && upload(tex_id, ptr, len) == 0 {
            uploaded = true;
        }
    });
    if !uploaded {
        cmd_stream_release_tex_id(tex_id);
        return qjs::JSValue::undefined();
    }
    qjs::JS_NewFloat64(ctx, tex_id as f64)
}

#[inline]
unsafe fn cmd_stream_update_texture_from_encoded(
    ctx: *mut qjs::JSContext,
    argc: i32,
    argv: *const qjs::JSValueConst,
    upload: CmdStreamEncodedUploadFn,
) -> qjs::JSValue {
    let Some(args) = cmd_stream_args(argv, argc, 2) else {
        return qjs::JSValue::undefined();
    };
    let Some(tex_id_f) = cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    if !cmd_stream_is_managed_tex(tex_id) {
        return qjs::JSValue::undefined();
    }

    let _ = cmd_stream_with_u8_buffer(ctx, args[1], |ptr, len| {
        if len > 0 {
            let _ = upload(tex_id, ptr, len);
        }
    });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_rgba(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = cmd_stream_args(argv, argc, 3) else {
        return qjs::JSValue::undefined();
    };
    let Some(w_f) = cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let Some(h_f) = cmd_stream_arg_f64(ctx, args, 1) else {
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
    let _ = cmd_stream_with_u8_buffer(ctx, args[2], |ptr, len| {
        if len >= need {
            uploaded = true;
            let _ = trueos_cabi_gfx_upload_texture_rgba(tex_id, w, h, ptr, need);
        }
    });
    if !uploaded {
        cmd_stream_release_tex_id(tex_id);
        return qjs::JSValue::undefined();
    }
    qjs::JS_NewFloat64(ctx, tex_id as f64)
}

unsafe extern "C" fn qjs_cmd_stream_create_render_target(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = cmd_stream_args(argv, argc, 2) else {
        return qjs::JSValue::undefined();
    };
    let Some(w_f) = cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let Some(h_f) = cmd_stream_arg_f64(ctx, args, 1) else {
        return qjs::JSValue::undefined();
    };
    let w = (w_f as i64).max(1) as u32;
    let h = (h_f as i64).max(1) as u32;
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if need == 0 {
        return qjs::JSValue::undefined();
    }
    let tex_id = cmd_stream_alloc_tex_id();
    let zeros = vec![0u8; need];
    if trueos_cabi_gfx_upload_texture_rgba_image(tex_id, w, h, zeros.as_ptr(), zeros.len()) != 0 {
        cmd_stream_release_tex_id(tex_id);
        return qjs::JSValue::undefined();
    }
    qjs::JS_NewFloat64(ctx, tex_id as f64)
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_png(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_png) }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_png_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_png_async)
    }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_jpeg(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_jpeg) }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_jpeg_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_jpeg_async)
    }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_svg(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_svg) }
}

unsafe extern "C" fn qjs_cmd_stream_create_texture_svg_async(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe {
        cmd_stream_create_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_svg_async)
    }
}

unsafe extern "C" fn qjs_cmd_stream_update_texture_rgba(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = cmd_stream_args(argv, argc, 4) else {
        return qjs::JSValue::undefined();
    };
    let Some(tex_id_f) = cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let Some(w_f) = cmd_stream_arg_f64(ctx, args, 1) else {
        return qjs::JSValue::undefined();
    };
    let Some(h_f) = cmd_stream_arg_f64(ctx, args, 2) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    let w = (w_f as i64).max(1) as u32;
    let h = (h_f as i64).max(1) as u32;
    let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if !cmd_stream_is_managed_tex(tex_id) || need == 0 {
        return qjs::JSValue::undefined();
    }

    let _ = cmd_stream_with_u8_buffer(ctx, args[3], |ptr, len| {
        if len >= need {
            let _ = trueos_cabi_gfx_upload_texture_rgba(tex_id, w, h, ptr, need);
        }
    });
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_update_texture_png(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { cmd_stream_update_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_png) }
}

unsafe extern "C" fn qjs_cmd_stream_update_texture_jpeg(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { cmd_stream_update_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_jpeg) }
}

unsafe extern "C" fn qjs_cmd_stream_update_texture_svg(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { cmd_stream_update_texture_from_encoded(ctx, argc, argv, trueos_cabi_gfx_upload_texture_svg) }
}

unsafe extern "C" fn qjs_cmd_stream_destroy_texture(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = cmd_stream_args(argv, argc, 1) else {
        return qjs::JSValue::undefined();
    };
    let Some(tex_id_f) = cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    if !cmd_stream_is_managed_tex(tex_id) {
        return qjs::JSValue::undefined();
    }
    let clear = [0u8, 0, 0, 0];
    let _ = trueos_cabi_gfx_upload_texture_rgba(tex_id, 1, 1, clear.as_ptr(), clear.len());
    cmd_stream_release_tex_id(tex_id);
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_cmd_stream_get_texture_status(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = cmd_stream_args(argv, argc, 1) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let Some(tex_id_f) = cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    qjs::JS_NewFloat64(ctx, trueos_cabi_gfx_texture_status(tex_id) as f64)
}

fn cmd_stream_draw_texture_rect(
    tex_id: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    rgba: u32,
) -> bool {
    if !CMD_STREAM_FRAME_OPEN.load(Ordering::Relaxed) {
        return false;
    }
    if tex_id == 0 || !(width > 0.0 && height > 0.0) {
        return false;
    }

    let (origin_x, origin_y) = cmd_stream_origin_px();
    let left_px = x + origin_x;
    let top_px = y + origin_y;
    let right_px = left_px + width;
    let bottom_px = top_px + height;
    let transform = ViewTransform::from_extent(
        CMD_STREAM_VIEW_W.load(Ordering::Relaxed),
        CMD_STREAM_VIEW_H.load(Ordering::Relaxed),
    );
    let color = Rgba8::from_rgba_u32(rgba);

    let mut verts = Vec::with_capacity(6 * TEX_VERTEX_SIZE);
    push_tex_quad_px(
        &mut verts,
        transform,
        left_px,
        top_px,
        right_px,
        bottom_px,
        [u0, v0, u1, v1],
        color,
    );

    let rc = unsafe {
        trueos_cabi_gfx_draw_tex_triangles_no_present(tex_id, verts.as_ptr(), verts.len())
    };
    rc == 0
}

#[inline]
fn cmd_stream_draw_rgb_triangles(verts: &[u8], a: u8) -> bool {
    if !CMD_STREAM_FRAME_OPEN.load(Ordering::Relaxed) {
        return false;
    }
    if verts.is_empty() {
        return false;
    }

    let temp_alpha_blend = CMD_STREAM_BLEND_ENABLED.load(Ordering::Relaxed) == 0 && a < 255;
    if temp_alpha_blend {
        let _ = unsafe { trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0) };
    }

    let rc = unsafe { trueos_cabi_gfx_draw_rgb_triangles_no_present(verts.as_ptr(), verts.len()) };

    if temp_alpha_blend {
        let _ = unsafe { trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    }

    rc == 0
}

#[inline]
fn cmd_stream_fill_rect(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    rgba: u32,
    outline: bool,
    chamfer: bool,
) -> bool {
    let (origin_x, origin_y) = cmd_stream_origin_px();
    let x = x + origin_x;
    let y = y + origin_y;
    let x2 = x + width;
    let y2 = y + height;
    let left_px = x.min(x2);
    let right_px = x.max(x2);
    let top_px = y.min(y2);
    let bottom_px = y.max(y2);
    let rect_w = right_px - left_px;
    let rect_h = bottom_px - top_px;

    if !(rect_w > 0.0 && rect_h > 0.0) {
        return false;
    }

    let transform = ViewTransform::from_extent(
        CMD_STREAM_VIEW_W.load(Ordering::Relaxed),
        CMD_STREAM_VIEW_H.load(Ordering::Relaxed),
    );
    let color = Rgba8::from_rgba_u32(rgba);
    let a = color.a;

    let push_rect = |verts: &mut Vec<u8>,
                     rect_left_px: f32,
                     rect_top_px: f32,
                     rect_right_px: f32,
                     rect_bottom_px: f32| {
        push_rgb_quad_px(
            verts,
            transform,
            rect_left_px,
            rect_top_px,
            rect_right_px,
            rect_bottom_px,
            color,
        );
    };

    let use_chamfer = chamfer && rect_w >= 10.0 && rect_h >= 10.0;
    let chamfer_px = if use_chamfer { 5.0f32 } else { 0.0f32 };

    let push_chamfer_fill = |verts: &mut Vec<u8>| {
        let pts = [
            (left_px + chamfer_px, top_px),
            (right_px - chamfer_px, top_px),
            (right_px, top_px + chamfer_px),
            (right_px, bottom_px - chamfer_px),
            (right_px - chamfer_px, bottom_px),
            (left_px + chamfer_px, bottom_px),
            (left_px, bottom_px - chamfer_px),
            (left_px, top_px + chamfer_px),
        ];
        let cx = (left_px + right_px) * 0.5;
        let cy = (top_px + bottom_px) * 0.5;
        for i in 0..pts.len() {
            let (x1, y1) = pts[i];
            let (x2, y2) = pts[(i + 1) % pts.len()];
            trueos_gfx_core::push_rgb_vertex_bytes(
                verts,
                transform.rgb_vertex_px(cx, cy, color),
            );
            trueos_gfx_core::push_rgb_vertex_bytes(
                verts,
                transform.rgb_vertex_px(x1, y1, color),
            );
            trueos_gfx_core::push_rgb_vertex_bytes(
                verts,
                transform.rgb_vertex_px(x2, y2, color),
            );
        }
    };

    let push_chamfer_outline = |verts: &mut Vec<u8>| {
        let pts = [
            (left_px + chamfer_px, top_px),
            (right_px - chamfer_px, top_px),
            (right_px, top_px + chamfer_px),
            (right_px, bottom_px - chamfer_px),
            (right_px - chamfer_px, bottom_px),
            (left_px + chamfer_px, bottom_px),
            (left_px, bottom_px - chamfer_px),
            (left_px, top_px + chamfer_px),
        ];
        for i in 0..pts.len() {
            let (x1, y1) = pts[i];
            let (x2, y2) = pts[(i + 1) % pts.len()];
            push_rgb_line_quad_px(verts, transform, x1, y1, x2, y2, 1.0, color);
        }
    };

    if use_chamfer {
        let mut verts = Vec::with_capacity(if outline { 48 * RGB_VERTEX_SIZE } else { 24 * RGB_VERTEX_SIZE });
        if outline {
            push_chamfer_outline(&mut verts);
        } else {
            push_chamfer_fill(&mut verts);
        }
        return cmd_stream_draw_rgb_triangles(&verts, a);
    }

    if outline {
        let stroke = 1.0f32;
        let top_h = stroke.min(bottom_px - top_px);
        let bottom_h = stroke.min((bottom_px - top_px - top_h).max(0.0));
        let inner_top = top_px + top_h;
        let inner_bottom = (bottom_px - bottom_h).max(inner_top);
        let left_w = stroke.min(right_px - left_px);
        let right_w = stroke.min((right_px - left_px - left_w).max(0.0));

        let mut verts = Vec::with_capacity(24 * RGB_VERTEX_SIZE);
        push_rect(&mut verts, left_px, top_px, right_px, top_px + top_h);
        push_rect(
            &mut verts,
            left_px,
            bottom_px - bottom_h,
            right_px,
            bottom_px,
        );
        push_rect(
            &mut verts,
            left_px,
            inner_top,
            left_px + left_w,
            inner_bottom,
        );
        push_rect(
            &mut verts,
            right_px - right_w,
            inner_top,
            right_px,
            inner_bottom,
        );
        return cmd_stream_draw_rgb_triangles(&verts, a);
    }

    let mut verts = Vec::with_capacity(6 * RGB_VERTEX_SIZE);

    push_rect(&mut verts, left_px, top_px, right_px, bottom_px);

    cmd_stream_draw_rgb_triangles(&verts, a)
}

#[inline]
fn cmd_stream_draw_line(x1: f32, y1: f32, x2: f32, y2: f32, rgba: u32, thickness: f32) -> bool {
    let (origin_x, origin_y) = cmd_stream_origin_px();
    let x1 = x1 + origin_x;
    let y1 = y1 + origin_y;
    let x2 = x2 + origin_x;
    let y2 = y2 + origin_y;
    let transform = ViewTransform::from_extent(
        CMD_STREAM_VIEW_W.load(Ordering::Relaxed),
        CMD_STREAM_VIEW_H.load(Ordering::Relaxed),
    );
    let color = Rgba8::from_rgba_u32(rgba);
    let a = color.a;
    let mut verts = Vec::with_capacity(6 * RGB_VERTEX_SIZE);
    push_rgb_line_quad_px(&mut verts, transform, x1, y1, x2, y2, thickness, color);
    cmd_stream_draw_rgb_triangles(&verts, a)
}

    type CmdStreamQjsCallback = unsafe extern "C" fn(
        *mut qjs::JSContext,
        qjs::JSValueConst,
        i32,
        *const qjs::JSValueConst,
    ) -> qjs::JSValue;

    type CmdStreamModuleExport = (&'static [u8], CmdStreamQjsCallback, i32);

    #[inline]
    unsafe fn cmd_stream_register_module_exports(
        ctx: *mut qjs::JSContext,
        m: *mut qjs::JSModuleDef,
        exports: &[CmdStreamModuleExport],
    ) {
        for &(name, func, argc) in exports {
            let f = qjs::JS_NewCFunction2(
                ctx,
                Some(func),
                name.as_ptr() as *const c_char,
                argc,
                qjs::JS_CFUNC_GENERIC,
                0,
            );
            let _ = qjs::JS_SetModuleExport(ctx, m, name.as_ptr() as *const c_char, f);
        }
    }

    #[inline]
    unsafe fn cmd_stream_add_module_exports(
        ctx: *mut qjs::JSContext,
        m: *mut qjs::JSModuleDef,
        exports: &[CmdStreamModuleExport],
    ) {
        for &(name, _, _) in exports {
            let _ = qjs::JS_AddModuleExport(ctx, m, name.as_ptr() as *const c_char);
        }
    }

    unsafe extern "C" fn qjs_cmd_stream_begin_frame(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        atlas_cmd_stream::clear_text_batches();
        let clear = CMD_STREAM_CLEAR_RGB.load(Ordering::Relaxed);
        let rc = trueos_cabi_gfx_begin_frame_no_present(clear);
        let opened = rc == 0;
        CMD_STREAM_FRAME_OPEN.store(opened, Ordering::Relaxed);
        if opened {
            cmd_stream_reset_frame_state_defaults();
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_end_frame(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !CMD_STREAM_FRAME_OPEN.swap(false, Ordering::Relaxed) {
            atlas_cmd_stream::clear_text_batches();
            return qjs::JSValue::undefined();
        }
        atlas_cmd_stream::flush_text_batches();
        let _ = trueos_cabi_gfx_end_frame();
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_signal_loadscreen_end(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !FIRST_QJS_END_FRAME_SEEN.swap(true, Ordering::AcqRel) {
            trueos_cabi_signal_loadscreen_end();
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_clear_rgb(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 1) else {
            return qjs::JSValue::undefined();
        };
        let Some(v_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        let rgb = (v_f as i64).max(0) as u32 & 0x00FF_FFFF;
        CMD_STREAM_CLEAR_RGB.store(rgb, Ordering::Relaxed);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_viewport(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 2) else {
            return qjs::JSValue::undefined();
        };
        let Some(w_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        let Some(h_f) = cmd_stream_arg_f64(ctx, args, 1) else {
            return qjs::JSValue::undefined();
        };
        let w = (w_f as i64).max(1) as u32;
        let h = (h_f as i64).max(1) as u32;
        CMD_STREAM_VIEW_W.store(w, Ordering::Relaxed);
        CMD_STREAM_VIEW_H.store(h, Ordering::Relaxed);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_origin(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 2) else {
            return qjs::JSValue::undefined();
        };
        let Some(x_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        let Some(y_f) = cmd_stream_arg_f64(ctx, args, 1) else {
            return qjs::JSValue::undefined();
        };
        cmd_stream_set_origin_px(x_f as f32, y_f as f32);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_clip_rect(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 4) else {
            return qjs::JSValue::undefined();
        };
        let Some(x_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        let Some(y_f) = cmd_stream_arg_f64(ctx, args, 1) else {
            return qjs::JSValue::undefined();
        };
        let Some(w_f) = cmd_stream_arg_f64(ctx, args, 2) else {
            return qjs::JSValue::undefined();
        };
        let Some(h_f) = cmd_stream_arg_f64(ctx, args, 3) else {
            return qjs::JSValue::undefined();
        };
        cmd_stream_apply_clip_state(cmd_stream_clip_rect_from_local(
            x_f as f32,
            y_f as f32,
            w_f as f32,
            h_f as f32,
        ));
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_push_clip_rect(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 4) else {
            return qjs::JSValue::undefined();
        };
        let Some(x_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        let Some(y_f) = cmd_stream_arg_f64(ctx, args, 1) else {
            return qjs::JSValue::undefined();
        };
        let Some(w_f) = cmd_stream_arg_f64(ctx, args, 2) else {
            return qjs::JSValue::undefined();
        };
        let Some(h_f) = cmd_stream_arg_f64(ctx, args, 3) else {
            return qjs::JSValue::undefined();
        };

        let next_local = cmd_stream_clip_rect_from_local(x_f as f32, y_f as f32, w_f as f32, h_f as f32);
        let prev = *CMD_STREAM_CUR_CLIP.lock();
        CMD_STREAM_CLIP_STACK.lock().push(prev);
        let (view_w, view_h) = cmd_stream_view_size();
        let next = match (prev, next_local) {
            (_, None) => Some(cmd_stream_empty_clip_rect(view_w, view_h)),
            (None, Some(next_rect)) => Some(next_rect),
            (Some(prev_rect), Some(next_rect)) => {
                Some(cmd_stream_intersect_clip_rects(prev_rect, next_rect, view_w, view_h))
            }
        };
        cmd_stream_apply_clip_state(next);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_pop_clip_rect(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let prev = CMD_STREAM_CLIP_STACK.lock().pop().flatten();
        cmd_stream_apply_clip_state(prev);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_clear_clip_rect(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        CMD_STREAM_CLIP_STACK.lock().clear();
        cmd_stream_apply_clip_state(None);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_render_target(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !CMD_STREAM_FRAME_OPEN.load(Ordering::Relaxed) {
            return qjs::JSValue::undefined();
        }
        let Some(args) = cmd_stream_args(argv, argc, 1) else {
            return qjs::JSValue::undefined();
        };
        let Some(tex_id_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        atlas_cmd_stream::flush_text_batches();
        let tex_id = (tex_id_f as i64).max(0) as u32;
        let _ = trueos_cabi_gfx_set_render_target(tex_id);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_clear_render_target(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !CMD_STREAM_FRAME_OPEN.load(Ordering::Relaxed) {
            return qjs::JSValue::undefined();
        }
        atlas_cmd_stream::flush_text_batches();
        let _ = trueos_cabi_gfx_clear_render_target();
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_push_origin(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 2) else {
            return qjs::JSValue::undefined();
        };
        let Some(dx_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        let Some(dy_f) = cmd_stream_arg_f64(ctx, args, 1) else {
            return qjs::JSValue::undefined();
        };
        let current = (
            CMD_STREAM_ORIGIN_X_BITS.load(Ordering::Relaxed),
            CMD_STREAM_ORIGIN_Y_BITS.load(Ordering::Relaxed),
        );
        CMD_STREAM_ORIGIN_STACK.lock().push(current);
        let (cur_x, cur_y) = cmd_stream_origin_px();
        cmd_stream_set_origin_px(cur_x + dx_f as f32, cur_y + dy_f as f32);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_pop_origin(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let prev = CMD_STREAM_ORIGIN_STACK.lock().pop();
        if let Some((x_bits, y_bits)) = prev {
            CMD_STREAM_ORIGIN_X_BITS.store(x_bits, Ordering::Relaxed);
            CMD_STREAM_ORIGIN_Y_BITS.store(y_bits, Ordering::Relaxed);
        } else {
            cmd_stream_set_origin_px(0.0, 0.0);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_blend_enabled(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 1) else {
            return qjs::JSValue::undefined();
        };
        let Some(enabled_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        atlas_cmd_stream::flush_text_batches();
        if enabled_f != 0.0 {
            CMD_STREAM_BLEND_ENABLED.store(1, Ordering::Relaxed);
            let mode = CMD_STREAM_BLEND_MODE.load(Ordering::Relaxed);
            let pma = CMD_STREAM_PMA.load(Ordering::Relaxed) != 0;
            cmd_stream_apply_blend_mode(mode, pma);
        } else {
            CMD_STREAM_BLEND_ENABLED.store(0, Ordering::Relaxed);
            let _ = trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_sampler(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 4) else {
            return qjs::JSValue::undefined();
        };
        let Some(wrap_u) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        let Some(wrap_v) = cmd_stream_arg_f64(ctx, args, 1) else {
            return qjs::JSValue::undefined();
        };
        let Some(min_f) = cmd_stream_arg_f64(ctx, args, 2) else {
            return qjs::JSValue::undefined();
        };
        let Some(mag_f) = cmd_stream_arg_f64(ctx, args, 3) else {
            return qjs::JSValue::undefined();
        };
        atlas_cmd_stream::flush_text_batches();
        let _ = trueos_cabi_gfx_set_sampler(
            (wrap_u as i64).max(0) as u32,
            (wrap_v as i64).max(0) as u32,
            (min_f as i64).max(0) as u32,
            (mag_f as i64).max(0) as u32,
        );
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_blend_mode(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 1) else {
            return qjs::JSValue::undefined();
        };
        let Some(mode_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        atlas_cmd_stream::flush_text_batches();
        let mode = ((mode_f as i64).clamp(0, 3)) as u32;
        CMD_STREAM_BLEND_MODE.store(mode, Ordering::Relaxed);
        if CMD_STREAM_BLEND_ENABLED.load(Ordering::Relaxed) != 0 {
            let pma = CMD_STREAM_PMA.load(Ordering::Relaxed) != 0;
            cmd_stream_apply_blend_mode(mode, pma);
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_cmd_stream_set_premultiplied_alpha(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let Some(args) = cmd_stream_args(argv, argc, 1) else {
            return qjs::JSValue::undefined();
        };
        let Some(pma_f) = cmd_stream_arg_f64(ctx, args, 0) else {
            return qjs::JSValue::undefined();
        };
        atlas_cmd_stream::flush_text_batches();
        CMD_STREAM_PMA.store(if pma_f != 0.0 { 1 } else { 0 }, Ordering::Relaxed);
        if CMD_STREAM_BLEND_ENABLED.load(Ordering::Relaxed) != 0 {
            let mode = CMD_STREAM_BLEND_MODE.load(Ordering::Relaxed);
            cmd_stream_apply_blend_mode(mode, pma_f != 0.0);
        }
        qjs::JSValue::undefined()
    }

    const CMD_STREAM_MODULE_EXPORTS: &[CmdStreamModuleExport] = &[
        (b"beginFrame\0", qjs_cmd_stream_begin_frame, 0),
        (b"endFrame\0", qjs_cmd_stream_end_frame, 0),
        (b"signalLoadscreenEnd\0", qjs_cmd_stream_signal_loadscreen_end, 0),
        (b"setClearRgb\0", qjs_cmd_stream_set_clear_rgb, 1),
        (b"setViewport\0", qjs_cmd_stream_set_viewport, 2),
        (b"setOrigin\0", qjs_cmd_stream_set_origin, 2),
        (b"setClipRect\0", qjs_cmd_stream_set_clip_rect, 4),
        (b"pushClipRect\0", qjs_cmd_stream_push_clip_rect, 4),
        (b"popClipRect\0", qjs_cmd_stream_pop_clip_rect, 0),
        (b"clearClipRect\0", qjs_cmd_stream_clear_clip_rect, 0),
        (b"setRenderTarget\0", qjs_cmd_stream_set_render_target, 1),
        (b"clearRenderTarget\0", qjs_cmd_stream_clear_render_target, 0),
        (b"pushOrigin\0", qjs_cmd_stream_push_origin, 2),
        (b"popOrigin\0", qjs_cmd_stream_pop_origin, 0),
        (b"setBlendEnabled\0", qjs_cmd_stream_set_blend_enabled, 1),
        (b"setSampler\0", qjs_cmd_stream_set_sampler, 4),
        (b"setBlendMode\0", qjs_cmd_stream_set_blend_mode, 1),
        (
            b"setPremultipliedAlpha\0",
            qjs_cmd_stream_set_premultiplied_alpha,
            1,
        ),
        (b"createTextureRgba\0", qjs_cmd_stream_create_texture_rgba, 3),
        (b"createRenderTarget\0", qjs_cmd_stream_create_render_target, 2),
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
        (
            b"createAtlasTexture\0",
            atlas_cmd_stream::qjs_cmd_stream_create_atlas_texture,
            1,
        ),
        (b"drawTrianglesU8\0", draw_cmd_stream::qjs_cmd_stream_draw_triangles_u8, 1),
        (b"fillRect\0", draw_cmd_stream::qjs_cmd_stream_fill_rect, 5),
        (b"drawLine\0", draw_cmd_stream::qjs_cmd_stream_draw_line, 5),
        (
            b"drawTexturedTrianglesU8\0",
            draw_cmd_stream::qjs_cmd_stream_draw_textured_triangles_u8,
            2,
        ),
        (b"drawTextureRect\0", draw_cmd_stream::qjs_cmd_stream_draw_texture_rect, 5),
        (
            b"drawAtlasText\0",
            atlas_cmd_stream::qjs_cmd_stream_draw_atlas_text,
            10,
        ),
        (
            b"drawLyonIconInFrame\0",
            lyon_cmd_stream::qjs_cmd_stream_draw_lyon_icon_in_frame,
            4,
        ),
        (b"stepIconCollisions\0", draw_cmd_stream::qjs_cmd_stream_step_icon_collisions, 5),
    ];

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
    let name = CStr::from_ptr(module_name).to_bytes();
    if name == b"cmd_stream" || name == b"trueos:cmd_stream" {
        let m = qjs::JS_NewCModule(ctx, module_name, Some(qjs_cmd_stream_module_init));
        if m.is_null() {
            return core::ptr::null_mut();
        }
            unsafe { cmd_stream_add_module_exports(ctx, m, CMD_STREAM_MODULE_EXPORTS) };
        return m;
    }

    core::ptr::null_mut()
}
