#![cfg(feature = "trueos")]

use core::ffi::{CStr, c_char};

use crate as qjs;

#[inline]
fn js_i32(ctx: *mut qjs::JSContext, v: i32) -> qjs::JSValue {
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
    if name != b"trueos:threejs" && name != b"threejs-native" && name != b"three" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn three_noop(
        _ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        _argc: i32,
        _argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        qjs::JSValue::undefined()
    }

    unsafe extern "C" fn three_module_init(
        ctx: *mut qjs::JSContext,
        m: *mut qjs::JSModuleDef,
    ) -> i32 {
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
                    qjs::JS_SetModuleExport(
                        ctx,
                        m,
                        k.as_ptr() as *const c_char,
                        js_i32(ctx, $value),
                    )
                };
            }};
        }

        export_fn!("init", three_noop, 0);
        export_i32!("IS_TRUEOS_NATIVE", 1);
        export_i32!("REVISION", 0);
        0
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(three_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    macro_rules! add_export {
        ($name:literal) => {{
            let k = concat!($name, "\0");
            let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char) };
        }};
    }

    add_export!("init");
    add_export!("IS_TRUEOS_NATIVE");
    add_export!("REVISION");

    m
}
