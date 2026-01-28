#![no_std]

use core::ffi::{c_char, c_int, c_void};

#[cfg(feature = "trueos")]
pub mod trueos_smoke;

#[cfg(feature = "trueos")]
pub mod trueos_modules;

#[cfg(feature = "trueos")]
pub mod node;

#[cfg(feature = "trueos")]
pub mod trueos_shims;

#[repr(C)]
pub struct JSRuntime {
    _private: [u8; 0],
}

#[repr(C)]
pub struct JSContext {
    _private: [u8; 0],
}

#[repr(C)]
pub struct JSRefCountHeader {
    pub ref_count: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union JSValueUnion {
    pub int32: i32,
    pub float64: f64,
    pub ptr: *mut c_void,
    pub short_big_int: i64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct JSValue {
    pub u: JSValueUnion,
    pub tag: i64,
}

pub type JSValueConst = JSValue;

pub const JS_TAG_FIRST: i64 = -9;
pub const JS_TAG_MODULE: i64 = -3;
pub const JS_TAG_OBJECT: i64 = -1;
pub const JS_TAG_INT: i64 = 0;
pub const JS_TAG_BOOL: i64 = 1;
pub const JS_TAG_NULL: i64 = 2;
pub const JS_TAG_UNDEFINED: i64 = 3;
pub const JS_TAG_EXCEPTION: i64 = 6;
pub const JS_TAG_FLOAT64: i64 = 8;

pub const JS_EVAL_TYPE_GLOBAL: c_int = 0;
pub const JS_EVAL_TYPE_MODULE: c_int = 1;

pub const JS_EVAL_FLAG_COMPILE_ONLY: c_int = 1 << 5;

pub const JS_CFUNC_GENERIC: c_int = 0;

impl JSValue {
    #[inline]
    pub fn undefined() -> Self {
        Self {
            u: JSValueUnion { int32: 0 },
            tag: JS_TAG_UNDEFINED,
        }
    }

    #[inline]
    pub fn is_exception(self) -> bool {
        self.tag == JS_TAG_EXCEPTION
    }

    #[inline]
    pub fn exception() -> Self {
        Self {
            u: JSValueUnion { int32: 0 },
            tag: JS_TAG_EXCEPTION,
        }
    }
}

/// Mirrors the `JS_FreeValue` inline in `quickjs.h` for the non-NAN-boxing (PTR64) ABI.
///
/// Safety: `ctx` must be a live context and `v` must be a valid value for that context.
#[inline]
pub unsafe fn js_free_value(ctx: *mut JSContext, v: JSValue) {
    if v.tag >= JS_TAG_FIRST && v.tag < 0 {
        let p = unsafe { v.u.ptr } as *mut JSRefCountHeader;
        unsafe {
            (*p).ref_count -= 1;
            if (*p).ref_count <= 0 {
                __JS_FreeValue(ctx, v);
            }
        }
    }
}

/// Convenience wrapper for `JS_ToCStringLen2(..., cesu8=false)`.
#[inline]
pub unsafe fn js_to_cstring(ctx: *mut JSContext, val: JSValueConst) -> *const c_char {
    JS_ToCStringLen2(ctx, core::ptr::null_mut(), val, 0)
}

/// Mirrors the `JS_NewFloat64` static inline in `quickjs.h` (PTR64 ABI).
///
/// Note: this is not an exported C symbol, so it must not be declared `extern`.
#[inline]
pub unsafe fn JS_NewFloat64(_ctx: *mut JSContext, d: f64) -> JSValue {
    JSValue {
        u: JSValueUnion { float64: d },
        tag: JS_TAG_FLOAT64,
    }
}

pub type JSCFunction = unsafe extern "C" fn(
    ctx: *mut JSContext,
    this_val: JSValueConst,
    argc: c_int,
    argv: *const JSValueConst,
) -> JSValue;

#[repr(C)]
pub struct JSModuleDef {
    _private: [u8; 0],
}

pub type JSModuleInitFunc = unsafe extern "C" fn(ctx: *mut JSContext, m: *mut JSModuleDef) -> c_int;
pub type JSModuleNormalizeFunc = unsafe extern "C" fn(
    ctx: *mut JSContext,
    module_base_name: *const c_char,
    module_name: *const c_char,
    opaque: *mut c_void,
) -> *mut c_char;
pub type JSModuleLoaderFunc = unsafe extern "C" fn(
    ctx: *mut JSContext,
    module_name: *const c_char,
    opaque: *mut c_void,
) -> *mut JSModuleDef;

extern "C" {
    pub fn JS_NewRuntime() -> *mut JSRuntime;
    pub fn JS_FreeRuntime(rt: *mut JSRuntime);
    pub fn JS_NewContext(rt: *mut JSRuntime) -> *mut JSContext;
    pub fn JS_FreeContext(ctx: *mut JSContext);

    pub fn JS_Eval(
        ctx: *mut JSContext,
        input: *const c_char,
        input_len: usize,
        filename: *const c_char,
        eval_flags: c_int,
    ) -> JSValue;

    pub fn JS_EvalFunction(ctx: *mut JSContext, fun_obj: JSValue) -> JSValue;

    pub fn JS_SetModuleLoaderFunc(
        rt: *mut JSRuntime,
        module_normalize: Option<JSModuleNormalizeFunc>,
        module_loader: Option<JSModuleLoaderFunc>,
        opaque: *mut c_void,
    );

    pub fn JS_NewCModule(
        ctx: *mut JSContext,
        name_str: *const c_char,
        func: Option<JSModuleInitFunc>,
    ) -> *mut JSModuleDef;
    pub fn JS_AddModuleExport(ctx: *mut JSContext, m: *mut JSModuleDef, name_str: *const c_char) -> c_int;
    pub fn JS_SetModuleExport(
        ctx: *mut JSContext,
        m: *mut JSModuleDef,
        export_name: *const c_char,
        val: JSValue,
    ) -> c_int;

    pub fn JS_GetGlobalObject(ctx: *mut JSContext) -> JSValue;

    pub fn JS_NewObject(ctx: *mut JSContext) -> JSValue;
    pub fn JS_NewArray(ctx: *mut JSContext) -> JSValue;
    pub fn JS_GetPropertyStr(ctx: *mut JSContext, this_obj: JSValueConst, prop: *const c_char) -> JSValue;

    pub fn JS_ToFloat64(ctx: *mut JSContext, pres: *mut f64, val: JSValueConst) -> c_int;
    pub fn JS_SetPropertyStr(
        ctx: *mut JSContext,
        this_obj: JSValueConst,
        prop: *const c_char,
        val: JSValue,
    ) -> c_int;

    pub fn JS_SetPropertyUint32(ctx: *mut JSContext, this_obj: JSValueConst, idx: u32, val: JSValue) -> c_int;

    pub fn JS_Call(
        ctx: *mut JSContext,
        func_obj: JSValueConst,
        this_obj: JSValueConst,
        argc: c_int,
        argv: *const JSValueConst,
    ) -> JSValue;

    pub fn JS_NewCFunction2(
        ctx: *mut JSContext,
        func: Option<JSCFunction>,
        name: *const c_char,
        length: c_int,
        cproto: c_int,
        magic: c_int,
    ) -> JSValue;

    pub fn JS_GetException(ctx: *mut JSContext) -> JSValue;

    pub fn JS_ToCStringLen2(
        ctx: *mut JSContext,
        plen: *mut usize,
        val1: JSValueConst,
        cesu8: c_int,
    ) -> *const c_char;
    pub fn JS_FreeCString(ctx: *mut JSContext, ptr: *const c_char);

    pub fn JS_NewStringLen(ctx: *mut JSContext, buf: *const c_char, buf_len: usize) -> JSValue;

    pub fn JS_NewArrayBufferCopy(ctx: *mut JSContext, buf: *const u8, len: usize) -> JSValue;

    pub fn JS_NewError(ctx: *mut JSContext) -> JSValue;
    pub fn JS_Throw(ctx: *mut JSContext, obj: JSValue) -> JSValue;
    pub fn JS_IsJobPending(rt: *mut JSRuntime) -> c_int;
    pub fn JS_ExecutePendingJob(rt: *mut JSRuntime, pctx: *mut *mut JSContext) -> c_int;

    pub fn js_malloc(ctx: *mut JSContext, size: usize) -> *mut c_void;
    pub fn js_free(ctx: *mut JSContext, ptr: *mut c_void);

    pub fn __JS_FreeValue(ctx: *mut JSContext, v: JSValue);
}
