#![cfg(feature = "trueos")]
use crate as qjs;
use alloc::vec::Vec;
use core::ffi::c_char;

unsafe extern "C" {
    fn trueos_cabi_input_cursor_pos(cursor_id: u32, out_x: *mut i32, out_y: *mut i32) -> i32;
    fn trueos_cabi_input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32;
    fn trueos_cabi_input_read_cursor_events_since(
        read_seq: u64,
        out: *mut qjs::trueos_shims::TrueosHidCursorEvent,
        out_cap: u32,
        out_next_seq: *mut u64,
        out_dropped: *mut u32,
    ) -> u32;
    fn trueos_cabi_input_write_cursor(
        slot_id: u32,
        x: i32,
        y: i32,
        buttons_down: u32,
        wheel: i32,
        flags: u32,
    ) -> i32;
    fn trueos_cabi_trueosfs_primary_html_tree(
        max_entries: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_cabi_ui2_primary_browser_window_id() -> u32;
    fn trueos_cabi_ui2_window_info(window_id: u32, out_info: *mut TrueosUi2WindowInfo) -> i32;
    fn trueos_cabi_ui2_window_set_title(
        window_id: u32,
        title_ptr: *const u8,
        title_len: usize,
    ) -> i32;
    fn trueos_cabi_ui2_window_set_position(window_id: u32, x: i32, y: i32) -> i32;
    fn trueos_cabi_ui2_window_set_size(window_id: u32, width: u32, height: u32) -> i32;
    fn trueos_cabi_ui2_window_set_decorations(window_id: u32, mode: u32) -> i32;
    fn trueos_cabi_ui2_window_set_vertical_scrollbar_side(window_id: u32, side: u32) -> i32;
    fn trueos_cabi_ui2_window_set_horizontal_scrollbar_side(window_id: u32, side: u32) -> i32;
    fn trueos_cabi_ui2_window_minimize(window_id: u32) -> i32;
    fn trueos_cabi_ui2_window_maximize(window_id: u32) -> i32;
    fn trueos_cabi_ui2_window_restore(window_id: u32) -> i32;
    fn trueos_cabi_ui2_window_focus(window_id: u32) -> i32;
    fn trueos_cabi_ui2_window_close(window_id: u32) -> i32;
    fn trueos_cabi_ui2_window_begin_move(window_id: u32) -> i32;
    fn trueos_cabi_ui2_window_begin_resize(window_id: u32, edge_mask: u32) -> i32;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
struct TrueosUi2WindowInfo {
    id: u32,
    kind: u32,
    state: u32,
    decoration_mode: u32,
    visible: u32,
    selected: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
    decoration_x: i32,
    decoration_y: i32,
    decoration_width: u32,
    decoration_height: u32,
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
fn round_to_i32(v: f64) -> i32 {
    if !v.is_finite() {
        return 0;
    }
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
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
        let _ =
            qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, ev.buttons_down as f64));
        i += 1;
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, ev.wheel as f64));
        i += 1;
        let _ = qjs::JS_SetPropertyUint32(ctx, out, i, qjs::JS_NewFloat64(ctx, ev.flags as f64));
        i += 1;
    }

    out
}

unsafe extern "C" fn qjs_write_cursor_event(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 6 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut slot_id_f = 0.0f64;
    let mut x_f = 0.0f64;
    let mut y_f = 0.0f64;
    let mut buttons_f = 0.0f64;
    let mut wheel_f = 0.0f64;
    let mut flags_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut slot_id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut x_f as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut y_f as *mut f64, args[2]) != 0
        || qjs::JS_ToFloat64(ctx, &mut buttons_f as *mut f64, args[3]) != 0
        || qjs::JS_ToFloat64(ctx, &mut wheel_f as *mut f64, args[4]) != 0
        || qjs::JS_ToFloat64(ctx, &mut flags_f as *mut f64, args[5]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let slot_id = if slot_id_f.is_finite() && slot_id_f >= 1.0 {
        slot_id_f as u32
    } else {
        0
    };
    if slot_id == 0 {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let rc = trueos_cabi_input_write_cursor(
        slot_id,
        round_to_i32(x_f),
        round_to_i32(y_f),
        if buttons_f.is_finite() && buttons_f >= 0.0 {
            buttons_f as u32
        } else {
            0
        },
        if wheel_f.is_finite() {
            round_to_i32(wheel_f)
        } else {
            0
        },
        if flags_f.is_finite() && flags_f >= 0.0 {
            flags_f as u32
        } else {
            0
        },
    );
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_read_primary_trueosfs_tree_html(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let max_entries = if argc >= 1 && !argv.is_null() {
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut value = 0.0f64;
        if qjs::JS_ToFloat64(ctx, &mut value as *mut f64, args[0]) == 0
            && value.is_finite()
            && value > 0.0
        {
            value as u32
        } else {
            64
        }
    } else {
        64
    };

    let len = trueos_cabi_trueosfs_primary_html_tree(max_entries, core::ptr::null_mut(), 0);
    if len <= 0 {
        return qjs::JSValue::undefined();
    }

    let mut bytes = alloc::vec![0u8; len as usize];
    let got = trueos_cabi_trueosfs_primary_html_tree(max_entries, bytes.as_mut_ptr(), bytes.len());
    if got <= 0 {
        return qjs::JSValue::undefined();
    }
    bytes.truncate(got as usize);
    qjs::JS_NewStringLen(ctx, bytes.as_ptr() as *const c_char, bytes.len())
}

#[inline]
fn round_to_u32(v: f64) -> Option<u32> {
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    Some(if v >= 0.0 { (v + 0.5) as u32 } else { 0 })
}

unsafe fn js_window_info_object(ctx: *mut qjs::JSContext, info: &TrueosUi2WindowInfo) -> qjs::JSValue {
    static K_ID: &[u8] = b"id\0";
    static K_KIND: &[u8] = b"kind\0";
    static K_STATE: &[u8] = b"state\0";
    static K_DECORATION_MODE: &[u8] = b"decorationMode\0";
    static K_VISIBLE: &[u8] = b"visible\0";
    static K_SELECTED: &[u8] = b"selected\0";
    static K_X: &[u8] = b"x\0";
    static K_Y: &[u8] = b"y\0";
    static K_WIDTH: &[u8] = b"width\0";
    static K_HEIGHT: &[u8] = b"height\0";
    static K_CONTENT_RECT: &[u8] = b"contentRect\0";
    static K_DECORATION_RECT: &[u8] = b"decorationRect\0";

    let rect = qjs::JS_NewObject(ctx);
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_ID.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.id as f64));
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_KIND.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.kind as f64));
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_STATE.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.state as f64));
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        rect,
        K_DECORATION_MODE.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.decoration_mode as f64),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_VISIBLE.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.visible as f64));
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_SELECTED.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.selected as f64));
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_X.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.x as f64));
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_Y.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.y as f64));
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        rect,
        K_WIDTH.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.width as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        rect,
        K_HEIGHT.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.height as f64),
    );

    let content = qjs::JS_NewObject(ctx);
    let _ = qjs::JS_SetPropertyStr(ctx, content, K_X.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.content_x as f64));
    let _ = qjs::JS_SetPropertyStr(ctx, content, K_Y.as_ptr() as *const c_char, qjs::JS_NewFloat64(ctx, info.content_y as f64));
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        content,
        K_WIDTH.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.content_width as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        content,
        K_HEIGHT.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.content_height as f64),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_CONTENT_RECT.as_ptr() as *const c_char, content);

    let decoration = qjs::JS_NewObject(ctx);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        decoration,
        K_X.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.decoration_x as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        decoration,
        K_Y.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.decoration_y as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        decoration,
        K_WIDTH.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.decoration_width as f64),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        decoration,
        K_HEIGHT.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, info.decoration_height as f64),
    );
    let _ = qjs::JS_SetPropertyStr(ctx, rect, K_DECORATION_RECT.as_ptr() as *const c_char, decoration);

    rect
}

unsafe extern "C" fn qjs_primary_window_id(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs::JS_NewFloat64(ctx, trueos_cabi_ui2_primary_browser_window_id() as f64)
}

unsafe extern "C" fn qjs_window_get_info(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JSValue::undefined();
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0 || !id_f.is_finite() || id_f < 1.0 {
        return qjs::JSValue::undefined();
    }
    let mut info = TrueosUi2WindowInfo::default();
    if trueos_cabi_ui2_window_info(id_f as u32, &mut info as *mut TrueosUi2WindowInfo) != 0 {
        return qjs::JSValue::undefined();
    }
    js_window_info_object(ctx, &info)
}

unsafe extern "C" fn qjs_window_set_title(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 2 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0 || !id_f.is_finite() || id_f < 1.0 {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let mut len: usize = 0;
    let title_c = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[1], 0);
    if title_c.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let rc = trueos_cabi_ui2_window_set_title(id_f as u32, title_c as *const u8, len);
    qjs::JS_FreeCString(ctx, title_c);
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_window_set_position(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 3 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    let mut x_f = 0.0f64;
    let mut y_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut x_f as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut y_f as *mut f64, args[2]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let rc = trueos_cabi_ui2_window_set_position(id_f as u32, round_to_i32(x_f), round_to_i32(y_f));
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_window_set_size(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 3 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    let mut w_f = 0.0f64;
    let mut h_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut w_f as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut h_f as *mut f64, args[2]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let Some(width) = round_to_u32(w_f) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let Some(height) = round_to_u32(h_f) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let rc = trueos_cabi_ui2_window_set_size(id_f as u32, width, height);
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_window_set_decorations(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 2 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    let mut mode_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[1]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let rc = trueos_cabi_ui2_window_set_decorations(id_f as u32, mode_f.max(0.0) as u32);
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_window_set_vertical_scrollbar_side(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 2 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    let mut side_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut side_f as *mut f64, args[1]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let rc = trueos_cabi_ui2_window_set_vertical_scrollbar_side(id_f as u32, side_f.max(0.0) as u32);
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_window_set_horizontal_scrollbar_side(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 2 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    let mut side_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut side_f as *mut f64, args[1]) != 0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let rc =
        trueos_cabi_ui2_window_set_horizontal_scrollbar_side(id_f as u32, side_f.max(0.0) as u32);
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_window_simple_action(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
    action: unsafe extern "C" fn(u32) -> i32,
) -> qjs::JSValue {
    if argc < 1 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0 || !id_f.is_finite() || id_f < 1.0 {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let rc = action(id_f as u32);
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

unsafe extern "C" fn qjs_window_minimize(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs_window_simple_action(ctx, this_val, argc, argv, trueos_cabi_ui2_window_minimize)
}

unsafe extern "C" fn qjs_window_maximize(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs_window_simple_action(ctx, this_val, argc, argv, trueos_cabi_ui2_window_maximize)
}

unsafe extern "C" fn qjs_window_restore(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs_window_simple_action(ctx, this_val, argc, argv, trueos_cabi_ui2_window_restore)
}

unsafe extern "C" fn qjs_window_focus(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs_window_simple_action(ctx, this_val, argc, argv, trueos_cabi_ui2_window_focus)
}

unsafe extern "C" fn qjs_window_close(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs_window_simple_action(ctx, this_val, argc, argv, trueos_cabi_ui2_window_close)
}

unsafe extern "C" fn qjs_window_begin_move(
    ctx: *mut qjs::JSContext,
    this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    qjs_window_simple_action(ctx, this_val, argc, argv, trueos_cabi_ui2_window_begin_move)
}

unsafe extern "C" fn qjs_window_begin_resize(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if argc < 2 || argv.is_null() {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let args = core::slice::from_raw_parts(argv, argc as usize);
    let mut id_f = 0.0f64;
    let mut edge_mask_f = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut id_f as *mut f64, args[0]) != 0
        || qjs::JS_ToFloat64(ctx, &mut edge_mask_f as *mut f64, args[1]) != 0
        || !id_f.is_finite()
        || id_f < 1.0
        || !edge_mask_f.is_finite()
        || edge_mask_f < 0.0
    {
        return qjs::JS_NewFloat64(ctx, 0.0);
    }
    let rc = trueos_cabi_ui2_window_begin_resize(id_f as u32, edge_mask_f as u32);
    qjs::JS_NewFloat64(ctx, if rc == 0 { 1.0 } else { 0.0 })
}

pub unsafe fn install_layout_api(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    static CURSOR_STATE_NAME: &[u8] = b"__trueosReadCursorState\0";
    static CURSOR_STATE_FN_NAME: &[u8] = b"__trueosReadCursorState\0";
    static CURSOR_EVENTS_NAME: &[u8] = b"__trueosReadCursorEventsSince\0";
    static CURSOR_EVENTS_FN_NAME: &[u8] = b"__trueosReadCursorEventsSince\0";
    static CURSOR_WRITE_NAME: &[u8] = b"__trueosWriteCursorEvent\0";
    static CURSOR_WRITE_FN_NAME: &[u8] = b"__trueosWriteCursorEvent\0";
    static TRUEOSFS_TREE_NAME: &[u8] = b"__trueosReadPrimaryTrueosFsTreeHtml\0";
    static TRUEOSFS_TREE_FN_NAME: &[u8] = b"__trueosReadPrimaryTrueosFsTreeHtml\0";
    static WINDOW_ID_NAME: &[u8] = b"__trueosPrimaryWindowId\0";
    static WINDOW_ID_FN_NAME: &[u8] = b"__trueosPrimaryWindowId\0";
    static WINDOW_INFO_NAME: &[u8] = b"__trueosWindowGetInfo\0";
    static WINDOW_INFO_FN_NAME: &[u8] = b"__trueosWindowGetInfo\0";
    static WINDOW_SET_TITLE_NAME: &[u8] = b"__trueosWindowSetTitle\0";
    static WINDOW_SET_TITLE_FN_NAME: &[u8] = b"__trueosWindowSetTitle\0";
    static WINDOW_SET_POSITION_NAME: &[u8] = b"__trueosWindowSetPosition\0";
    static WINDOW_SET_POSITION_FN_NAME: &[u8] = b"__trueosWindowSetPosition\0";
    static WINDOW_SET_SIZE_NAME: &[u8] = b"__trueosWindowSetSize\0";
    static WINDOW_SET_SIZE_FN_NAME: &[u8] = b"__trueosWindowSetSize\0";
    static WINDOW_SET_DECORATIONS_NAME: &[u8] = b"__trueosWindowSetDecorations\0";
    static WINDOW_SET_DECORATIONS_FN_NAME: &[u8] = b"__trueosWindowSetDecorations\0";
    static WINDOW_SET_VERTICAL_SCROLLBAR_SIDE_NAME: &[u8] = b"__trueosWindowSetVerticalScrollbarSide\0";
    static WINDOW_SET_VERTICAL_SCROLLBAR_SIDE_FN_NAME: &[u8] =
        b"__trueosWindowSetVerticalScrollbarSide\0";
    static WINDOW_SET_HORIZONTAL_SCROLLBAR_SIDE_NAME: &[u8] =
        b"__trueosWindowSetHorizontalScrollbarSide\0";
    static WINDOW_SET_HORIZONTAL_SCROLLBAR_SIDE_FN_NAME: &[u8] =
        b"__trueosWindowSetHorizontalScrollbarSide\0";
    static WINDOW_MINIMIZE_NAME: &[u8] = b"__trueosWindowMinimize\0";
    static WINDOW_MINIMIZE_FN_NAME: &[u8] = b"__trueosWindowMinimize\0";
    static WINDOW_MAXIMIZE_NAME: &[u8] = b"__trueosWindowMaximize\0";
    static WINDOW_MAXIMIZE_FN_NAME: &[u8] = b"__trueosWindowMaximize\0";
    static WINDOW_RESTORE_NAME: &[u8] = b"__trueosWindowRestore\0";
    static WINDOW_RESTORE_FN_NAME: &[u8] = b"__trueosWindowRestore\0";
    static WINDOW_FOCUS_NAME: &[u8] = b"__trueosWindowFocus\0";
    static WINDOW_FOCUS_FN_NAME: &[u8] = b"__trueosWindowFocus\0";
    static WINDOW_CLOSE_NAME: &[u8] = b"__trueosWindowClose\0";
    static WINDOW_CLOSE_FN_NAME: &[u8] = b"__trueosWindowClose\0";
    static WINDOW_BEGIN_MOVE_NAME: &[u8] = b"__trueosWindowBeginMove\0";
    static WINDOW_BEGIN_MOVE_FN_NAME: &[u8] = b"__trueosWindowBeginMove\0";
    static WINDOW_BEGIN_RESIZE_NAME: &[u8] = b"__trueosWindowBeginResize\0";
    static WINDOW_BEGIN_RESIZE_FN_NAME: &[u8] = b"__trueosWindowBeginResize\0";

    let global = qjs::JS_GetGlobalObject(ctx);

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

    let cursor_write_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_write_cursor_event),
        CURSOR_WRITE_FN_NAME.as_ptr() as *const c_char,
        6,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        CURSOR_WRITE_NAME.as_ptr() as *const c_char,
        cursor_write_func,
    );

    let trueosfs_tree_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_read_primary_trueosfs_tree_html),
        TRUEOSFS_TREE_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        TRUEOSFS_TREE_NAME.as_ptr() as *const c_char,
        trueosfs_tree_func,
    );

    let window_id_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_primary_window_id),
        WINDOW_ID_FN_NAME.as_ptr() as *const c_char,
        0,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_ID_NAME.as_ptr() as *const c_char,
        window_id_func,
    );

    let window_info_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_get_info),
        WINDOW_INFO_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_INFO_NAME.as_ptr() as *const c_char,
        window_info_func,
    );

    let window_set_title_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_set_title),
        WINDOW_SET_TITLE_FN_NAME.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_SET_TITLE_NAME.as_ptr() as *const c_char,
        window_set_title_func,
    );

    let window_set_position_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_set_position),
        WINDOW_SET_POSITION_FN_NAME.as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_SET_POSITION_NAME.as_ptr() as *const c_char,
        window_set_position_func,
    );

    let window_set_size_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_set_size),
        WINDOW_SET_SIZE_FN_NAME.as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_SET_SIZE_NAME.as_ptr() as *const c_char,
        window_set_size_func,
    );

    let window_set_decorations_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_set_decorations),
        WINDOW_SET_DECORATIONS_FN_NAME.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_SET_DECORATIONS_NAME.as_ptr() as *const c_char,
        window_set_decorations_func,
    );

    let window_set_vertical_scrollbar_side_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_set_vertical_scrollbar_side),
        WINDOW_SET_VERTICAL_SCROLLBAR_SIDE_FN_NAME.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_SET_VERTICAL_SCROLLBAR_SIDE_NAME.as_ptr() as *const c_char,
        window_set_vertical_scrollbar_side_func,
    );

    let window_set_horizontal_scrollbar_side_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_set_horizontal_scrollbar_side),
        WINDOW_SET_HORIZONTAL_SCROLLBAR_SIDE_FN_NAME.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_SET_HORIZONTAL_SCROLLBAR_SIDE_NAME.as_ptr() as *const c_char,
        window_set_horizontal_scrollbar_side_func,
    );

    let window_minimize_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_minimize),
        WINDOW_MINIMIZE_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_MINIMIZE_NAME.as_ptr() as *const c_char,
        window_minimize_func,
    );

    let window_maximize_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_maximize),
        WINDOW_MAXIMIZE_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_MAXIMIZE_NAME.as_ptr() as *const c_char,
        window_maximize_func,
    );

    let window_restore_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_restore),
        WINDOW_RESTORE_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_RESTORE_NAME.as_ptr() as *const c_char,
        window_restore_func,
    );

    let window_focus_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_focus),
        WINDOW_FOCUS_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_FOCUS_NAME.as_ptr() as *const c_char,
        window_focus_func,
    );

    let window_close_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_close),
        WINDOW_CLOSE_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_CLOSE_NAME.as_ptr() as *const c_char,
        window_close_func,
    );

    let window_begin_move_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_begin_move),
        WINDOW_BEGIN_MOVE_FN_NAME.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_BEGIN_MOVE_NAME.as_ptr() as *const c_char,
        window_begin_move_func,
    );

    let window_begin_resize_func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_window_begin_resize),
        WINDOW_BEGIN_RESIZE_FN_NAME.as_ptr() as *const c_char,
        2,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        WINDOW_BEGIN_RESIZE_NAME.as_ptr() as *const c_char,
        window_begin_resize_func,
    );
    qjs::js_free_value(ctx, global);
}
