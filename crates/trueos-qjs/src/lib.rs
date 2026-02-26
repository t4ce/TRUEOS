#![no_std]
#![allow(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::vec::Vec;
use core::ffi::{c_char, c_int, c_void};

#[cfg(feature = "trueos")]
pub mod async_ops;

#[cfg(feature = "trueos")]
pub mod async_fs;

#[cfg(feature = "trueos")]
pub mod trueos_module_loader;

#[cfg(feature = "trueos")]
pub mod browser;

#[cfg(feature = "trueos")]
pub mod timers;

#[cfg(feature = "trueos")]
pub mod node;

#[cfg(feature = "trueos")]
pub mod trueos_shims;

#[cfg(feature = "trueos")]
pub mod vm;

#[cfg(feature = "trueos")]
pub mod workers;

#[cfg(feature = "trueos")]
pub mod qjs_diag;

#[cfg(feature = "trueos")]
pub mod pixi;

#[cfg(feature = "trueos")]
pub mod threejs;

#[cfg(feature = "trueos")]
pub mod wgpu;

#[cfg(feature = "trueos")]
pub use pixi::hex as stream_gfx_smoke;

#[cfg(feature = "trueos")]
#[derive(Clone, Copy)]
pub struct FontAtlasView<'a> {
    pub alpha: &'a [u8],
    pub index: &'a [u16],
    pub widths: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub cell_w: u32,
    pub cell_h: u32,
    pub grid_w: u32,
    pub grid_h: u32,
}

#[cfg(feature = "trueos")]
pub type FontAtlasSmallProvider = fn() -> FontAtlasView<'static>;

#[cfg(feature = "trueos")]
static FONT_ATLAS_SMALL_PROVIDER: spin::Mutex<Option<FontAtlasSmallProvider>> =
    spin::Mutex::new(None);

#[cfg(feature = "trueos")]
pub fn set_font_atlas_small_provider(provider: FontAtlasSmallProvider) {
    *FONT_ATLAS_SMALL_PROVIDER.lock() = Some(provider);
}

#[cfg(feature = "trueos")]
pub fn font_atlas_small_view() -> Option<FontAtlasView<'static>> {
    FONT_ATLAS_SMALL_PROVIDER
        .lock()
        .as_ref()
        .copied()
        .map(|f| f())
}

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
pub const JS_READ_OBJ_BYTECODE: c_int = 1 << 0;
pub const JS_WRITE_OBJ_BYTECODE: c_int = 1 << 0;

pub const JS_CFUNC_GENERIC: c_int = 0;
// quickjs.h: JS_CFUNC_constructor
pub const JS_CFUNC_CONSTRUCTOR: c_int = 2;

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

/// Mirrors the `JS_DupValue` inline in `quickjs.h` for the non-NAN-boxing (PTR64) ABI.
///
/// Safety: `v` must be a valid value for the given runtime.
#[inline]
pub unsafe fn js_dup_value(_ctx: *mut JSContext, v: JSValue) -> JSValue {
    if v.tag >= JS_TAG_FIRST && v.tag < 0 {
        let p = unsafe { v.u.ptr } as *mut JSRefCountHeader;
        unsafe {
            (*p).ref_count += 1;
        }
    }
    v
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
#[allow(non_snake_case)]
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

pub type JSHostPromiseRejectionTracker = unsafe extern "C" fn(
    ctx: *mut JSContext,
    promise: JSValueConst,
    reason: JSValueConst,
    is_handled: c_int,
    opaque: *mut c_void,
);

unsafe extern "C" {
    pub fn JS_NewRuntime() -> *mut JSRuntime;
    pub fn JS_FreeRuntime(rt: *mut JSRuntime);
    pub fn JS_NewContext(rt: *mut JSRuntime) -> *mut JSContext;
    pub fn JS_FreeContext(ctx: *mut JSContext);

    pub fn JS_SetContextOpaque(ctx: *mut JSContext, opaque: *mut c_void);
    pub fn JS_GetContextOpaque(ctx: *mut JSContext) -> *mut c_void;

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
    pub fn JS_SetHostPromiseRejectionTracker(
        rt: *mut JSRuntime,
        cb: Option<JSHostPromiseRejectionTracker>,
        opaque: *mut c_void,
    );

    pub fn JS_NewCModule(
        ctx: *mut JSContext,
        name_str: *const c_char,
        func: Option<JSModuleInitFunc>,
    ) -> *mut JSModuleDef;
    pub fn JS_AddModuleExport(
        ctx: *mut JSContext,
        m: *mut JSModuleDef,
        name_str: *const c_char,
    ) -> c_int;
    pub fn JS_SetModuleExport(
        ctx: *mut JSContext,
        m: *mut JSModuleDef,
        export_name: *const c_char,
        val: JSValue,
    ) -> c_int;

    pub fn JS_GetGlobalObject(ctx: *mut JSContext) -> JSValue;

    pub fn JS_NewObject(ctx: *mut JSContext) -> JSValue;
    pub fn JS_NewArray(ctx: *mut JSContext) -> JSValue;
    pub fn JS_GetPropertyStr(
        ctx: *mut JSContext,
        this_obj: JSValueConst,
        prop: *const c_char,
    ) -> JSValue;
    pub fn JS_GetPropertyUint32(ctx: *mut JSContext, this_obj: JSValueConst, idx: u32) -> JSValue;

    pub fn JS_ToFloat64(ctx: *mut JSContext, pres: *mut f64, val: JSValueConst) -> c_int;
    pub fn JS_SetPropertyStr(
        ctx: *mut JSContext,
        this_obj: JSValueConst,
        prop: *const c_char,
        val: JSValue,
    ) -> c_int;

    pub fn JS_SetPropertyUint32(
        ctx: *mut JSContext,
        this_obj: JSValueConst,
        idx: u32,
        val: JSValue,
    ) -> c_int;

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

    pub fn JS_ReadObject(
        ctx: *mut JSContext,
        buf: *const u8,
        buf_len: usize,
        flags: c_int,
    ) -> JSValue;
    pub fn JS_WriteObject(
        ctx: *mut JSContext,
        psize: *mut usize,
        obj: JSValueConst,
        flags: c_int,
    ) -> *mut u8;

    // --- ArrayBuffer / TypedArray access (needed for WebGL-style shims) ---
    // quickjs.h: uint8_t *JS_GetArrayBuffer(JSContext *ctx, size_t *psize, JSValueConst obj);
    pub fn JS_GetArrayBuffer(ctx: *mut JSContext, psize: *mut usize, obj: JSValueConst) -> *mut u8;
    // quickjs.h:
    // JSValue JS_GetTypedArrayBuffer(JSContext *ctx, JSValueConst obj,
    //    size_t *pbyte_offset, size_t *pbyte_length, size_t *pbytes_per_element);
    pub fn JS_GetTypedArrayBuffer(
        ctx: *mut JSContext,
        obj: JSValueConst,
        pbyte_offset: *mut usize,
        pbyte_length: *mut usize,
        pbytes_per_element: *mut usize,
    ) -> JSValue;

    pub fn JS_NewError(ctx: *mut JSContext) -> JSValue;
    pub fn JS_Throw(ctx: *mut JSContext, obj: JSValue) -> JSValue;

    pub fn JS_NewPromiseCapability(ctx: *mut JSContext, resolving_funcs: *mut JSValue) -> JSValue;
    pub fn JS_IsJobPending(rt: *mut JSRuntime) -> c_int;
    pub fn JS_ExecutePendingJob(rt: *mut JSRuntime, pctx: *mut *mut JSContext) -> c_int;

    pub fn js_malloc(ctx: *mut JSContext, size: usize) -> *mut c_void;
    pub fn js_free(ctx: *mut JSContext, ptr: *mut c_void);

    pub fn __JS_FreeValue(ctx: *mut JSContext, v: JSValue);
}

/// Evaluate source while satisfying QuickJS' requirement:
/// `input[input_len] == '\0'`.
///
/// Safety: caller must provide a live `ctx` and a valid C-string `filename`.
#[inline]
pub unsafe fn js_eval_bytes(
    ctx: *mut JSContext,
    input: &[u8],
    filename: *const c_char,
    eval_flags: c_int,
) -> JSValue {
    if input.last().copied() == Some(0) {
        return JS_Eval(
            ctx,
            input.as_ptr() as *const c_char,
            input.len().saturating_sub(1),
            filename,
            eval_flags,
        );
    }

    let mut nul_terminated = Vec::with_capacity(input.len().saturating_add(1));
    nul_terminated.extend_from_slice(input);
    nul_terminated.push(0);
    JS_Eval(
        ctx,
        nul_terminated.as_ptr() as *const c_char,
        input.len(),
        filename,
        eval_flags,
    )
}
