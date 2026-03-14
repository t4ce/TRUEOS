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
    static ICON_CMDS_NAME: &[u8] = b"__trueosReadWindowSvgCmds\0";
    static ICON_CMDS_FN_NAME: &[u8] = b"__trueosReadWindowSvgCmds\0";
    static SVG_IMPORT_NAME: &[u8] = b"__trueosImportSvgAsset\0";
    static SVG_IMPORT_FN_NAME: &[u8] = b"__trueosImportSvgAsset\0";
    static SVG_PIXELS_NAME: &[u8] = b"__trueosReadSvgPixels\0";
    static SVG_PIXELS_FN_NAME: &[u8] = b"__trueosReadSvgPixels\0";

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
    qjs::js_free_value(ctx, global);
}
