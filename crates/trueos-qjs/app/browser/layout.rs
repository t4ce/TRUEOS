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
    fn trueos_cabi_input_cursor_pos(cursor_id: u32, out_x: *mut i32, out_y: *mut i32) -> i32;
    fn trueos_cabi_input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32;
    fn trueos_cabi_input_read_cursor_events_since(
        read_seq: u64,
        out: *mut qjs::trueos_shims::TrueosHidCursorEvent,
        out_cap: u32,
        out_next_seq: *mut u64,
        out_dropped: *mut u32,
    ) -> u32;
}

#[inline]
fn window_icon_kind_from_f64(v: f64) -> qjs::svg::WindowIconKind {
    let n = if v.is_finite() { v as i32 } else { 0 };
    match n {
        1 => qjs::svg::WindowIconKind::Minimize,
        2 => qjs::svg::WindowIconKind::Maximize,
        3 => qjs::svg::WindowIconKind::ArrowLeft,
        4 => qjs::svg::WindowIconKind::ArrowRight,
        5 => qjs::svg::WindowIconKind::ArrowUp,
        6 => qjs::svg::WindowIconKind::ArrowDown,
        7 => qjs::svg::WindowIconKind::RadioSelected,
        _ => qjs::svg::WindowIconKind::Close,
    }
}

#[inline]
fn intersect_rect(
    a: (f32, f32, f32, f32),
    b: (f32, f32, f32, f32),
) -> Option<(f32, f32, f32, f32)> {
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
    push_border_rect_rgba(out, x, y, w, h, bw, 0, 0, 0, 255, viewport_w, viewport_h);
}

fn push_border_rect_rgba(
    out: &mut Vec<u8>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    bw: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
    viewport_w: f32,
    viewport_h: f32,
) {
    if w <= 1.0 || h <= 1.0 {
        return;
    }

    let x0 = x;
    let y0 = y;
    let x1 = x + w;
    let y1 = y + h;

    push_rect(out, x0, y0, x1, y0 + bw, r, g, b, a, viewport_w, viewport_h);
    push_rect(out, x0, y1 - bw, x1, y1, r, g, b, a, viewport_w, viewport_h);
    push_rect(out, x0, y0, x0 + bw, y1, r, g, b, a, viewport_w, viewport_h);
    push_rect(out, x1 - bw, y0, x1, y1, r, g, b, a, viewport_w, viewport_h);
}

fn push_filled_rect(
    out: &mut Vec<u8>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
    viewport_w: f32,
    viewport_h: f32,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    push_rect(out, x, y, x + w, y + h, r, g, b, a, viewport_w, viewport_h);
}

fn push_diag_gradient_rect_rgba(
    out: &mut Vec<u8>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    c0: (u8, u8, u8, u8),
    c1: (u8, u8, u8, u8),
    viewport_w: f32,
    viewport_h: f32,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }

    let x0 = x;
    let y0 = y;
    let x1 = x + w;
    let y1 = y + h;

    let ax = to_ndc_x(x0, viewport_w);
    let ay = to_ndc_y(y1, viewport_h);
    let bx = to_ndc_x(x1, viewport_w);
    let by = to_ndc_y(y1, viewport_h);
    let cx = to_ndc_x(x1, viewport_w);
    let cy = to_ndc_y(y0, viewport_h);
    let dx = to_ndc_x(x0, viewport_w);
    let dy = to_ndc_y(y0, viewport_h);

    let (r0, g0, b0, a0) = c0;
    let (r1, g1, b1, a1) = c1;

    // Two triangles with per-vertex colors produce a smooth diagonal tint.
    push_vtx(out, ax, ay, r0, g0, b0, a0);
    push_vtx(out, bx, by, r1, g1, b1, a1);
    push_vtx(out, cx, cy, r1, g1, b1, a1);

    push_vtx(out, ax, ay, r0, g0, b0, a0);
    push_vtx(out, cx, cy, r1, g1, b1, a1);
    push_vtx(out, dx, dy, r0, g0, b0, a0);
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
    if qjs::JS_ToFloat64(ctx, &mut vw as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut vh as *mut f64, args[2]) != 0
    {
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

    let mut bytes = Vec::with_capacity(12 * 8);
    let mut clip_stack: Vec<(u32, (f32, f32, f32, f32))> = Vec::new();

    let _ = trueos_cabi_gfx_begin_frame(0xFFFFFF);
    let mut i = 0u32;
    while i + 6 < len {
        let vx = qjs::JS_GetPropertyUint32(ctx, rects, i + 0);
        let vy = qjs::JS_GetPropertyUint32(ctx, rects, i + 1);
        let vw2 = qjs::JS_GetPropertyUint32(ctx, rects, i + 2);
        let vh2 = qjs::JS_GetPropertyUint32(ctx, rects, i + 3);
        let vd = qjs::JS_GetPropertyUint32(ctx, rects, i + 4);
        let vs = qjs::JS_GetPropertyUint32(ctx, rects, i + 5);
        let vsty = qjs::JS_GetPropertyUint32(ctx, rects, i + 6);

        let mut x = 0.0f64;
        let mut y = 0.0f64;
        let mut w = 0.0f64;
        let mut h = 0.0f64;
        let mut depth_f = 0.0f64;
        let mut scrollable_f = 0.0f64;
        let mut style_f = 0.0f64;
        let _ = qjs::JS_ToFloat64(ctx, &mut x as *mut f64, vx);
        let _ = qjs::JS_ToFloat64(ctx, &mut y as *mut f64, vy);
        let _ = qjs::JS_ToFloat64(ctx, &mut w as *mut f64, vw2);
        let _ = qjs::JS_ToFloat64(ctx, &mut h as *mut f64, vh2);
        let _ = qjs::JS_ToFloat64(ctx, &mut depth_f as *mut f64, vd);
        let _ = qjs::JS_ToFloat64(ctx, &mut scrollable_f as *mut f64, vs);
        let _ = qjs::JS_ToFloat64(ctx, &mut style_f as *mut f64, vsty);

        qjs::js_free_value(ctx, vx);
        qjs::js_free_value(ctx, vy);
        qjs::js_free_value(ctx, vw2);
        qjs::js_free_value(ctx, vh2);
        qjs::js_free_value(ctx, vd);
        qjs::js_free_value(ctx, vs);
        qjs::js_free_value(ctx, vsty);

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
        let raw_color_fill = scrollable_f.is_finite() && scrollable_f <= -0.5;
        let style = if style_f.is_finite() && style_f >= 0.0 {
            style_f as u32
        } else {
            0
        };

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
        if raw_color_fill {
            let rgba = style;
            let r = ((rgba >> 16) & 0xFF) as u8;
            let g = ((rgba >> 8) & 0xFF) as u8;
            let b = (rgba & 0xFF) as u8;
            let a = ((rgba >> 24) & 0xFF) as u8;
            push_filled_rect(
                &mut bytes, xf, yf, wf, hf, r, g, b, a, viewport_w, viewport_h,
            );
        } else {
            match style {
                2 => {
                    // Scrollbar thumb: gray filled rectangle.
                    push_filled_rect(
                        &mut bytes, xf, yf, wf, hf, 160, 160, 160, 255, viewport_w, viewport_h,
                    );
                }
                6 => {
                    // Icon pixel fill: darker tint for titlebar controls.
                    push_filled_rect(
                        &mut bytes, xf, yf, wf, hf, 24, 24, 24, 255, viewport_w, viewport_h,
                    );
                }
                5 => {
                    // Button tint: subtle diagonal two-value gradient.
                    push_diag_gradient_rect_rgba(
                        &mut bytes,
                        xf,
                        yf,
                        wf,
                        hf,
                        (236, 241, 252, 190),
                        (219, 228, 246, 190),
                        viewport_w,
                        viewport_h,
                    );
                }
                3 => {
                    // Widget-update pulse: warm highlight border.
                    push_border_rect_rgba(
                        &mut bytes, xf, yf, wf, hf, 2.0, 255, 180, 60, 230, viewport_w, viewport_h,
                    );
                }
                4 => {
                    // Alternate phase for pulse rhythm.
                    push_border_rect_rgba(
                        &mut bytes, xf, yf, wf, hf, 2.0, 255, 225, 150, 230, viewport_w, viewport_h,
                    );
                }
                7 => {
                    // Horizontal rule: a centered 1px line spanning the available width.
                    let h_i = (hf as i32).max(1);
                    let line_y = (yf as i32 + (h_i - 1) / 2) as f32;
                    push_filled_rect(
                        &mut bytes,
                        xf,
                        line_y,
                        wf,
                        1.0,
                        0,
                        0,
                        0,
                        255,
                        viewport_w,
                        viewport_h,
                    );
                }
                8 => {
                    // Dialog fill: translucent white-to-gray diagonal linear gradient.
                    push_diag_gradient_rect_rgba(
                        &mut bytes,
                        xf,
                        yf,
                        wf,
                        hf,
                        (255, 255, 255, 128),
                        (196, 196, 196, 128),
                        viewport_w,
                        viewport_h,
                    );
                }
                _ => {
                    // Default node and scrollbar frame: black 1px border.
                    push_border_rect(&mut bytes, xf, yf, wf, hf, 1.0, viewport_w, viewport_h);
                }
            }
        }
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

        i += 7;
    }

    let _ = trueos_cabi_gfx_clear_scissor();

    // Optional inline text payload: [x, y, text, x, y, text, ...]
    if argc >= 4 {
        let text_entries = args[3];
        let text_len_val =
            qjs::JS_GetPropertyStr(ctx, text_entries, LENGTH_KEY.as_ptr() as *const c_char);
        let mut text_len_f = 0.0f64;
        let _ = qjs::JS_ToFloat64(ctx, &mut text_len_f as *mut f64, text_len_val);
        qjs::js_free_value(ctx, text_len_val);
        let text_len = if text_len_f.is_finite() && text_len_f > 0.0 {
            text_len_f as u32
        } else {
            0
        };

        let view_w_u = viewport_w.max(1.0) as u32;
        let view_h_u = viewport_h.max(1.0) as u32;
        let mut ti = 0u32;
        while ti + 2 < text_len {
            let vx = qjs::JS_GetPropertyUint32(ctx, text_entries, ti + 0);
            let vy = qjs::JS_GetPropertyUint32(ctx, text_entries, ti + 1);
            let vt = qjs::JS_GetPropertyUint32(ctx, text_entries, ti + 2);

            let mut x = 0.0f64;
            let mut y = 0.0f64;
            let _ = qjs::JS_ToFloat64(ctx, &mut x as *mut f64, vx);
            let _ = qjs::JS_ToFloat64(ctx, &mut y as *mut f64, vy);
            qjs::js_free_value(ctx, vx);
            qjs::js_free_value(ctx, vy);

            let mut text_n = 0usize;
            let text_c = qjs::JS_ToCStringLen2(ctx, &mut text_n as *mut usize, vt, 0);
            qjs::js_free_value(ctx, vt);
            if !text_c.is_null() && text_n > 0 {
                let text = core::slice::from_raw_parts(text_c as *const u8, text_n);
                let _ = qjs::cmd_stream::draw_text_widget_in_frame(
                    text, x as f32, y as f32, view_w_u, view_h_u,
                );
                qjs::JS_FreeCString(ctx, text_c);
            } else if !text_c.is_null() {
                qjs::JS_FreeCString(ctx, text_c);
            }

            ti += 3;
        }
    }

    let _ = trueos_cabi_gfx_end_frame();

    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_read_window_svg_cmds(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JSValue::undefined();
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut kind_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut kind_f as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    let kind = window_icon_kind_from_f64(kind_f);

    let out = qjs::JS_NewArray(ctx);
    let mut wrote = false;
    let _ = qjs::svg::with_window_svg(kind, |icon| {
        wrote = true;
        let mut i: u32 = 0;
        let _ =
            qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, icon.width() as f64));
        i += 1;
        let _ =
            qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, icon.height() as f64));
        i += 1;

        let cmds = icon.line_cmds();
        let _ = qjs::JS_SetPropertyUint32(
            ctx,
            out,
            i,
            qjs::JS_NewFloat64(ctx, (cmds.len() / 6) as f64),
        );
        i += 1;

        let mut ci = 0usize;
        while ci + 5 < cmds.len() {
            let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, cmds[ci]));
            i += 1;
            let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, cmds[ci + 1]));
            i += 1;
            let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, cmds[ci + 2]));
            i += 1;
            let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, cmds[ci + 3]));
            i += 1;
            let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, cmds[ci + 4]));
            i += 1;
            let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, cmds[ci + 5]));
            i += 1;
            ci += 6;
        }
    });

    if wrote {
        out
    } else {
        qjs::js_free_value(ctx, out);
        qjs::JSValue::undefined()
    }
}

unsafe extern "C" fn qjs_import_svg_asset(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 4 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut asset_id_f = 0.0f64;
    let mut width_f = 0.0f64;
    let mut height_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut asset_id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut width_f as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut height_f as *mut f64, args[2]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let flat = args[3];
    static LENGTH_KEY: &[u8] = b"length\0";
    let len_val = qjs::JS_GetPropertyStr(ctx, flat, LENGTH_KEY.as_ptr() as *const c_char);
    let mut len_f = 0.0f64;
    let _ = qjs::JS_ToFloat64(ctx, &mut len_f as *mut f64, len_val);
    qjs::js_free_value(ctx, len_val);
    let len = if len_f.is_finite() && len_f > 0.0 {
        len_f as u32
    } else {
        0
    };

    let mut cmds = alloc::vec::Vec::with_capacity(len as usize);
    for i in 0..len {
        let v = qjs::JS_GetPropertyUint32(ctx, flat, i);
        let mut f = 0.0f64;
        let _ = qjs::JS_ToFloat64(ctx, &mut f as *mut f64, v);
        qjs::js_free_value(ctx, v);
        cmds.push(f);
    }

    let ok = qjs::svg::import_svg_from_flat(
        asset_id_f as u32,
        width_f.max(1.0) as u32,
        height_f.max(1.0) as u32,
        &cmds,
    );
    qjs::JS_NewFloat64(ctx, if ok { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_read_svg_pixels(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JSValue::undefined();
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut asset_id_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut asset_id_f as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    let asset_id = asset_id_f as u32;
    if asset_id == 0 {
        return qjs::JSValue::undefined();
    }

    let out = qjs::JS_NewArray(ctx);
    let mut wrote = false;
    let _ = qjs::svg::with_imported_svg(asset_id, |svg| {
        wrote = true;
        let mut i: u32 = 0;
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, svg.width() as f64));
        i += 1;
        let _ =
            qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, svg.height() as f64));
        i += 1;
        let px = svg.pixels_rgba();
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, px.len() as f64));
        i += 1;
        for p in px {
            let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, *p as f64));
            i += 1;
        }
    });

    if wrote {
        out
    } else {
        qjs::js_free_value(ctx, out);
        qjs::JSValue::undefined()
    }
}

unsafe extern "C" fn qjs_draw_cursor_plane(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // Cursor-plane drawing is intentionally disabled. Position/state is provided
    // via `qjs_read_cursor_state` only.
    qjs::JSValue::undefined()
}

unsafe extern "C" fn qjs_read_cursor_state(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JSValue::undefined();
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut cursor_id_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut cursor_id_f as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    let cursor_id = if cursor_id_f.is_finite() && cursor_id_f >= 1.0 {
        cursor_id_f as u32
    } else {
        0
    };
    if cursor_id == 0 {
        return qjs::JSValue::undefined();
    }

    let mut x = 0i32;
    let mut y = 0i32;
    let mut buttons = 0u32;
    let pos_rc = trueos_cabi_input_cursor_pos(cursor_id, &mut x as *mut i32, &mut y as *mut i32);
    let _ = trueos_cabi_input_cursor_buttons(cursor_id, &mut buttons as *mut u32);

    let obj = qjs::JS_NewObject(ctx);
    static K_OK: &[u8] = b"ok\0";
    static K_X: &[u8] = b"x\0";
    static K_Y: &[u8] = b"y\0";
    static K_BUTTONS: &[u8] = b"buttons\0";

    let ok = if pos_rc == 0 { 1 } else { 0 };
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        K_OK.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, ok as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        K_X.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, x as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        K_Y.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, y as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        K_BUTTONS.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, buttons as f64),
    );
    obj
}

unsafe extern "C" fn qjs_read_cursor_events_since(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JSValue::undefined();
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut read_seq_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut read_seq_f as *mut f64, args[0]) != 0 {
        return qjs::JSValue::undefined();
    }
    let read_seq = if read_seq_f.is_finite() && read_seq_f >= 0.0 {
        read_seq_f as u64
    } else {
        0
    };

    const CAP: usize = 32;
    let mut events = [qjs::trueos_shims::TrueosHidCursorEvent::default(); CAP];
    let mut next_seq = read_seq;
    let mut dropped = 0u32;
    let wrote = trueos_cabi_input_read_cursor_events_since(
        read_seq,
        events.as_mut_ptr(),
        CAP as u32,
        &mut next_seq as *mut u64,
        &mut dropped as *mut u32,
    ) as usize;

    let out = qjs::JS_NewArray(ctx);
    let mut i = 0u32;
    let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, next_seq as f64));
    i += 1;
    let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, dropped as f64));
    i += 1;
    let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, wrote as f64));
    i += 1;

    let count = core::cmp::min(wrote, CAP);
    for ev in events.iter().take(count) {
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, ev.slot_id as f64));
        i += 1;
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, ev.x));
        i += 1;
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, ev.y));
        i += 1;
        let _ = qjs::JS_SetPropertyUint32(
            ctx,
            out,
            i,
            qjs::JS_NewFloat64(ctx, ev.buttons_down as f64),
        );
        i += 1;
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, ev.wheel as f64));
        i += 1;
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, ev.flags as f64));
        i += 1;
    }

    out
}

pub unsafe fn install_layout_api(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    static NAME: &[u8] = b"__trueosDrawLayoutRects\0";
    static FN_NAME: &[u8] = b"__trueosDrawLayoutRects\0";
    static CURSOR_STATE_NAME: &[u8] = b"__trueosReadCursorState\0";
    static CURSOR_STATE_FN_NAME: &[u8] = b"__trueosReadCursorState\0";
    static CURSOR_EVENTS_NAME: &[u8] = b"__trueosReadCursorEventsSince\0";
    static CURSOR_EVENTS_FN_NAME: &[u8] = b"__trueosReadCursorEventsSince\0";
    static ICON_CMDS_NAME: &[u8] = b"__trueosReadWindowSvgCmds\0";
    static ICON_CMDS_FN_NAME: &[u8] = b"__trueosReadWindowSvgCmds\0";
    static SVG_IMPORT_NAME: &[u8] = b"__trueosImportSvgAsset\0";
    static SVG_IMPORT_FN_NAME: &[u8] = b"__trueosImportSvgAsset\0";
    static SVG_PIXELS_NAME: &[u8] = b"__trueosReadSvgPixels\0";
    static SVG_PIXELS_FN_NAME: &[u8] = b"__trueosReadSvgPixels\0";

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

    // Keep cursor-state read API but do not expose the old custom cursor-plane
    // draw hook.

    let cursor_state_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_read_cursor_state),
        CURSOR_STATE_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        CURSOR_STATE_NAME.as_ptr() as *const c_char,
        cursor_state_func,
    );

    let cursor_events_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_read_cursor_events_since),
        CURSOR_EVENTS_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        CURSOR_EVENTS_NAME.as_ptr() as *const c_char,
        cursor_events_func,
    );

    let icon_cmds_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_read_window_svg_cmds),
        ICON_CMDS_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        ICON_CMDS_NAME.as_ptr() as *const c_char,
        icon_cmds_func,
    );

    let svg_import_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_import_svg_asset),
        SVG_IMPORT_FN_NAME.as_ptr() as *const c_char,
        4,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        SVG_IMPORT_NAME.as_ptr() as *const c_char,
        svg_import_func,
    );

    let svg_pixels_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_read_svg_pixels),
        SVG_PIXELS_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        SVG_PIXELS_NAME.as_ptr() as *const c_char,
        svg_pixels_func,
    );
    qjs::js_free_value(ctx, global);
}
