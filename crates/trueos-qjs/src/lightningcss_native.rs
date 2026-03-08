#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;
use core::ffi::{CStr, c_char};

use crate as qjs;

#[cfg(feature = "lightningcss-native")]
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleAttribute, StyleSheet};

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
fn js_str(ctx: *mut qjs::JSContext, s: &str) -> qjs::JSValue {
    unsafe { qjs::JS_NewStringLen(ctx, s.as_ptr() as *const c_char, s.len()) }
}

#[inline]
unsafe fn set_prop_str(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &str, val: qjs::JSValue) {
    let mut keyz = String::from(key);
    keyz.push('\0');
    let _ = unsafe { qjs::JS_SetPropertyStr(ctx, obj, keyz.as_ptr() as *const c_char, val) };
}

#[inline]
unsafe fn parse_arg0_string(ctx: *mut qjs::JSContext, argc: i32, argv: *const qjs::JSValueConst) -> String {
    if argc <= 0 || argv.is_null() {
        return String::new();
    }
    let args = unsafe { core::slice::from_raw_parts(argv, argc as usize) };
    let mut len: usize = 0;
    let c = unsafe { qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, args[0], 0) };
    if c.is_null() {
        return String::new();
    }
    let bytes = unsafe { core::slice::from_raw_parts(c as *const u8, len) };
    let out = match core::str::from_utf8(bytes) {
        Ok(s) => String::from(s),
        Err(_) => String::new(),
    };
    unsafe { qjs::JS_FreeCString(ctx, c) };
    out
}

fn minified_inline_css(input: &str) -> Result<String, &'static str> {
    #[cfg(feature = "lightningcss-native")]
    {
        let mut style = StyleAttribute::parse(input, ParserOptions::default()).map_err(|_| "parse failed")?;
        style.minify(MinifyOptions::default());
        let out = style
            .to_css(PrinterOptions::default())
            .map_err(|_| "serialize failed")?;
        return Ok(out.code);
    }

    #[cfg(not(feature = "lightningcss-native"))]
    {
        let _ = input;
        Err("lightningcss backend disabled")
    }
}

fn minified_stylesheet_css(input: &str) -> Result<String, &'static str> {
    #[cfg(feature = "lightningcss-native")]
    {
        let mut sheet =
            StyleSheet::parse(input, ParserOptions::default()).map_err(|_| "parse failed")?;
        sheet.minify(MinifyOptions::default()).map_err(|_| "minify failed")?;
        let out = sheet
            .to_css(PrinterOptions::default())
            .map_err(|_| "serialize failed")?;
        return Ok(out.code);
    }

    #[cfg(not(feature = "lightningcss-native"))]
    {
        let _ = input;
        Err("lightningcss backend disabled")
    }
}

unsafe fn declarations_to_js(ctx: *mut qjs::JSContext, css: &str) -> qjs::JSValue {
    let arr = unsafe { qjs::JS_NewArray(ctx) };
    if arr.is_exception() {
        return arr;
    }

    let mut idx = 0u32;
    for part in css.split(';') {
        let s = part.trim();
        if s.is_empty() {
            continue;
        }
        let (name, value) = match s.split_once(':') {
            Some((n, v)) => (n.trim(), v.trim()),
            None => continue,
        };
        if name.is_empty() || value.is_empty() {
            continue;
        }

        let decl = unsafe { qjs::JS_NewObject(ctx) };
        if decl.is_exception() {
            continue;
        }
        unsafe { set_prop_str(ctx, decl, "name", js_str(ctx, name)) };
        unsafe { set_prop_str(ctx, decl, "value", js_str(ctx, value)) };
        let _ = unsafe { qjs::JS_SetPropertyUint32(ctx, arr, idx, decl) };
        idx += 1;
    }
    arr
}

unsafe extern "C" fn qjs_lightningcss_parse_inline_style(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let input = unsafe { parse_arg0_string(ctx, argc, argv) };

    let out = unsafe { qjs::JS_NewObject(ctx) };
    if out.is_exception() {
        return out;
    }

    match minified_inline_css(&input) {
        Ok(css) => {
            unsafe { set_prop_str(ctx, out, "ok", js_bool(true)) };
            unsafe { set_prop_str(ctx, out, "css", js_str(ctx, &css)) };
            let decls = unsafe { declarations_to_js(ctx, &css) };
            unsafe { set_prop_str(ctx, out, "declarations", decls) };
        }
        Err(msg) => {
            unsafe { set_prop_str(ctx, out, "ok", js_bool(false)) };
            unsafe { set_prop_str(ctx, out, "error", js_str(ctx, msg)) };
            let empty = unsafe { qjs::JS_NewArray(ctx) };
            unsafe { set_prop_str(ctx, out, "declarations", empty) };
        }
    }

    out
}

unsafe extern "C" fn qjs_lightningcss_parse_stylesheet(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let input = unsafe { parse_arg0_string(ctx, argc, argv) };

    let out = unsafe { qjs::JS_NewObject(ctx) };
    if out.is_exception() {
        return out;
    }

    match minified_stylesheet_css(&input) {
        Ok(css) => {
            unsafe { set_prop_str(ctx, out, "ok", js_bool(true)) };
            unsafe { set_prop_str(ctx, out, "css", js_str(ctx, &css)) };
        }
        Err(msg) => {
            unsafe { set_prop_str(ctx, out, "ok", js_bool(false)) };
            unsafe { set_prop_str(ctx, out, "error", js_str(ctx, msg)) };
        }
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
    if name != b"trueos:lightningcss" && name != b"lightningcss-native" {
        return core::ptr::null_mut();
    }

    unsafe extern "C" fn lightningcss_module_init(
        ctx: *mut qjs::JSContext,
        m: *mut qjs::JSModuleDef,
    ) -> i32 {
        let parse_name = b"parseInlineStyle\0";
        let parse_fn = unsafe {
            qjs::JS_NewCFunction2(
                ctx,
                Some(qjs_lightningcss_parse_inline_style),
                parse_name.as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            )
        };
        let _ = unsafe { qjs::JS_SetModuleExport(ctx, m, parse_name.as_ptr() as *const c_char, parse_fn) };

        let parse_sheet_name = b"parseStylesheet\0";
        let parse_sheet_fn = unsafe {
            qjs::JS_NewCFunction2(
                ctx,
                Some(qjs_lightningcss_parse_stylesheet),
                parse_sheet_name.as_ptr() as *const c_char,
                1,
                qjs::JS_CFUNC_GENERIC,
                0,
            )
        };
        let _ = unsafe {
            qjs::JS_SetModuleExport(
                ctx,
                m,
                parse_sheet_name.as_ptr() as *const c_char,
                parse_sheet_fn,
            )
        };

        let avail_name = b"isAvailable\0";
        #[cfg(feature = "lightningcss-native")]
        let avail = js_bool(true);
        #[cfg(not(feature = "lightningcss-native"))]
        let avail = js_bool(false);
        let _ = unsafe { qjs::JS_SetModuleExport(ctx, m, avail_name.as_ptr() as *const c_char, avail) };
        0
    }

    let m = unsafe { qjs::JS_NewCModule(ctx, module_name, Some(lightningcss_module_init)) };
    if m.is_null() {
        return core::ptr::null_mut();
    }

    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"parseInlineStyle\0".as_ptr() as *const c_char) };
    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"parseStylesheet\0".as_ptr() as *const c_char) };
    let _ = unsafe { qjs::JS_AddModuleExport(ctx, m, b"isAvailable\0".as_ptr() as *const c_char) };
    m
}
