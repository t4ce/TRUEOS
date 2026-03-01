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
fn js_f64(ctx: *mut qjs::JSContext, v: f64) -> qjs::JSValue {
    unsafe { qjs::JS_NewFloat64(ctx, v) }
}

#[inline]
unsafe fn arg_f64(ctx: *mut qjs::JSContext, argc: i32, argv: *const qjs::JSValueConst, idx: usize) -> f64 {
    if argc <= 0 || argv.is_null() || idx >= argc as usize {
        return 0.0;
    }
    let mut out = 0.0f64;
    let _ = unsafe { qjs::JS_ToFloat64(ctx, &mut out as *mut f64, *argv.add(idx)) };
    out
}

#[inline]
unsafe fn arg_i32(ctx: *mut qjs::JSContext, argc: i32, argv: *const qjs::JSValueConst, idx: usize) -> i32 {
    unsafe { arg_f64(ctx, argc, argv, idx) as i32 }
}

#[inline]
unsafe fn arg_u32(ctx: *mut qjs::JSContext, argc: i32, argv: *const qjs::JSValueConst, idx: usize) -> u32 {
    let v = unsafe { arg_f64(ctx, argc, argv, idx) };
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    v as u32
}

#[inline]
unsafe fn arg_bool(ctx: *mut qjs::JSContext, argc: i32, argv: *const qjs::JSValueConst, idx: usize) -> bool {
    unsafe { arg_f64(ctx, argc, argv, idx) != 0.0 }
}

#[cfg(feature = "yoga-native")]
mod backend {
    pub(crate) use crate::trueos_shims::yoga::{
        config_create, config_free, config_set_use_web_defaults, node_calculate_layout, node_create,
        node_free_recursive, node_get_child_count, node_get_computed_height, node_get_computed_left,
        node_get_computed_top, node_get_computed_width, node_insert_child, node_set_align_items,
        node_set_align_self, node_set_flex_direction, node_set_flex_grow, node_set_flex_shrink,
        node_set_flex_wrap, node_set_height, node_set_justify_content, node_set_margin,
        node_set_min_height, node_set_min_width, node_set_padding, node_set_position,
        node_set_position_type, node_set_width,
    };
}

#[cfg(not(feature = "yoga-native"))]
mod backend {
    pub(crate) fn config_create() -> u32 {
        0
    }
    pub(crate) fn config_free(_handle: u32) {}
    pub(crate) fn config_set_use_web_defaults(_handle: u32, _enabled: bool) {}

    pub(crate) fn node_create(_config_handle: u32) -> u32 {
        0
    }
    pub(crate) fn node_free_recursive(_handle: u32) {}
    pub(crate) fn node_insert_child(_parent: u32, _child: u32, _index: u32) {}
    pub(crate) fn node_get_child_count(_handle: u32) -> u32 {
        0
    }
    pub(crate) fn node_calculate_layout(_handle: u32, _width: f32, _height: f32, _direction: i32) {}

    pub(crate) fn node_set_flex_direction(_handle: u32, _v: i32) {}
    pub(crate) fn node_set_align_items(_handle: u32, _v: i32) {}
    pub(crate) fn node_set_align_self(_handle: u32, _v: i32) {}
    pub(crate) fn node_set_justify_content(_handle: u32, _v: i32) {}
    pub(crate) fn node_set_flex_wrap(_handle: u32, _v: i32) {}
    pub(crate) fn node_set_flex_grow(_handle: u32, _v: f32) {}
    pub(crate) fn node_set_flex_shrink(_handle: u32, _v: f32) {}
    pub(crate) fn node_set_position_type(_handle: u32, _v: i32) {}

    pub(crate) fn node_set_width(_handle: u32, _v: f32) {}
    pub(crate) fn node_set_height(_handle: u32, _v: f32) {}
    pub(crate) fn node_set_min_width(_handle: u32, _v: f32) {}
    pub(crate) fn node_set_min_height(_handle: u32, _v: f32) {}
    pub(crate) fn node_set_padding(_handle: u32, _edge: i32, _v: f32) {}
    pub(crate) fn node_set_margin(_handle: u32, _edge: i32, _v: f32) {}
    pub(crate) fn node_set_position(_handle: u32, _edge: i32, _v: f32) {}

    pub(crate) fn node_get_computed_left(_handle: u32) -> f32 {
        0.0
    }
    pub(crate) fn node_get_computed_top(_handle: u32) -> f32 {
        0.0
    }
    pub(crate) fn node_get_computed_width(_handle: u32) -> f32 {
        0.0
    }
    pub(crate) fn node_get_computed_height(_handle: u32) -> f32 {
        0.0
    }
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

    unsafe extern "C" fn yoga_config_create(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_u32(ctx, backend::config_create())
    }

    unsafe extern "C" fn yoga_config_free(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        backend::config_free(unsafe { arg_u32(_ctx, argc, argv, 0) });
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_config_set_use_web_defaults(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let h = unsafe { arg_u32(ctx, argc, argv, 0) };
        let enabled = unsafe { arg_bool(ctx, argc, argv, 1) };
        backend::config_set_use_web_defaults(h, enabled);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_create(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let cfg = unsafe { arg_u32(ctx, argc, argv, 0) };
        js_u32(ctx, backend::node_create(cfg))
    }

    unsafe extern "C" fn yoga_node_free_recursive(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        backend::node_free_recursive(unsafe { arg_u32(ctx, argc, argv, 0) });
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_insert_child(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let parent = unsafe { arg_u32(ctx, argc, argv, 0) };
        let child = unsafe { arg_u32(ctx, argc, argv, 1) };
        let index = unsafe { arg_u32(ctx, argc, argv, 2) };
        backend::node_insert_child(parent, child, index);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_get_child_count(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_u32(
            ctx,
            backend::node_get_child_count(unsafe { arg_u32(ctx, argc, argv, 0) }),
        )
    }

    unsafe extern "C" fn yoga_node_set_i32(
        ctx: *mut qjs::JSContext,
        argc: i32,
        argv: *const qjs::JSValueConst,
        f: fn(u32, i32),
    ) {
        let h = unsafe { arg_u32(ctx, argc, argv, 0) };
        let v = unsafe { arg_i32(ctx, argc, argv, 1) };
        f(h, v);
    }

    unsafe extern "C" fn yoga_node_set_f32(
        ctx: *mut qjs::JSContext,
        argc: i32,
        argv: *const qjs::JSValueConst,
        f: fn(u32, f32),
    ) {
        let h = unsafe { arg_u32(ctx, argc, argv, 0) };
        let v = unsafe { arg_f64(ctx, argc, argv, 1) as f32 };
        f(h, v);
    }

    unsafe extern "C" fn yoga_node_set_edge_f32(
        ctx: *mut qjs::JSContext,
        argc: i32,
        argv: *const qjs::JSValueConst,
        f: fn(u32, i32, f32),
    ) {
        let h = unsafe { arg_u32(ctx, argc, argv, 0) };
        let edge = unsafe { arg_i32(ctx, argc, argv, 1) };
        let v = unsafe { arg_f64(ctx, argc, argv, 2) as f32 };
        f(h, edge, v);
    }

    unsafe extern "C" fn yoga_node_set_flex_direction(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_i32(ctx, argc, argv, backend::node_set_flex_direction) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_align_items(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_i32(ctx, argc, argv, backend::node_set_align_items) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_align_self(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_i32(ctx, argc, argv, backend::node_set_align_self) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_justify_content(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_i32(ctx, argc, argv, backend::node_set_justify_content) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_flex_wrap(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_i32(ctx, argc, argv, backend::node_set_flex_wrap) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_flex_grow(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_f32(ctx, argc, argv, backend::node_set_flex_grow) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_flex_shrink(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_f32(ctx, argc, argv, backend::node_set_flex_shrink) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_position_type(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_i32(ctx, argc, argv, backend::node_set_position_type) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_width(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_f32(ctx, argc, argv, backend::node_set_width) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_height(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_f32(ctx, argc, argv, backend::node_set_height) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_min_width(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_f32(ctx, argc, argv, backend::node_set_min_width) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_min_height(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_f32(ctx, argc, argv, backend::node_set_min_height) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_padding(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_edge_f32(ctx, argc, argv, backend::node_set_padding) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_margin(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_edge_f32(ctx, argc, argv, backend::node_set_margin) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_set_position(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        unsafe { yoga_node_set_edge_f32(ctx, argc, argv, backend::node_set_position) };
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_calculate_layout(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        let handle = unsafe { arg_u32(ctx, argc, argv, 0) };
        let w = unsafe { arg_f64(ctx, argc, argv, 1) as f32 };
        let h = unsafe { arg_f64(ctx, argc, argv, 2) as f32 };
        let dir = unsafe { arg_i32(ctx, argc, argv, 3) };
        backend::node_calculate_layout(handle, w, h, dir);
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn yoga_node_get_computed_left(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_f64(
            ctx,
            backend::node_get_computed_left(unsafe { arg_u32(ctx, argc, argv, 0) }) as f64,
        )
    }

    unsafe extern "C" fn yoga_node_get_computed_top(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_f64(
            ctx,
            backend::node_get_computed_top(unsafe { arg_u32(ctx, argc, argv, 0) }) as f64,
        )
    }

    unsafe extern "C" fn yoga_node_get_computed_width(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_f64(
            ctx,
            backend::node_get_computed_width(unsafe { arg_u32(ctx, argc, argv, 0) }) as f64,
        )
    }

    unsafe extern "C" fn yoga_node_get_computed_height(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: i32,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        js_f64(
            ctx,
            backend::node_get_computed_height(unsafe { arg_u32(ctx, argc, argv, 0) }) as f64,
        )
    }

    // We do not bridge JS measure callbacks yet; keep API-compatible no-op.
    unsafe extern "C" fn yoga_node_set_measure_func(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue::undefined()
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

        export_fn!("configCreate", yoga_config_create, 0);
        export_fn!("configFree", yoga_config_free, 1);
        export_fn!("configSetUseWebDefaults", yoga_config_set_use_web_defaults, 2);
        export_fn!("nodeCreate", yoga_node_create, 1);
        export_fn!("nodeFreeRecursive", yoga_node_free_recursive, 1);
        export_fn!("nodeInsertChild", yoga_node_insert_child, 3);
        export_fn!("nodeGetChildCount", yoga_node_get_child_count, 1);
        export_fn!("nodeSetFlexDirection", yoga_node_set_flex_direction, 2);
        export_fn!("nodeSetAlignItems", yoga_node_set_align_items, 2);
        export_fn!("nodeSetAlignSelf", yoga_node_set_align_self, 2);
        export_fn!("nodeSetJustifyContent", yoga_node_set_justify_content, 2);
        export_fn!("nodeSetFlexWrap", yoga_node_set_flex_wrap, 2);
        export_fn!("nodeSetFlexGrow", yoga_node_set_flex_grow, 2);
        export_fn!("nodeSetFlexShrink", yoga_node_set_flex_shrink, 2);
        export_fn!("nodeSetPositionType", yoga_node_set_position_type, 2);
        export_fn!("nodeSetWidth", yoga_node_set_width, 2);
        export_fn!("nodeSetHeight", yoga_node_set_height, 2);
        export_fn!("nodeSetMinWidth", yoga_node_set_min_width, 2);
        export_fn!("nodeSetMinHeight", yoga_node_set_min_height, 2);
        export_fn!("nodeSetPadding", yoga_node_set_padding, 3);
        export_fn!("nodeSetMargin", yoga_node_set_margin, 3);
        export_fn!("nodeSetPosition", yoga_node_set_position, 3);
        export_fn!("nodeCalculateLayout", yoga_node_calculate_layout, 4);
        export_fn!("nodeGetComputedLeft", yoga_node_get_computed_left, 1);
        export_fn!("nodeGetComputedTop", yoga_node_get_computed_top, 1);
        export_fn!("nodeGetComputedWidth", yoga_node_get_computed_width, 1);
        export_fn!("nodeGetComputedHeight", yoga_node_get_computed_height, 1);
        export_fn!("nodeSetMeasureFunc", yoga_node_set_measure_func, 2);

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
