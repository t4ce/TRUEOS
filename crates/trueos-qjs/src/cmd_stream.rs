#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};
use core::sync::atomic::{AtomicU32, Ordering};
use libm::sqrtf;
use parry2d::math::Isometry;
use parry2d::query;
use parry2d::shape::Ball;
use spin::Mutex;

use crate as qjs;

#[path = "cmd_stream/atlas_cmd_stream.rs"]
mod atlas_cmd_stream;

unsafe extern "C" {
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
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
    fn trueos_cabi_gfx_set_sampler(
        wrap_u: u32,
        wrap_v: u32,
        min_filter: u32,
        mag_filter: u32,
    ) -> i32;
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
static CMD_STREAM_TEX_IDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

struct CmdStreamLyonIconTexRecord {
    icon_id: u32,
    color_id: u32,
    small_set: u32,
    tex_id: u32,
    side_px: u32,
}

struct CmdStreamLyonUnitQuadRecord {
    view_w: u32,
    view_h: u32,
    side_px: u32,
    r: u8,
    g: u8,
    b: u8,
    verts: Arc<[u8]>,
}

static CMD_STREAM_LYON_ICON_TEX_RECS: Mutex<Vec<CmdStreamLyonIconTexRecord>> = Mutex::new(Vec::new());
static CMD_STREAM_LYON_UNIT_QUAD_RECS: Mutex<Vec<CmdStreamLyonUnitQuadRecord>> = Mutex::new(Vec::new());

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
}

#[inline]
fn cmd_stream_alloc_tex_id() -> u32 {
    let id = CMD_STREAM_NEXT_TEX_ID.fetch_add(1, Ordering::AcqRel);
    CMD_STREAM_TEX_IDS.lock().push(id);
    id
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

#[inline]
fn cmd_stream_push_tex_vtx(
    out: &mut Vec<u8>,
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    out.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(&y.to_le_bytes());
    out.extend_from_slice(&u.to_le_bytes());
    out.extend_from_slice(&v.to_le_bytes());
    out.push(r);
    out.push(g);
    out.push(b);
    out.push(a);
}

#[inline]
fn cmd_stream_push_rgb_vtx(out: &mut Vec<u8>, x: f32, y: f32, r: u8, g: u8, b: u8, a: u8) {
    out.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(&y.to_le_bytes());
    out.push(r);
    out.push(g);
    out.push(b);
    out.push(a);
}

#[inline]
fn cmd_stream_draw_rgb_triangles(verts: &[u8], a: u8) -> bool {
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
fn cmd_stream_push_rgb_line_quad_px(
    verts: &mut Vec<u8>,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    thickness: f32,
    vw: f32,
    vh: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = (dx * dx) + (dy * dy);
    if !len_sq.is_finite() || len_sq <= f32::EPSILON {
        return;
    }

    let half = (thickness * 0.5).max(0.5);
    if !half.is_finite() {
        return;
    }

    let inv_len = sqrtf(len_sq).recip();
    let nx = -dy * inv_len;
    let ny = dx * inv_len;
    let ox = nx * half;
    let oy = ny * half;

    let to_ndc = |px: f32, py: f32| -> (f32, f32) {
        ((2.0 * (px / vw)) - 1.0, 1.0 - (2.0 * (py / vh)))
    };

    let (ax, ay) = to_ndc(x1 + ox, y1 + oy);
    let (bx, by) = to_ndc(x2 + ox, y2 + oy);
    let (cx, cy) = to_ndc(x2 - ox, y2 - oy);
    let (dxn, dyn_) = to_ndc(x1 - ox, y1 - oy);

    cmd_stream_push_rgb_vtx(verts, ax, ay, r, g, b, a);
    cmd_stream_push_rgb_vtx(verts, bx, by, r, g, b, a);
    cmd_stream_push_rgb_vtx(verts, cx, cy, r, g, b, a);
    cmd_stream_push_rgb_vtx(verts, ax, ay, r, g, b, a);
    cmd_stream_push_rgb_vtx(verts, cx, cy, r, g, b, a);
    cmd_stream_push_rgb_vtx(verts, dxn, dyn_, r, g, b, a);
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

    let vw = CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1) as f32;
    let vh = CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1) as f32;
    let r = ((rgba >> 24) & 0xFF) as u8;
    let g = ((rgba >> 16) & 0xFF) as u8;
    let b = ((rgba >> 8) & 0xFF) as u8;
    let a = (rgba & 0xFF) as u8;

    let mut push_rect = |verts: &mut Vec<u8>,
                         rect_left_px: f32,
                         rect_top_px: f32,
                         rect_right_px: f32,
                         rect_bottom_px: f32| {
        if !(rect_left_px < rect_right_px && rect_top_px < rect_bottom_px) {
            return;
        }
        let left = (2.0 * (rect_left_px / vw)) - 1.0;
        let right = (2.0 * (rect_right_px / vw)) - 1.0;
        let top = 1.0 - (2.0 * (rect_top_px / vh));
        let bottom = 1.0 - (2.0 * (rect_bottom_px / vh));
        cmd_stream_push_rgb_vtx(verts, left, top, r, g, b, a);
        cmd_stream_push_rgb_vtx(verts, right, top, r, g, b, a);
        cmd_stream_push_rgb_vtx(verts, right, bottom, r, g, b, a);
        cmd_stream_push_rgb_vtx(verts, left, top, r, g, b, a);
        cmd_stream_push_rgb_vtx(verts, right, bottom, r, g, b, a);
        cmd_stream_push_rgb_vtx(verts, left, bottom, r, g, b, a);
    };

    let use_chamfer = chamfer && rect_w >= 10.0 && rect_h >= 10.0;
    let chamfer_px = if use_chamfer { 5.0f32 } else { 0.0f32 };

    let mut push_chamfer_fill = |verts: &mut Vec<u8>| {
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
        let center = ((2.0 * (cx / vw)) - 1.0, 1.0 - (2.0 * (cy / vh)));
        for i in 0..pts.len() {
            let (x1, y1) = pts[i];
            let (x2, y2) = pts[(i + 1) % pts.len()];
            let p1 = ((2.0 * (x1 / vw)) - 1.0, 1.0 - (2.0 * (y1 / vh)));
            let p2 = ((2.0 * (x2 / vw)) - 1.0, 1.0 - (2.0 * (y2 / vh)));
            cmd_stream_push_rgb_vtx(verts, center.0, center.1, r, g, b, a);
            cmd_stream_push_rgb_vtx(verts, p1.0, p1.1, r, g, b, a);
            cmd_stream_push_rgb_vtx(verts, p2.0, p2.1, r, g, b, a);
        }
    };

    let mut push_chamfer_outline = |verts: &mut Vec<u8>| {
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
            cmd_stream_push_rgb_line_quad_px(verts, x1, y1, x2, y2, 1.0, vw, vh, r, g, b, a);
        }
    };

    if use_chamfer {
        let mut verts = Vec::with_capacity(if outline { 48 * 12 } else { 24 * 12 });
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

        let mut verts = Vec::with_capacity(24 * 12);
        push_rect(&mut verts, left_px, top_px, right_px, top_px + top_h);
        push_rect(&mut verts, left_px, bottom_px - bottom_h, right_px, bottom_px);
        push_rect(&mut verts, left_px, inner_top, left_px + left_w, inner_bottom);
        push_rect(&mut verts, right_px - right_w, inner_top, right_px, inner_bottom);
        return cmd_stream_draw_rgb_triangles(&verts, a);
    }

    let mut verts = Vec::with_capacity(6 * 12);

    push_rect(&mut verts, left_px, top_px, right_px, bottom_px);

    cmd_stream_draw_rgb_triangles(&verts, a)
}

pub fn draw_lyon_in_frame(
    icon_id: u32,
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    color_id: u32,
) -> bool {
    CMD_STREAM_VIEW_W.store(view_w.max(1), Ordering::Relaxed);
    CMD_STREAM_VIEW_H.store(view_h.max(1), Ordering::Relaxed);

    // Preserve in-frame ordering with queued atlas text, but do not mutate global
    // frame state here (blend/sampler), since callers may have configured it.
    let Some((tex_id, side_px)) = cmd_stream_ensure_lyon_icon_tex(icon_id, 0, 1) else {
        return false;
    };
    let (r, g, b) = cmd_stream_lyon_palette_rgb(color_id);

    let view_w_u = view_w.max(1);
    let view_h_u = view_h.max(1);
    let view_w_f = view_w_u as f32;
    let view_h_f = view_h_u as f32;
    let origin_x_ndc = (2.0 * (x / view_w_f)) - 1.0;
    let origin_y_ndc = 1.0 - (2.0 * (y / view_h_f));

    let verts = cmd_stream_get_lyon_unit_quad_verts(view_w_u, view_h_u, side_px, r, g, b);
    atlas_cmd_stream::enqueue_text_batch(tex_id, verts.as_ref(), origin_x_ndc, origin_y_ndc);
    true
}

#[inline]
fn cmd_stream_lyon_palette_rgb(color_id: u32) -> (u8, u8, u8) {
    match (color_id % 5) as usize {
        0 => (0, 0, 0),
        1 => (217, 46, 46),
        2 => (31, 158, 56),
        3 => (31, 82, 217),
        _ => (242, 140, 31),
    }
}

#[inline]
fn cmd_stream_ensure_lyon_icon_tex(icon_id: u32, color_id: u32, small_set: u32) -> Option<(u32, u32)> {
    {
        let recs = CMD_STREAM_LYON_ICON_TEX_RECS.lock();
        if let Some(rec) = recs
            .iter()
            .find(|r| r.icon_id == icon_id && r.color_id == color_id && r.small_set == small_set)
        {
            return Some((rec.tex_id, rec.side_px));
        }
    }

    let need = unsafe {
        trueos_cabi_gfx_bake_lyon_icon_rgba(icon_id, color_id, small_set, core::ptr::null_mut(), 0)
    };
    if need <= 0 {
        return None;
    }
    let need = need as usize;
    if need % 4 != 0 {
        return None;
    }
    let px_count = need / 4;
    let side = if small_set != 0 { 16usize } else { 32usize };
    if side.saturating_mul(side) != px_count {
        return None;
    }

    let mut rgba = vec![0u8; need];
    let wrote = unsafe {
        trueos_cabi_gfx_bake_lyon_icon_rgba(icon_id, color_id, small_set, rgba.as_mut_ptr(), rgba.len())
    };
    if wrote != need as i32 {
        return None;
    }

    let tex_id = cmd_stream_alloc_tex_id();
    let rc = unsafe {
        trueos_cabi_gfx_upload_texture_rgba(
            tex_id,
            side as u32,
            side as u32,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    if rc != 0 {
        cmd_stream_release_tex_id(tex_id);
        return None;
    }

    CMD_STREAM_LYON_ICON_TEX_RECS.lock().push(CmdStreamLyonIconTexRecord {
        icon_id,
        color_id,
        small_set,
        tex_id,
        side_px: side as u32,
    });
    Some((tex_id, side as u32))
}

#[inline]
fn cmd_stream_get_lyon_unit_quad_verts(
    view_w: u32,
    view_h: u32,
    side_px: u32,
    r: u8,
    g: u8,
    b: u8,
) -> Arc<[u8]> {
    {
        let recs = CMD_STREAM_LYON_UNIT_QUAD_RECS.lock();
        if let Some(rec) = recs.iter().find(|rec| {
            rec.view_w == view_w
                && rec.view_h == view_h
                && rec.side_px == side_px
                && rec.r == r
                && rec.g == g
                && rec.b == b
        }) {
            return rec.verts.clone();
        }
    }

    let vw = view_w.max(1) as f32;
    let vh = view_h.max(1) as f32;
    let side = side_px.max(1) as f32;
    let dx = 2.0 * (side / vw);
    let dy = 2.0 * (side / vh);

    let mut verts = Vec::with_capacity(6 * 20);
    // Local quad in NDC delta-space with top-left origin at (0, 0).
    cmd_stream_push_tex_vtx(&mut verts, 0.0, -dy, 0.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, dx, -dy, 1.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, dx, 0.0, 1.0, 0.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, 0.0, -dy, 0.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, dx, 0.0, 1.0, 0.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, 0.0, 0.0, 0.0, 0.0, r, g, b, 255);

    let out: Arc<[u8]> = Arc::from(verts.into_boxed_slice());
    let mut recs = CMD_STREAM_LYON_UNIT_QUAD_RECS.lock();
    recs.push(CmdStreamLyonUnitQuadRecord {
        view_w,
        view_h,
        side_px,
        r,
        g,
        b,
        verts: out.clone(),
    });
    if recs.len() > 64 {
        let excess = recs.len() - 64;
        recs.drain(0..excess);
    }
    out
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
        unsafe extern "C" fn qjs_cmd_stream_begin_frame(
            _ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: i32,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            atlas_cmd_stream::clear_text_batches();
            let clear = CMD_STREAM_CLEAR_RGB.load(Ordering::Relaxed);
            let _ = trueos_cabi_gfx_begin_frame(clear);
            cmd_stream_reset_frame_state_defaults();
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_end_frame(
            _ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: i32,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            atlas_cmd_stream::flush_text_batches();
            let _ = trueos_cabi_gfx_end_frame();
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
                // disabled
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

        unsafe extern "C" fn qjs_cmd_stream_draw_triangles_u8(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            let Some(args) = cmd_stream_args(argv, argc, 1) else {
                return qjs::JSValue::undefined();
            };
            atlas_cmd_stream::flush_text_batches();
            let _ = cmd_stream_with_u8_buffer(ctx, args[0], |ptr, len| {
                if len > 0 {
                    let _ = trueos_cabi_gfx_draw_rgb_triangles_no_present(ptr, len);
                }
            });
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_fill_rect(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            let Some(args) = cmd_stream_args(argv, argc, 5) else {
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
            let Some(rgba_f) = cmd_stream_arg_f64(ctx, args, 4) else {
                return qjs::JSValue::undefined();
            };

            atlas_cmd_stream::flush_text_batches();
            let rgba = (rgba_f as i64).max(0) as u32;
            let outline = cmd_stream_arg_f64(ctx, args, 5).unwrap_or(0.0) != 0.0;
            let chamfer = cmd_stream_arg_f64(ctx, args, 6).unwrap_or(0.0) != 0.0;
            let _ = cmd_stream_fill_rect(
                x_f as f32,
                y_f as f32,
                w_f as f32,
                h_f as f32,
                rgba,
                outline,
                chamfer,
            );
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_draw_textured_triangles_u8(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            let Some(args) = cmd_stream_args(argv, argc, 2) else {
                return qjs::JSValue::undefined();
            };
            atlas_cmd_stream::flush_text_batches();
            let Some(tex_id_f) = cmd_stream_arg_f64(ctx, args, 0) else {
                return qjs::JSValue::undefined();
            };
            let tex_id = (tex_id_f as i64).max(0) as u32;
            if tex_id == 0 {
                return qjs::JSValue::undefined();
            }
            let _ = cmd_stream_with_u8_buffer(ctx, args[1], |ptr, len| {
                if len > 0 {
                    let _ = trueos_cabi_gfx_draw_tex_triangles_no_present(tex_id, ptr, len);
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

        unsafe extern "C" fn qjs_cmd_stream_draw_lyon_icon_in_frame(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            let Some(args) = cmd_stream_args(argv, argc, 3) else {
                return qjs::JSValue::undefined();
            };
            let Some(icon_id_f) = cmd_stream_arg_f64(ctx, args, 0) else {
                return qjs::JSValue::undefined();
            };
            let Some(x_f) = cmd_stream_arg_f64(ctx, args, 1) else {
                return qjs::JSValue::undefined();
            };
            let Some(y_f) = cmd_stream_arg_f64(ctx, args, 2) else {
                return qjs::JSValue::undefined();
            };

            let color_id_f = cmd_stream_arg_f64(ctx, args, 3).unwrap_or(0.0);

            let icon_id = (icon_id_f as i64).max(0) as u32;
            let color_id = (color_id_f as i64).max(0) as u32;
            let view_w = CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1);
            let view_h = CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1);

            let _ = draw_lyon_in_frame(
                icon_id,
                x_f as f32,
                y_f as f32,
                view_w,
                view_h,
                color_id,
            );
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_step_icon_collisions(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            let Some(args) = cmd_stream_args(argv, argc, 5) else {
                return qjs::JS_NewFloat64(ctx, 0.0);
            };

            let Some((pos_ptr, pos_len, pos_ab)) = cmd_stream_read_f32_slice_from_value(ctx, args[0]) else {
                return qjs::JS_NewFloat64(ctx, 0.0);
            };
            let Some((vel_ptr, vel_len, vel_ab)) = cmd_stream_read_f32_slice_from_value(ctx, args[1]) else {
                qjs::js_free_value(ctx, pos_ab);
                return qjs::JS_NewFloat64(ctx, 0.0);
            };

            let Some(dt_ms_f) = cmd_stream_arg_f64(ctx, args, 2) else {
                qjs::js_free_value(ctx, vel_ab);
                qjs::js_free_value(ctx, pos_ab);
                return qjs::JS_NewFloat64(ctx, 0.0);
            };
            let Some(icon_size_f) = cmd_stream_arg_f64(ctx, args, 3) else {
                qjs::js_free_value(ctx, vel_ab);
                qjs::js_free_value(ctx, pos_ab);
                return qjs::JS_NewFloat64(ctx, 0.0);
            };
            let Some(restitution_f) = cmd_stream_arg_f64(ctx, args, 4) else {
                qjs::js_free_value(ctx, vel_ab);
                qjs::js_free_value(ctx, pos_ab);
                return qjs::JS_NewFloat64(ctx, 0.0);
            };

            let icon_size = (icon_size_f as f32).max(1.0);
            let radius = (icon_size * 0.5).max(0.5);
            let dt = ((dt_ms_f as f32) / 1000.0).clamp(0.0, 0.05);
            let restitution = (restitution_f as f32).clamp(0.0, 1.0);
            let n = core::cmp::min(pos_len, vel_len) / 2;
            if n < 2 {
                qjs::js_free_value(ctx, vel_ab);
                qjs::js_free_value(ctx, pos_ab);
                return qjs::JS_NewFloat64(ctx, 0.0);
            }

            let pos = core::slice::from_raw_parts_mut(pos_ptr, n * 2);
            let vel = core::slice::from_raw_parts_mut(vel_ptr, n * 2);
            let view_w = CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1) as f32;
            let view_h = CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1) as f32;

            for i in 0..n {
                let b = i * 2;
                pos[b] += vel[b] * dt;
                pos[b + 1] += vel[b + 1] * dt;

                if pos[b] < 0.0 {
                    pos[b] = 0.0;
                    vel[b] = vel[b].abs() * restitution;
                } else if pos[b] + icon_size > view_w {
                    pos[b] = (view_w - icon_size).max(0.0);
                    vel[b] = -vel[b].abs() * restitution;
                }

                if pos[b + 1] < 0.0 {
                    pos[b + 1] = 0.0;
                    vel[b + 1] = vel[b + 1].abs() * restitution;
                } else if pos[b + 1] + icon_size > view_h {
                    pos[b + 1] = (view_h - icon_size).max(0.0);
                    vel[b + 1] = -vel[b + 1].abs() * restitution;
                }
            }

            let shape = Ball::new(radius);
            let mut contacts = 0u32;
            for i in 0..n {
                let ib = i * 2;
                let ci_x = pos[ib] + radius;
                let ci_y = pos[ib + 1] + radius;
                let pi = Isometry::translation(ci_x, ci_y);

                for j in (i + 1)..n {
                    let jb = j * 2;
                    let cj_x = pos[jb] + radius;
                    let cj_y = pos[jb + 1] + radius;
                    let pj = Isometry::translation(cj_x, cj_y);

                    let Ok(Some(c)) = query::contact(&pi, &shape, &pj, &shape, 0.0) else {
                        continue;
                    };

                    if c.dist >= 0.0 {
                        continue;
                    }
                    contacts = contacts.saturating_add(1);

                    let nrm = c.normal1.into_inner();
                    let nx = nrm.x;
                    let ny = nrm.y;

                    let rvx = vel[jb] - vel[ib];
                    let rvy = vel[jb + 1] - vel[ib + 1];
                    let rel = (rvx * nx) + (rvy * ny);
                    if rel < 0.0 {
                        let impulse = -((1.0 + restitution) * rel) * 0.5;
                        vel[ib] -= impulse * nx;
                        vel[ib + 1] -= impulse * ny;
                        vel[jb] += impulse * nx;
                        vel[jb + 1] += impulse * ny;
                    }

                    let penetration = (-c.dist).max(0.0);
                    if penetration > 0.0 {
                        let corr = (penetration * 0.5) + 0.01;
                        pos[ib] -= corr * nx;
                        pos[ib + 1] -= corr * ny;
                        pos[jb] += corr * nx;
                        pos[jb + 1] += corr * ny;
                    }
                }
            }

            qjs::js_free_value(ctx, vel_ab);
            qjs::js_free_value(ctx, pos_ab);
            qjs::JS_NewFloat64(ctx, contacts as f64)
        }

        unsafe extern "C" fn qjs_cmd_stream_module_init(
            ctx: *mut qjs::JSContext,
            m: *mut qjs::JSModuleDef,
        ) -> i32 {
            macro_rules! export_fn {
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
                    let _ = qjs::JS_SetModuleExport(ctx, m, k.as_ptr() as *const c_char, f);
                }};
            }
            export_fn!("beginFrame", qjs_cmd_stream_begin_frame, 0);
            export_fn!("endFrame", qjs_cmd_stream_end_frame, 0);
            export_fn!("setClearRgb", qjs_cmd_stream_set_clear_rgb, 1);
            export_fn!("setViewport", qjs_cmd_stream_set_viewport, 2);
            export_fn!("setBlendEnabled", qjs_cmd_stream_set_blend_enabled, 1);
            export_fn!("setSampler", qjs_cmd_stream_set_sampler, 4);
            export_fn!("setBlendMode", qjs_cmd_stream_set_blend_mode, 1);
            export_fn!(
                "setPremultipliedAlpha",
                qjs_cmd_stream_set_premultiplied_alpha,
                1
            );
            export_fn!("createTextureRgba", qjs_cmd_stream_create_texture_rgba, 3);
            export_fn!("updateTextureRgba", qjs_cmd_stream_update_texture_rgba, 4);
            export_fn!("destroyTexture", qjs_cmd_stream_destroy_texture, 1);
            export_fn!(
                "createAtlasTexture",
                atlas_cmd_stream::qjs_cmd_stream_create_atlas_texture,
                1
            );
            export_fn!("drawTrianglesU8", qjs_cmd_stream_draw_triangles_u8, 1);
            export_fn!("fillRect", qjs_cmd_stream_fill_rect, 5);
            export_fn!(
                "drawTexturedTrianglesU8",
                qjs_cmd_stream_draw_textured_triangles_u8,
                2
            );
            export_fn!("drawAtlasText", atlas_cmd_stream::qjs_cmd_stream_draw_atlas_text, 10);
            export_fn!(
                "drawLyonIconInFrame",
                qjs_cmd_stream_draw_lyon_icon_in_frame,
                4
            );
            export_fn!(
                "stepIconCollisions",
                qjs_cmd_stream_step_icon_collisions,
                5
            );
            0
        }

        let m = qjs::JS_NewCModule(ctx, module_name, Some(qjs_cmd_stream_module_init));
        if m.is_null() {
            return core::ptr::null_mut();
        }
        macro_rules! add_export {
            ($name:literal) => {{
                let k = concat!($name, "\0");
                let _ = qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char);
            }};
        }
        add_export!("beginFrame");
        add_export!("endFrame");
        add_export!("setClearRgb");
        add_export!("setViewport");
        add_export!("setBlendEnabled");
        add_export!("setSampler");
        add_export!("setBlendMode");
        add_export!("setPremultipliedAlpha");
        add_export!("createTextureRgba");
        add_export!("updateTextureRgba");
        add_export!("destroyTexture");
        add_export!("createAtlasTexture");
        add_export!("drawTrianglesU8");
        add_export!("fillRect");
        add_export!("drawTexturedTrianglesU8");
        add_export!("drawAtlasText");
        add_export!("drawLyonIconInFrame");
        add_export!("stepIconCollisions");
        return m;
    }

    core::ptr::null_mut()
}
