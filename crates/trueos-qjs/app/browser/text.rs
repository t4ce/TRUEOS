#![cfg(feature = "trueos")]

use core::ffi::c_char;

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

#[inline]
fn log_str(s: &str) {
    if s.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, s.as_ptr(), s.len()) };
}

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

    let text_ptr = qjs::js_to_cstring(ctx, args[0]);
    if text_ptr.is_null() {
        return qjs::JSValue::undefined();
    }

    // Foundation step: expose a stable text-widget entrypoint.
    // Raster text rendering will be wired behind this API in follow-up patches.
    let _ = x;
    let _ = y;
    log_str("qjs-text: draw text\n");

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
