#![cfg(feature = "trueos")]
use core::ffi::CStr;
use core::ffi::c_char;
use alloc::vec::Vec;

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
    fn trueos_cabi_gfx_draw_rgb_triangles_no_present(vtx_ptr: *const u8, vtx_len: usize) -> i32;
}

#[inline]
fn to_ndc_x(x: f32, viewport_w: f32) -> f32 {
    ((x / viewport_w) * 2.0) - 1.0
}

#[inline]
fn to_ndc_y(y: f32, viewport_h: f32) -> f32 {
    1.0 - ((y / viewport_h) * 2.0)
}

#[inline]
fn push_rgb_vtx(out: &mut Vec<u8>, x_px: f32, y_px: f32, rgba: (u8, u8, u8, u8), vw: f32, vh: f32) {
    let x = to_ndc_x(x_px, vw);
    let y = to_ndc_y(y_px, vh);
    out.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(&y.to_le_bytes());
    out.push(rgba.0);
    out.push(rgba.1);
    out.push(rgba.2);
    out.push(rgba.3);
}

#[inline]
unsafe fn js_num_at(ctx: *mut qjs::JSContext, arr: qjs::JSValueConst, idx: u32) -> Option<f64> {
    let v = qjs::JS_GetPropertyUint32(ctx, arr, idx);
    let mut out = 0.0f64;
    let ok = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, v) == 0;
    qjs::js_free_value(ctx, v);
    if ok { Some(out) } else { None }
}

unsafe fn draw_lyon_runs_in_frame(
    ctx: *mut qjs::JSContext,
    runs: qjs::JSValueConst,
    view_w: u32,
    view_h: u32,
) {
    static LENGTH_NAME: &[u8] = b"length\0";

    let len_val = qjs::JS_GetPropertyStr(ctx, runs, LENGTH_NAME.as_ptr() as *const c_char);
    let mut len_f = 0.0f64;
    let _ = qjs::JS_ToFloat64(ctx, &mut len_f as *mut f64, len_val);
    qjs::js_free_value(ctx, len_val);
    let runs_len = if len_f.is_finite() && len_f > 0.0 {
        len_f as u32
    } else {
        0
    };
    if runs_len == 0 {
        return;
    }

    let vw = view_w.max(1) as f32;
    let vh = view_h.max(1) as f32;
    let mut p = 0u32;

    while p + 8 < runs_len {
        let Some(ox_f) = js_num_at(ctx, runs, p) else {
            break;
        };
        let Some(oy_f) = js_num_at(ctx, runs, p + 1) else {
            break;
        };
        let Some(scale_f) = js_num_at(ctx, runs, p + 2) else {
            break;
        };
        let Some(r_f) = js_num_at(ctx, runs, p + 3) else {
            break;
        };
        let Some(g_f) = js_num_at(ctx, runs, p + 4) else {
            break;
        };
        let Some(b_f) = js_num_at(ctx, runs, p + 5) else {
            break;
        };
        let Some(a_f) = js_num_at(ctx, runs, p + 6) else {
            break;
        };
        let Some(vcount_f) = js_num_at(ctx, runs, p + 7) else {
            break;
        };
        let Some(icount_f) = js_num_at(ctx, runs, p + 8) else {
            break;
        };

        let vcount = if vcount_f.is_finite() && vcount_f > 0.0 {
            vcount_f as u32
        } else {
            0
        };
        let icount = if icount_f.is_finite() && icount_f > 0.0 {
            icount_f as u32
        } else {
            0
        };

        let header = 9u32;
        let vspan = vcount.saturating_mul(2);
        let payload = header.saturating_add(vspan).saturating_add(icount);
        if payload == 0 || p.saturating_add(payload) > runs_len {
            break;
        }

        let ox = ox_f as f32;
        let oy = oy_f as f32;
        let scale = if scale_f.is_finite() {
            (scale_f as f32).max(0.01)
        } else {
            1.0
        };
        let rgba = (
            (r_f as i32).clamp(0, 255) as u8,
            (g_f as i32).clamp(0, 255) as u8,
            (b_f as i32).clamp(0, 255) as u8,
            (a_f as i32).clamp(0, 255) as u8,
        );

        let verts_base = p + header;
        let idx_base = verts_base + vspan;
        let mut mesh = Vec::with_capacity((icount as usize / 3).saturating_mul(3 * 12));

        let mut ii = 0u32;
        while ii + 2 < icount {
            let i0 = js_num_at(ctx, runs, idx_base + ii).unwrap_or(-1.0) as i32;
            let i1 = js_num_at(ctx, runs, idx_base + ii + 1).unwrap_or(-1.0) as i32;
            let i2 = js_num_at(ctx, runs, idx_base + ii + 2).unwrap_or(-1.0) as i32;

            if i0 >= 0 && i1 >= 0 && i2 >= 0 {
                let vi0 = i0 as u32;
                let vi1 = i1 as u32;
                let vi2 = i2 as u32;
                if vi0 < vcount && vi1 < vcount && vi2 < vcount {
                    let x0 = js_num_at(ctx, runs, verts_base + vi0.saturating_mul(2)).unwrap_or(0.0) as f32;
                    let y0 = js_num_at(ctx, runs, verts_base + vi0.saturating_mul(2) + 1).unwrap_or(0.0) as f32;
                    let x1 = js_num_at(ctx, runs, verts_base + vi1.saturating_mul(2)).unwrap_or(0.0) as f32;
                    let y1 = js_num_at(ctx, runs, verts_base + vi1.saturating_mul(2) + 1).unwrap_or(0.0) as f32;
                    let x2 = js_num_at(ctx, runs, verts_base + vi2.saturating_mul(2)).unwrap_or(0.0) as f32;
                    let y2 = js_num_at(ctx, runs, verts_base + vi2.saturating_mul(2) + 1).unwrap_or(0.0) as f32;

                    push_rgb_vtx(&mut mesh, ox + x0 * scale, oy + y0 * scale, rgba, vw, vh);
                    push_rgb_vtx(&mut mesh, ox + x1 * scale, oy + y1 * scale, rgba, vw, vh);
                    push_rgb_vtx(&mut mesh, ox + x2 * scale, oy + y2 * scale, rgba, vw, vh);
                }
            }

            ii += 3;
        }

        if !mesh.is_empty() {
            let _ = trueos_cabi_gfx_draw_rgb_triangles_no_present(mesh.as_ptr(), mesh.len());
        }

        p = p.saturating_add(payload);
    }
}

unsafe extern "C" fn draw_html(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 4 || argv.is_null() {
        return qjs::JSValue::undefined();
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);

    let mut view_w_f = 0.0f64;
    let mut view_h_f = 0.0f64;
    let _ = qjs::JS_ToFloat64(ctx, &mut view_w_f as *mut f64, args[1]);
    let _ = qjs::JS_ToFloat64(ctx, &mut view_h_f as *mut f64, args[2]);
    let view_w = core::cmp::max(1, view_w_f as i32) as u32;
    let view_h = core::cmp::max(1, view_h_f as i32) as u32;

    static LENGTH_NAME: &[u8] = b"length\0";
    let text_runs = args[3];
    let len_val = qjs::JS_GetPropertyStr(ctx, text_runs, LENGTH_NAME.as_ptr() as *const c_char);
    let mut runs_len_f = 0.0f64;
    let _ = qjs::JS_ToFloat64(ctx, &mut runs_len_f as *mut f64, len_val);
    qjs::js_free_value(ctx, len_val);
    let runs_len = if runs_len_f.is_finite() && runs_len_f > 0.0 {
        runs_len_f as u32
    } else {
        0
    };

    let _ = trueos_cabi_gfx_begin_frame(0xF4F4F4);

    let mut i = 0u32;
    while i + 2 < runs_len {
        let x_val = qjs::JS_GetPropertyUint32(ctx, text_runs, i);
        let y_val = qjs::JS_GetPropertyUint32(ctx, text_runs, i + 1);
        let t_val = qjs::JS_GetPropertyUint32(ctx, text_runs, i + 2);

        let mut x_f = 0.0f64;
        let mut y_f = 0.0f64;
        let _ = qjs::JS_ToFloat64(ctx, &mut x_f as *mut f64, x_val);
        let _ = qjs::JS_ToFloat64(ctx, &mut y_f as *mut f64, y_val);

        let t_ptr = qjs::js_to_cstring(ctx, t_val);
        if !t_ptr.is_null() {
            let text = CStr::from_ptr(t_ptr).to_bytes();
            if !text.is_empty() {
                let _ = qjs::cmd_stream::draw_text_widget_in_frame(
                    text,
                    x_f as f32,
                    y_f as f32,
                    view_w,
                    view_h,
                );
            }
            qjs::JS_FreeCString(ctx, t_ptr);
        }

        qjs::js_free_value(ctx, x_val);
        qjs::js_free_value(ctx, y_val);
        qjs::js_free_value(ctx, t_val);
        i += 3;
    }

    if argc >= 5 {
        let lyon_runs = args[4];
        draw_lyon_runs_in_frame(ctx, lyon_runs, view_w, view_h);
    }

    let _ = trueos_cabi_gfx_end_frame();
    qjs::JSValue::undefined()
}

pub unsafe fn install_scene_api(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    static NAME: &[u8] = b"__trueosDrawLayoutRects\0";
    static FN_NAME: &[u8] = b"__trueosDrawLayoutRects\0";

    let global = qjs::JS_GetGlobalObject(ctx);
    let draw_func = qjs::JS_NewCFunction2(
        ctx,
        Some(draw_html),
        FN_NAME.as_ptr() as *const c_char,
        5,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, NAME.as_ptr() as *const c_char, draw_func);
    qjs::js_free_value(ctx, global);
}
