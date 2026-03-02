#![cfg(feature = "trueos")]

use core::ffi::{CStr, c_char};

use crate as qjs;

#[inline]
unsafe fn js_new_string(ctx: *mut qjs::JSContext, s: &str) -> qjs::JSValue {
    unsafe { qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len()) }
}

#[inline]
fn js_bool(v: bool) -> qjs::JSValue {
    qjs::JSValue {
        u: qjs::JSValueUnion {
            int32: if v { 1 } else { 0 },
        },
        tag: qjs::JS_TAG_BOOL,
    }
}

unsafe extern "C" fn qjs_browser_webgpu_is_available(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    js_bool(true)
}

unsafe extern "C" fn qjs_browser_webgpu_get_preferred_canvas_format(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { js_new_string(ctx, "bgra8unorm") }
}

unsafe extern "C" fn qjs_browser_webgpu_get_backend_name(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { js_new_string(ctx, "trueos-cmd-stream") }
}

pub unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(module_name).to_bytes() };
    if name != b"trueos:browser_webgpu" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn qjs_browser_webgpu_module_init(
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

        export_fn!("isAvailable", qjs_browser_webgpu_is_available, 0);
        export_fn!(
            "getPreferredCanvasFormat",
            qjs_browser_webgpu_get_preferred_canvas_format,
            0
        );
        export_fn!("getBackendName", qjs_browser_webgpu_get_backend_name, 0);
        0
    }

    let m =
        unsafe { qjs::JS_NewCModule(ctx, module_name, Some(qjs_browser_webgpu_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    macro_rules! add_export {
        ($name:literal) => {{
            let k = concat!($name, "\0");
            let _ = qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char);
        }};
    }

    add_export!("isAvailable");
    add_export!("getPreferredCanvasFormat");
    add_export!("getBackendName");

    m
}