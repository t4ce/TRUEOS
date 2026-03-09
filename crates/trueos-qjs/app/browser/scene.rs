#![cfg(feature = "trueos")]
use core::ffi::CStr;
use core::ffi::c_char;

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
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

    let begin_rc = trueos_cabi_gfx_begin_frame(0xF4F4F4);
    if begin_rc != 0 {
        return qjs::JSValue::undefined();
    }

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

    let _end_rc = trueos_cabi_gfx_end_frame();
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
