#![cfg(feature = "trueos")]

use alloc::vec::Vec;
use core::ffi::c_char;

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_draw_rgb_triangles_no_present(vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
    fn trueos_cabi_gfx_set_scissor(x: u32, y: u32, width: u32, height: u32) -> i32;
    fn trueos_cabi_gfx_clear_scissor() -> i32;
}

#[inline]
fn intersect_rect(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> Option<(f32, f32, f32, f32)> {
    let x0 = a.0.max(b.0);
    let y0 = a.1.max(b.1);
    let x1 = a.2.min(b.2);
    let y1 = a.3.min(b.3);
    if x1 > x0 && y1 > y0 {
        Some((x0, y0, x1, y1))
    } else {
        None
    }
}

#[inline]
fn clamp_u8(v: i32) -> u8 {
    if v <= 0 {
        0
    } else if v >= 255 {
        255
    } else {
        v as u8
    }
}

#[inline]
fn push_vtx(out: &mut Vec<u8>, x_ndc: f32, y_ndc: f32, r: u8, g: u8, b: u8, a: u8) {
    out.extend_from_slice(&x_ndc.to_le_bytes());
    out.extend_from_slice(&y_ndc.to_le_bytes());
    out.push(r);
    out.push(g);
    out.push(b);
    out.push(a);
}

#[inline]
fn to_ndc_x(x: f32, viewport_w: f32) -> f32 {
    ((x / viewport_w) * 2.0) - 1.0
}

#[inline]
fn to_ndc_y(y: f32, viewport_h: f32) -> f32 {
    1.0 - ((y / viewport_h) * 2.0)
}

fn push_rect(
    out: &mut Vec<u8>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
    viewport_w: f32,
    viewport_h: f32,
) {
    let lx = x0.min(x1);
    let rx = x0.max(x1);
    let ty = y0.min(y1);
    let by = y0.max(y1);
    if rx <= lx || by <= ty {
        return;
    }

    let ax = to_ndc_x(lx, viewport_w);
    let ay = to_ndc_y(by, viewport_h);
    let bx = to_ndc_x(rx, viewport_w);
    let by2 = to_ndc_y(by, viewport_h);
    let cx = to_ndc_x(rx, viewport_w);
    let cy = to_ndc_y(ty, viewport_h);
    let dx = to_ndc_x(lx, viewport_w);
    let dy = to_ndc_y(ty, viewport_h);

    push_vtx(out, ax, ay, r, g, b, a);
    push_vtx(out, bx, by2, r, g, b, a);
    push_vtx(out, cx, cy, r, g, b, a);

    push_vtx(out, ax, ay, r, g, b, a);
    push_vtx(out, cx, cy, r, g, b, a);
    push_vtx(out, dx, dy, r, g, b, a);
}

fn push_border_rect(
    out: &mut Vec<u8>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    bw: f32,
    viewport_w: f32,
    viewport_h: f32,
) {
    if w <= 1.0 || h <= 1.0 {
        return;
    }
    let r = 0u8;
    let g = 0u8;
    let b = 0u8;
    let a = 255u8;

    let x0 = x;
    let y0 = y;
    let x1 = x + w;
    let y1 = y + h;

    push_rect(out, x0, y0, x1, y0 + bw, r, g, b, a, viewport_w, viewport_h);
    push_rect(out, x0, y1 - bw, x1, y1, r, g, b, a, viewport_w, viewport_h);
    push_rect(out, x0, y0, x0 + bw, y1, r, g, b, a, viewport_w, viewport_h);
    push_rect(out, x1 - bw, y0, x1, y1, r, g, b, a, viewport_w, viewport_h);
}

unsafe extern "C" fn qjs_draw_layout_rects(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 3 || argv.is_null() {
        return qjs::JSValue::undefined();
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let rects = args[0];

    let mut vw = 0.0f64;
    let mut vh = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut vw as *mut f64, args[1]) != 0 || qjs::JS_ToFloat64(ctx, &mut vh as *mut f64, args[2]) != 0 {
        return qjs::JSValue::undefined();
    }

    let viewport_w = (vw as f32).max(1.0);
    let viewport_h = (vh as f32).max(1.0);

    static LENGTH_KEY: &[u8] = b"length\0";
    let len_val = qjs::JS_GetPropertyStr(ctx, rects, LENGTH_KEY.as_ptr() as *const c_char);
    let mut len_f = 0.0f64;
    let _ = qjs::JS_ToFloat64(ctx, &mut len_f as *mut f64, len_val);
    qjs::js_free_value(ctx, len_val);
    let len = if len_f.is_finite() && len_f > 0.0 {
        len_f as u32
    } else {
        0
    };

    let mut bytes = Vec::with_capacity(12 * 6);
    let mut clip_stack: Vec<(u32, (f32, f32, f32, f32))> = Vec::new();

    let _ = trueos_cabi_gfx_begin_frame(0xFFFFFF);
    let mut i = 0u32;
    while i + 5 < len {
        let vx = qjs::JS_GetPropertyUint32(ctx, rects, i + 0);
        let vy = qjs::JS_GetPropertyUint32(ctx, rects, i + 1);
        let vw2 = qjs::JS_GetPropertyUint32(ctx, rects, i + 2);
        let vh2 = qjs::JS_GetPropertyUint32(ctx, rects, i + 3);
        let vd = qjs::JS_GetPropertyUint32(ctx, rects, i + 4);
        let vs = qjs::JS_GetPropertyUint32(ctx, rects, i + 5);

        let mut x = 0.0f64;
        let mut y = 0.0f64;
        let mut w = 0.0f64;
        let mut h = 0.0f64;
        let mut depth_f = 0.0f64;
        let mut scrollable_f = 0.0f64;
        let _ = qjs::JS_ToFloat64(ctx, &mut x as *mut f64, vx);
        let _ = qjs::JS_ToFloat64(ctx, &mut y as *mut f64, vy);
        let _ = qjs::JS_ToFloat64(ctx, &mut w as *mut f64, vw2);
        let _ = qjs::JS_ToFloat64(ctx, &mut h as *mut f64, vh2);
        let _ = qjs::JS_ToFloat64(ctx, &mut depth_f as *mut f64, vd);
        let _ = qjs::JS_ToFloat64(ctx, &mut scrollable_f as *mut f64, vs);

        qjs::js_free_value(ctx, vx);
        qjs::js_free_value(ctx, vy);
        qjs::js_free_value(ctx, vw2);
        qjs::js_free_value(ctx, vh2);
        qjs::js_free_value(ctx, vd);
        qjs::js_free_value(ctx, vs);

        let xf = x as f32;
        let yf = y as f32;
        let wf = (w as f32).max(0.0);
        let hf = (h as f32).max(0.0);
        let depth = if depth_f.is_finite() && depth_f >= 0.0 {
            depth_f as u32
        } else {
            0
        };
        let is_scrollable = scrollable_f.is_finite() && scrollable_f >= 0.5;

        while let Some((d, _)) = clip_stack.last().copied() {
            if d >= depth {
                clip_stack.pop();
            } else {
                break;
            }
        }

        let parent_clip = clip_stack.last().map(|(_, r)| *r);
        if let Some((cx0, cy0, cx1, cy1)) = parent_clip {
            let sx = cx0.max(0.0) as u32;
            let sy = cy0.max(0.0) as u32;
            let sw = (cx1 - cx0).max(0.0) as u32;
            let sh = (cy1 - cy0).max(0.0) as u32;
            let _ = trueos_cabi_gfx_set_scissor(sx, sy, sw, sh);
        } else {
            let _ = trueos_cabi_gfx_clear_scissor();
        }

        bytes.clear();
        push_border_rect(&mut bytes, xf, yf, wf, hf, 1.0, viewport_w, viewport_h);
        if !bytes.is_empty() {
            let _ = trueos_cabi_gfx_draw_rgb_triangles_no_present(bytes.as_ptr(), bytes.len());
        }

        if is_scrollable {
            let self_clip = (xf, yf, xf + wf, yf + hf);
            let effective = match parent_clip {
                Some(p) => intersect_rect(p, self_clip),
                None => Some(self_clip),
            };
            if let Some(rect) = effective {
                clip_stack.push((depth, rect));
            }
        }

        i += 6;
    }

    let _ = trueos_cabi_gfx_clear_scissor();
    let _ = trueos_cabi_gfx_end_frame();

    qjs::JSValue::undefined()
}

pub unsafe fn install_layout_api(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    static NAME: &[u8] = b"__trueosDrawLayoutRects\0";
    static FN_NAME: &[u8] = b"__trueosDrawLayoutRects\0";

    let global = qjs::JS_GetGlobalObject(ctx);
    let func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_draw_layout_rects),
        FN_NAME.as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, NAME.as_ptr() as *const c_char, func);
    qjs::js_free_value(ctx, global);
}
