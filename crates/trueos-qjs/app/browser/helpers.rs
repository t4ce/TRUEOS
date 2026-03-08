#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;
use core::ffi::c_char;

use crate as qjs;

pub fn js_single_quoted_literal(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 32);
    out.push('\'');
    for ch in src.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

pub unsafe fn eval_or_log(
    ctx: *mut qjs::JSContext,
    src: &[u8],
    filename: *const c_char,
    flags: i32,
    label: &str,
) -> bool {
    let val = qjs::js_eval_bytes(ctx, src, filename, flags);
    if val.is_exception() {
        qjs::trueos_shims::log_error("qjs-browser: ");
        qjs::trueos_shims::log_error(label);
        qjs::trueos_shims::log_error(" JS_Eval exception\n");
        qjs::qjs_diag::dump_last_exception(ctx, "browser eval");
        return false;
    }
    qjs::js_free_value(ctx, val);
    true
}
