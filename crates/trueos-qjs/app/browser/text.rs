#![cfg(feature = "trueos")]

use core::ffi::c_char;

use crate as qjs;

unsafe extern "C" fn qjs_draw_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    // Minimal text widget ABI: (text, x, y) only.
    if argc < 3 || argv.is_null() {
        return qjs::JSValue::undefined();
    }

    let args = core::slice::from_raw_parts(argv, argc as usize);

    let mut x = 0.0f64;
    let mut y = 0.0f64;
    if qjs::JS_ToFloat64(ctx, &mut x as *mut f64, args[1]) != 0
        || qjs::JS_ToFloat64(ctx, &mut y as *mut f64, args[2]) != 0
    {
        return qjs::JSValue::undefined();
    }

    let mut text_len: usize = 0;
    let text_ptr = qjs::JS_ToCStringLen2(ctx, &mut text_len as *mut usize, args[0], 0);
    if text_ptr.is_null() {
        return qjs::JSValue::undefined();
    }

    let text = core::slice::from_raw_parts(text_ptr as *const u8, text_len);
    let _ = qjs::cmd_stream::draw_text_widget(text, x as f32, y as f32);

    qjs::JS_FreeCString(ctx, text_ptr);
    qjs::JSValue::undefined()
}

pub unsafe fn install_text_api(ctx: *mut qjs::JSContext) {
    if ctx.is_null() {
        return;
    }

    static NAME: &[u8] = b"__trueosDrawText\0";
    static FN_NAME: &[u8] = b"__trueosDrawText\0";

    let global = qjs::JS_GetGlobalObject(ctx);
    let func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_draw_text),
        FN_NAME.as_ptr() as *const c_char,
        3,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, NAME.as_ptr() as *const c_char, func);
    qjs::js_free_value(ctx, global);
}
