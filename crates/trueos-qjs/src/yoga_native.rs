#![cfg(feature = "trueos")]

extern crate alloc;

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
    let name = CStr::from_ptr(module_name).to_bytes();
    if name != b"trueos:yoga" && name != b"yoga-native" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn qjs_yoga_config_create(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let h = qjs::trueos_shims::yoga::config_create();
        js_u32(ctx, h)
    }

    unsafe extern "C" fn qjs_yoga_config_free(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !argv.is_null() && argc >= 1 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut h = 0.0f64;
            if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) == 0 {
                qjs::trueos_shims::yoga::config_free((h as i64).max(0) as u32);
            }
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_yoga_config_set_use_web_defaults(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !argv.is_null() && argc >= 2 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut h = 0.0f64;
            let mut e = 0.0f64;
            if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) == 0
                && qjs::JS_ToFloat64(ctx, &mut e as *mut f64, args[1]) == 0
            {
                qjs::trueos_shims::yoga::config_set_use_web_defaults((h as i64).max(0) as u32, e != 0.0);
            }
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_yoga_node_create(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let mut cfg = 0u32;
        if !argv.is_null() && argc >= 1 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut v = 0.0f64;
            if qjs::JS_ToFloat64(ctx, &mut v as *mut f64, args[0]) == 0 {
                cfg = (v as i64).max(0) as u32;
            }
        }
        let h = qjs::trueos_shims::yoga::node_create(cfg);
        js_u32(ctx, h)
    }

    unsafe extern "C" fn qjs_yoga_node_free_recursive(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !argv.is_null() && argc >= 1 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut h = 0.0f64;
            if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) == 0 {
                qjs::trueos_shims::yoga::node_free_recursive((h as i64).max(0) as u32);
            }
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_yoga_node_insert_child(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !argv.is_null() && argc >= 3 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut p = 0.0f64;
            let mut c = 0.0f64;
            let mut i = 0.0f64;
            if qjs::JS_ToFloat64(ctx, &mut p as *mut f64, args[0]) == 0
                && qjs::JS_ToFloat64(ctx, &mut c as *mut f64, args[1]) == 0
                && qjs::JS_ToFloat64(ctx, &mut i as *mut f64, args[2]) == 0
            {
                qjs::trueos_shims::yoga::node_insert_child(
                    (p as i64).max(0) as u32,
                    (c as i64).max(0) as u32,
                    (i as i64).max(0) as u32,
                );
            }
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_yoga_node_get_child_count(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if argv.is_null() || argc < 1 {
            return js_u32(ctx, 0);
        }
        let args = core::slice::from_raw_parts(argv, argc as usize);
        let mut h = 0.0f64;
        if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) != 0 {
            return js_u32(ctx, 0);
        }
        js_u32(
            ctx,
            qjs::trueos_shims::yoga::node_get_child_count((h as i64).max(0) as u32),
        )
    }

    macro_rules! yoga_set_float_1 {
        ($fn_name:ident, $target:ident) => {
            unsafe extern "C" fn $fn_name(
                ctx: *mut qjs::JSContext,
                _this_val: qjs::JSValueConst,
                argc: i32,
                argv: *const qjs::JSValueConst,
            ) -> qjs::JSValue {
                if !argv.is_null() && argc >= 2 {
                    let args = core::slice::from_raw_parts(argv, argc as usize);
                    let mut h = 0.0f64;
                    let mut v = 0.0f64;
                    if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) == 0
                        && qjs::JS_ToFloat64(ctx, &mut v as *mut f64, args[1]) == 0
                    {
                        qjs::trueos_shims::yoga::$target((h as i64).max(0) as u32, v as f32);
                    }
                }
                qjs::JSValue::undefined()
            }
        };
    }

    macro_rules! yoga_set_int_1 {
        ($fn_name:ident, $target:ident) => {
            unsafe extern "C" fn $fn_name(
                ctx: *mut qjs::JSContext,
                _this_val: qjs::JSValueConst,
                argc: i32,
                argv: *const qjs::JSValueConst,
            ) -> qjs::JSValue {
                if !argv.is_null() && argc >= 2 {
                    let args = core::slice::from_raw_parts(argv, argc as usize);
                    let mut h = 0.0f64;
                    let mut v = 0.0f64;
                    if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) == 0
                        && qjs::JS_ToFloat64(ctx, &mut v as *mut f64, args[1]) == 0
                    {
                        qjs::trueos_shims::yoga::$target((h as i64).max(0) as u32, v as i32);
                    }
                }
                qjs::JSValue::undefined()
            }
        };
    }

    macro_rules! yoga_set_edge_float {
        ($fn_name:ident, $target:ident) => {
            unsafe extern "C" fn $fn_name(
                ctx: *mut qjs::JSContext,
                _this_val: qjs::JSValueConst,
                argc: i32,
                argv: *const qjs::JSValueConst,
            ) -> qjs::JSValue {
                if !argv.is_null() && argc >= 3 {
                    let args = core::slice::from_raw_parts(argv, argc as usize);
                    let mut h = 0.0f64;
                    let mut e = 0.0f64;
                    let mut v = 0.0f64;
                    if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) == 0
                        && qjs::JS_ToFloat64(ctx, &mut e as *mut f64, args[1]) == 0
                        && qjs::JS_ToFloat64(ctx, &mut v as *mut f64, args[2]) == 0
                    {
                        qjs::trueos_shims::yoga::$target(
                            (h as i64).max(0) as u32,
                            e as i32,
                            v as f32,
                        );
                    }
                }
                qjs::JSValue::undefined()
            }
        };
    }

    macro_rules! yoga_get_float_1 {
        ($fn_name:ident, $target:ident) => {
            unsafe extern "C" fn $fn_name(
                ctx: *mut qjs::JSContext,
                _this_val: qjs::JSValueConst,
                argc: i32,
                argv: *const qjs::JSValueConst,
            ) -> qjs::JSValue {
                if argv.is_null() || argc < 1 {
                    return qjs::JS_NewFloat64(ctx, 0.0);
                }
                let args = core::slice::from_raw_parts(argv, argc as usize);
                let mut h = 0.0f64;
                if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) != 0 {
                    return qjs::JS_NewFloat64(ctx, 0.0);
                }
                qjs::JS_NewFloat64(
                    ctx,
                    qjs::trueos_shims::yoga::$target((h as i64).max(0) as u32) as f64,
                )
            }
        };
    }

    yoga_set_int_1!(qjs_yoga_node_set_flex_direction, node_set_flex_direction);
    yoga_set_int_1!(qjs_yoga_node_set_align_items, node_set_align_items);
    yoga_set_int_1!(qjs_yoga_node_set_align_self, node_set_align_self);
    yoga_set_int_1!(qjs_yoga_node_set_justify_content, node_set_justify_content);
    yoga_set_int_1!(qjs_yoga_node_set_flex_wrap, node_set_flex_wrap);
    yoga_set_float_1!(qjs_yoga_node_set_flex_grow, node_set_flex_grow);
    yoga_set_float_1!(qjs_yoga_node_set_flex_shrink, node_set_flex_shrink);
    yoga_set_int_1!(qjs_yoga_node_set_position_type, node_set_position_type);
    yoga_set_float_1!(qjs_yoga_node_set_width, node_set_width);
    yoga_set_float_1!(qjs_yoga_node_set_height, node_set_height);
    yoga_set_float_1!(qjs_yoga_node_set_min_width, node_set_min_width);
    yoga_set_float_1!(qjs_yoga_node_set_min_height, node_set_min_height);
    yoga_set_edge_float!(qjs_yoga_node_set_padding, node_set_padding);
    yoga_set_edge_float!(qjs_yoga_node_set_margin, node_set_margin);
    yoga_set_edge_float!(qjs_yoga_node_set_position, node_set_position);
    yoga_get_float_1!(qjs_yoga_node_get_computed_left, node_get_computed_left);
    yoga_get_float_1!(qjs_yoga_node_get_computed_top, node_get_computed_top);
    yoga_get_float_1!(qjs_yoga_node_get_computed_width, node_get_computed_width);
    yoga_get_float_1!(qjs_yoga_node_get_computed_height, node_get_computed_height);

    unsafe extern "C" fn qjs_yoga_node_calculate_layout(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        if !argv.is_null() && argc >= 4 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut h = 0.0f64;
            let mut w = 0.0f64;
            let mut ht = 0.0f64;
            let mut d = 0.0f64;
            if qjs::JS_ToFloat64(ctx, &mut h as *mut f64, args[0]) == 0
                && qjs::JS_ToFloat64(ctx, &mut w as *mut f64, args[1]) == 0
                && qjs::JS_ToFloat64(ctx, &mut ht as *mut f64, args[2]) == 0
                && qjs::JS_ToFloat64(ctx, &mut d as *mut f64, args[3]) == 0
            {
                qjs::trueos_shims::yoga::node_calculate_layout(
                    (h as i64).max(0) as u32,
                    w as f32,
                    ht as f32,
                    d as i32,
                );
            }
        }
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_yoga_noop(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn qjs_yoga_module_init(
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

        macro_rules! export_i32 {
            ($name:literal, $value:expr) => {{
                let k = concat!($name, "\0");
                let _ = qjs::JS_SetModuleExport(ctx, m, k.as_ptr() as *const c_char, js_i32(ctx, $value));
            }};
        }

        export_fn!("configCreate", qjs_yoga_config_create, 0);
        export_fn!("configFree", qjs_yoga_config_free, 1);
        export_fn!(
            "configSetUseWebDefaults",
            qjs_yoga_config_set_use_web_defaults,
            2
        );
        export_fn!("nodeCreate", qjs_yoga_node_create, 1);
        export_fn!("nodeFreeRecursive", qjs_yoga_node_free_recursive, 1);
        export_fn!("nodeInsertChild", qjs_yoga_node_insert_child, 3);
        export_fn!("nodeGetChildCount", qjs_yoga_node_get_child_count, 1);
        export_fn!("nodeSetFlexDirection", qjs_yoga_node_set_flex_direction, 2);
        export_fn!("nodeSetAlignItems", qjs_yoga_node_set_align_items, 2);
        export_fn!("nodeSetAlignSelf", qjs_yoga_node_set_align_self, 2);
        export_fn!("nodeSetJustifyContent", qjs_yoga_node_set_justify_content, 2);
        export_fn!("nodeSetFlexWrap", qjs_yoga_node_set_flex_wrap, 2);
        export_fn!("nodeSetFlexGrow", qjs_yoga_node_set_flex_grow, 2);
        export_fn!("nodeSetFlexShrink", qjs_yoga_node_set_flex_shrink, 2);
        export_fn!("nodeSetPositionType", qjs_yoga_node_set_position_type, 2);
        export_fn!("nodeSetWidth", qjs_yoga_node_set_width, 2);
        export_fn!("nodeSetHeight", qjs_yoga_node_set_height, 2);
        export_fn!("nodeSetMinWidth", qjs_yoga_node_set_min_width, 2);
        export_fn!("nodeSetMinHeight", qjs_yoga_node_set_min_height, 2);
        export_fn!("nodeSetPadding", qjs_yoga_node_set_padding, 3);
        export_fn!("nodeSetMargin", qjs_yoga_node_set_margin, 3);
        export_fn!("nodeSetPosition", qjs_yoga_node_set_position, 3);
        export_fn!("nodeCalculateLayout", qjs_yoga_node_calculate_layout, 4);
        export_fn!("nodeGetComputedLeft", qjs_yoga_node_get_computed_left, 1);
        export_fn!("nodeGetComputedTop", qjs_yoga_node_get_computed_top, 1);
        export_fn!("nodeGetComputedWidth", qjs_yoga_node_get_computed_width, 1);
        export_fn!("nodeGetComputedHeight", qjs_yoga_node_get_computed_height, 1);
        export_fn!("nodeSetMeasureFunc", qjs_yoga_noop, 2);

        // Yoga enum values used by JS bridge.
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

    let m = qjs::JS_NewCModule(ctx, module_name, Some(qjs_yoga_module_init));
    if m.is_null() {
        return core::ptr::null_mut();
    }

    macro_rules! add_export {
        ($name:literal) => {{
            let k = concat!($name, "\0");
            let _ = qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char);
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
