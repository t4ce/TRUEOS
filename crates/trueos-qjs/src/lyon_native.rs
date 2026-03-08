#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;
use core::ffi::{CStr, c_char};

use crate as qjs;

#[cfg(feature = "lyon-native")]
use lyon::geom::{CubicBezierSegment, LineSegment, QuadraticBezierSegment};
#[cfg(feature = "lyon-native")]
use lyon::math::point;

#[inline]
fn js_bool(v: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion {
            int32: if v { 1 } else { 0 },
        },
        tag: qjs::JS_TAG_BOOL,
    }
}

#[inline]
fn js_num(ctx: *mut qjs::JSContext, v: f64) -> qjs::JSValue {
    unsafe { qjs::JS_NewFloat64(ctx, v) }
}

#[inline]
fn js_str(ctx: *mut qjs::JSContext, s: &str) -> qjs::JSValue {
    unsafe { qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len()) }
}

#[inline]
unsafe fn set_prop_str(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &str, val: qjs::JSValue) {
    let mut keyz = String::from(key);
    keyz.push('\0');
    let _ = unsafe { qjs::JS_SetPropertyStr(ctx, obj, keyz.as_ptr() as *const c_char, val) };
}

fn triangle_area(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f64 {
    let area2 = a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1);
    (0.5f32 * area2.abs()) as f64
}

#[cfg(feature = "lyon-native")]
fn demo_shapes_values() -> (f64, f64, f64, f64, f64, f64) {
    let line = LineSegment {
        from: point(0.0, 0.0),
        to: point(40.0, 30.0),
    };
    let line_len = line.length() as f64;

    let quad = QuadraticBezierSegment {
        from: point(0.0, 0.0),
        ctrl: point(20.0, 36.0),
        to: point(44.0, 2.0),
    };
    let quad_mid = quad.sample(0.5);

    let cubic = CubicBezierSegment {
        from: point(0.0, 0.0),
        ctrl1: point(12.0, 34.0),
        ctrl2: point(38.0, -12.0),
        to: point(60.0, 10.0),
    };
    let cubic_mid = cubic.sample(0.5);

    let area = triangle_area((0.0, 0.0), (24.0, 0.0), (10.0, 18.0));

    (
        line_len,
        area,
        quad_mid.x as f64,
        quad_mid.y as f64,
        cubic_mid.x as f64,
        cubic_mid.y as f64,
    )
}

#[cfg(not(feature = "lyon-native"))]
fn demo_shapes_values() -> (f64, f64, f64, f64, f64, f64) {
    (0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
}

unsafe extern "C" fn qjs_lyon_demo_shapes(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let out = unsafe { qjs::JS_NewObject(ctx) };
    if out.is_exception() {
        return out;
    }

    #[cfg(feature = "lyon-native")]
    {
        let (line_len, tri_area, quad_x, quad_y, cubic_x, cubic_y) = demo_shapes_values();
        unsafe { set_prop_str(ctx, out, "ok", js_bool(true)) };
        unsafe { set_prop_str(ctx, out, "lineLength", js_num(ctx, line_len)) };
        unsafe { set_prop_str(ctx, out, "triangleArea", js_num(ctx, tri_area)) };
        unsafe { set_prop_str(ctx, out, "quadMidX", js_num(ctx, quad_x)) };
        unsafe { set_prop_str(ctx, out, "quadMidY", js_num(ctx, quad_y)) };
        unsafe { set_prop_str(ctx, out, "cubicMidX", js_num(ctx, cubic_x)) };
        unsafe { set_prop_str(ctx, out, "cubicMidY", js_num(ctx, cubic_y)) };
    }

    #[cfg(not(feature = "lyon-native"))]
    {
        unsafe { set_prop_str(ctx, out, "ok", js_bool(false)) };
        unsafe { set_prop_str(ctx, out, "error", js_str(ctx, "lyon backend disabled")) };
    }

    out
}

pub(crate) unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }

    let name = unsafe { CStr::from_ptr(module_name).to_bytes() };
    if name != b"trueos:lyon" && name != b"lyon-native" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn lyon_module_init(ctx: *mut qjs::JSContext, m: *mut qjs::JSModuleDef) -> i32 {
        let demo_name = b"demoShapes\0";
        let demo_fn = unsafe {
            qjs::JS_NewCFunction2(
                ctx,
                Some(qjs_lyon_demo_shapes),
                demo_name.as_ptr() as *const c_char,
                0,
                qjs::JS_CFUNC_GENERIC,
                0,
            )
        };
        let _ = unsafe { qjs::JS_SetModuleExport(ctx, m, demo_name.as_ptr() as *const c_char, demo_fn) };

        let avail_name = b"isAvailable\0";
        #[cfg(feature = "lyon-native")]
        let avail = js_bool(true);
        #[cfg(not(feature = "lyon-native"))]
        let avail = js_bool(false);
        let _ = unsafe { qjs::JS_SetModuleExport(ctx, m, avail_name.as_ptr() as *const c_char, avail) };
        0
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(lyon_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"demoShapes\0".as_ptr() as *const c_char) };
    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"isAvailable\0".as_ptr() as *const c_char) };
    m
}
