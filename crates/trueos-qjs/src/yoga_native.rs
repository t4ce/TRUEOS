#![cfg(feature = "trueos")]

use core::ffi::{CStr, c_char};

use crate as qjs;

#[inline]
fn js_i32(ctx: *mut qjs::JSContext, v: i32) -> qjs::JSValue {
    unsafe { qjs::JS_NewFloat64(ctx, v as f64) }
}

#[inline]
fn js_u32(ctx: *mut qjs::JSContext, v: u32) -> qjs::JSValue {
    unsafe { qjs::JS_NewFloat64(ctx, v as f64) }
}

#[inline]
pub(crate) unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(module_name).to_bytes() };
    if name != b"trueos:yoga" && name != b"yoga-native" {
        return core::ptr::null_mut();
    }

    // Keep a stable API shape while native Yoga C symbols are not linked yet.
    unsafe extern "C" fn yoga_noop(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_zero_u32(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_u32(ctx, 0)
    }

    unsafe extern "C" fn yoga_zero_f64(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { qjs::JS_NewFloat64(ctx, 0.0) }
    }

    unsafe extern "C" fn yoga_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> i32 {
        macro_rules! export_fn {
            ($name:literal, $func:expr, $argc:expr) => {{
                let k = concat!($name, "\0");
                let f = unsafe {
                    qjs::JS_NewCFunction2(
                        ctx,
                        Some($func),
                        k.as_ptr() as *const c_char,
                        $argc,
                        qjs::JS_CFUNC_GENERIC,
                        0,
                    )
                };
                let _ = unsafe { qjs::JS_SetModuleExport(ctx, m, k.as_ptr() as *const c_char, f) };
            }};
        }

        macro_rules! export_i32 {
            ($name:literal, $value:expr) => {{
                let k = concat!($name, "\0");
                let _ = unsafe {
                    qjs::JS_SetModuleExport(ctx, m, k.as_ptr() as *const c_char, js_i32(ctx, $value))
                };
            }};
        }

        export_fn!("configCreate", yoga_zero_u32, 0);
        export_fn!("configFree", yoga_noop, 1);
        export_fn!("configSetUseWebDefaults", yoga_noop, 2);
        export_fn!("nodeCreate", yoga_zero_u32, 1);
        export_fn!("nodeFreeRecursive", yoga_noop, 1);
        export_fn!("nodeInsertChild", yoga_noop, 3);
        export_fn!("nodeGetChildCount", yoga_zero_u32, 1);
        export_fn!("nodeSetFlexDirection", yoga_noop, 2);
        export_fn!("nodeSetAlignItems", yoga_noop, 2);
        export_fn!("nodeSetAlignSelf", yoga_noop, 2);
        export_fn!("nodeSetJustifyContent", yoga_noop, 2);
        export_fn!("nodeSetFlexWrap", yoga_noop, 2);
        export_fn!("nodeSetFlexGrow", yoga_noop, 2);
        export_fn!("nodeSetFlexShrink", yoga_noop, 2);
        export_fn!("nodeSetPositionType", yoga_noop, 2);
        export_fn!("nodeSetWidth", yoga_noop, 2);
        export_fn!("nodeSetHeight", yoga_noop, 2);
        export_fn!("nodeSetMinWidth", yoga_noop, 2);
        export_fn!("nodeSetMinHeight", yoga_noop, 2);
        export_fn!("nodeSetPadding", yoga_noop, 3);
        export_fn!("nodeSetMargin", yoga_noop, 3);
        export_fn!("nodeSetPosition", yoga_noop, 3);
        export_fn!("nodeCalculateLayout", yoga_noop, 4);
        export_fn!("nodeGetComputedLeft", yoga_zero_f64, 1);
        export_fn!("nodeGetComputedTop", yoga_zero_f64, 1);
        export_fn!("nodeGetComputedWidth", yoga_zero_f64, 1);
        export_fn!("nodeGetComputedHeight", yoga_zero_f64, 1);
        export_fn!("nodeSetMeasureFunc", yoga_noop, 2);

        export_i32!("ALIGN_AUTO", 0);
        export_i32!("ALIGN_FLEX_START", 1);
        export_i32!("ALIGN_CENTER", 2);
        export_i32!("ALIGN_FLEX_END", 3);
        export_i32!("ALIGN_STRETCH", 4);
        export_i32!("JUSTIFY_FLEX_START", 0);
        export_i32!("JUSTIFY_CENTER", 1);
        export_i32!("JUSTIFY_SPACE_BETWEEN", 3);
        export_i32!("FLEX_DIRECTION_COLUMN", 0);
        export_i32!("FLEX_DIRECTION_ROW", 2);
        export_i32!("WRAP_NO_WRAP", 0);
        export_i32!("WRAP_WRAP", 1);
        export_i32!("POSITION_TYPE_RELATIVE", 1);
        export_i32!("POSITION_TYPE_ABSOLUTE", 2);
        export_i32!("EDGE_LEFT", 0);
        export_i32!("EDGE_TOP", 1);
        export_i32!("EDGE_RIGHT", 2);
        export_i32!("EDGE_BOTTOM", 3);
        export_i32!("DIRECTION_LTR", 1);
        export_i32!("MEASURE_MODE_UNDEFINED", 0);
        0
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(yoga_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    macro_rules! add_export {
        ($name:literal) => {{
            let k = concat!($name, "\0");
            let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char) };
        }};
    }

    add_export!("configCreate");
    add_export!("configFree");
    add_export!("configSetUseWebDefaults");
    add_export!("nodeCreate");
    add_export!("nodeFreeRecursive");
    add_export!("nodeInsertChild");
    add_export!("nodeGetChildCount");
    add_export!("nodeSetFlexDirection");
    add_export!("nodeSetAlignItems");
    add_export!("nodeSetAlignSelf");
    add_export!("nodeSetJustifyContent");
    add_export!("nodeSetFlexWrap");
    add_export!("nodeSetFlexGrow");
    add_export!("nodeSetFlexShrink");
    add_export!("nodeSetPositionType");
    add_export!("nodeSetWidth");
    add_export!("nodeSetHeight");
    add_export!("nodeSetMinWidth");
    add_export!("nodeSetMinHeight");
    add_export!("nodeSetPadding");
    add_export!("nodeSetMargin");
    add_export!("nodeSetPosition");
    add_export!("nodeCalculateLayout");
    add_export!("nodeGetComputedLeft");
    add_export!("nodeGetComputedTop");
    add_export!("nodeGetComputedWidth");
    add_export!("nodeGetComputedHeight");
    add_export!("nodeSetMeasureFunc");
    add_export!("ALIGN_AUTO");
    add_export!("ALIGN_FLEX_START");
    add_export!("ALIGN_CENTER");
    add_export!("ALIGN_FLEX_END");
    add_export!("ALIGN_STRETCH");
    add_export!("JUSTIFY_FLEX_START");
    add_export!("JUSTIFY_CENTER");
    add_export!("JUSTIFY_SPACE_BETWEEN");
    add_export!("FLEX_DIRECTION_COLUMN");
    add_export!("FLEX_DIRECTION_ROW");
    add_export!("WRAP_NO_WRAP");
    add_export!("WRAP_WRAP");
    add_export!("POSITION_TYPE_RELATIVE");
    add_export!("POSITION_TYPE_ABSOLUTE");
    add_export!("EDGE_LEFT");
    add_export!("EDGE_TOP");
    add_export!("EDGE_RIGHT");
    add_export!("EDGE_BOTTOM");
    add_export!("DIRECTION_LTR");
    add_export!("MEASURE_MODE_UNDEFINED");

    m
}
