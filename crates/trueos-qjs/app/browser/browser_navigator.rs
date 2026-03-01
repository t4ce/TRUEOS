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

unsafe extern "C" fn qjs_browser_navigator_get_user_agent(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { js_new_string(ctx, "Mozilla/5.0 (TRUEOS; QuickJS) AppleWebKit/537.36 (KHTML, like Gecko) TRUEOSBrowser/1.0") }
}

unsafe extern "C" fn qjs_browser_navigator_get_platform(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { js_new_string(ctx, "TRUEOS") }
}

unsafe extern "C" fn qjs_browser_navigator_get_language(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { js_new_string(ctx, "en-US") }
}

unsafe extern "C" fn qjs_browser_navigator_get_vendor(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { js_new_string(ctx, "TRUEOS") }
}

unsafe extern "C" fn qjs_browser_navigator_get_hardware_concurrency(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    unsafe { qjs::JS_NewFloat64(ctx, 1.0) }
}

unsafe extern "C" fn qjs_browser_navigator_is_online(
    _ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    _argc: i32,
    _argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    js_bool(true)
}

pub unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(module_name).to_bytes() };
    if name != b"trueos:browser_navigator" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn qjs_browser_navigator_module_init(
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

        export_fn!("getUserAgent", qjs_browser_navigator_get_user_agent, 0);
        export_fn!("getPlatform", qjs_browser_navigator_get_platform, 0);
        export_fn!("getLanguage", qjs_browser_navigator_get_language, 0);
        export_fn!("getVendor", qjs_browser_navigator_get_vendor, 0);
        export_fn!(
            "getHardwareConcurrency",
            qjs_browser_navigator_get_hardware_concurrency,
            0
        );
        export_fn!("isOnline", qjs_browser_navigator_is_online, 0);
        0
    }

    let m = unsafe {
        qjs::JS_NewCModule(ctx, module_name, Some(qjs_browser_navigator_module_init))
    };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    macro_rules! add_export {
        ($name:literal) => {{
            let k = concat!($name, "\0");
            let _ = qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char);
        }};
    }

    add_export!("getUserAgent");
    add_export!("getPlatform");
    add_export!("getLanguage");
    add_export!("getVendor");
    add_export!("getHardwareConcurrency");
    add_export!("isOnline");

    m
}
