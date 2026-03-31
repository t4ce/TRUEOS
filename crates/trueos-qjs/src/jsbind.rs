#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::ffi::c_char;
use core::marker::PhantomData;

use crate as qjs;

pub struct JsStringRef<'a> {
    ctx: *mut qjs::JSContext,
    ptr: *const c_char,
    len: usize,
    _marker: PhantomData<&'a qjs::JSContext>,
}

impl<'a> JsStringRef<'a> {
    pub unsafe fn new(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<Self> {
        let mut len = 0usize;
        let ptr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, v, 0);
        if ptr.is_null() {
            return None;
        }
        Some(Self {
            ctx,
            ptr,
            len,
            _marker: PhantomData,
        })
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr as *const u8, self.len) }
    }

    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(self.as_bytes()).ok()
    }
}

impl Drop for JsStringRef<'_> {
    fn drop(&mut self) {
        unsafe {
            qjs::JS_FreeCString(self.ctx, self.ptr);
        }
    }
}

#[inline]
pub unsafe fn new_string(ctx: *mut qjs::JSContext, bytes: &[u8]) -> qjs::JSValue {
    qjs::JS_NewStringLen(ctx, bytes.as_ptr() as *const c_char, bytes.len())
}

#[inline]
pub unsafe fn set_prop(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
    val: qjs::JSValue,
) -> bool {
    qjs::JS_SetPropertyStr(ctx, obj, key.as_ptr() as *const c_char, val) >= 0
}

#[inline]
pub unsafe fn set_str_prop(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
    value: &str,
) -> bool {
    let js = new_string(ctx, value.as_bytes());
    set_prop(ctx, obj, key, js)
}

#[inline]
pub unsafe fn new_c_function(
    ctx: *mut qjs::JSContext,
    name: &[u8],
    argc: i32,
    func: Option<qjs::JSCFunction>,
) -> qjs::JSValue {
    qjs::JS_NewCFunction2(ctx, func, name.as_ptr() as *const c_char, argc, qjs::JS_CFUNC_GENERIC, 0)
}

#[inline]
pub unsafe fn install_fn(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    name: &[u8],
    argc: i32,
    func: Option<qjs::JSCFunction>,
) -> bool {
    set_prop(ctx, obj, name, new_c_function(ctx, name, argc, func))
}

#[inline]
pub unsafe fn call(
    ctx: *mut qjs::JSContext,
    func: qjs::JSValueConst,
    this_obj: qjs::JSValueConst,
    args: &[qjs::JSValueConst],
) -> qjs::JSValue {
    qjs::JS_Call(ctx, func, this_obj, args.len() as i32, args.as_ptr())
}

#[inline]
pub unsafe fn call1(
    ctx: *mut qjs::JSContext,
    func: qjs::JSValueConst,
    this_obj: qjs::JSValueConst,
    arg: qjs::JSValueConst,
) -> qjs::JSValue {
    let args = [arg];
    call(ctx, func, this_obj, &args)
}

pub unsafe fn to_bytes(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<Vec<u8>> {
    let s = JsStringRef::new(ctx, v)?;
    Some(s.as_bytes().to_vec())
}

pub unsafe fn to_string(ctx: *mut qjs::JSContext, v: qjs::JSValueConst) -> Option<String> {
    let bytes = to_bytes(ctx, v)?;
    String::from_utf8(bytes).ok()
}
