#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::rc::Rc;
use core::marker::PhantomData;

use crate as qjs;

/// Owning wrapper for a QuickJS runtime/context pair.
///
/// Guardrails (kept in one place):
/// - A `JSRuntime*`/`JSContext*` must only ever be used from its single owning task/thread.
/// - This type is intentionally `!Send` and `!Sync` so it cannot be moved or shared across cores.
/// - Do not store the raw pointers for later use; instead, perform all QuickJS calls while you
///   have `&mut QjsVm` in the owning executor/task.
/// - Cross-task/core interaction must be "enqueue + notify" (message passing), never "call into ctx".
pub struct QjsVm {
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    _no_send_sync: PhantomData<Rc<()>>,
}

impl QjsVm {
    /// Create a new runtime/context pair with the TRUEOS Node-ish module loader installed.
    ///
    /// Safety: the returned VM must stay on a single owning task/thread for its entire lifetime.
    pub unsafe fn new_node() -> Option<Self> {
        let rt = unsafe { qjs::JS_NewRuntime() };
        if rt.is_null() {
            return None;
        }

        unsafe { qjs::node::install(rt) };

        let ctx = unsafe { qjs::JS_NewContext(rt) };
        if ctx.is_null() {
            unsafe { qjs::JS_FreeRuntime(rt) };
            return None;
        }

        Some(Self {
            rt,
            ctx,
            _no_send_sync: PhantomData,
        })
    }

    #[inline]
    pub fn rt_ptr(&self) -> *mut qjs::JSRuntime {
        self.rt
    }

    #[inline]
    pub fn ctx_ptr(&self) -> *mut qjs::JSContext {
        self.ctx
    }
}

impl Drop for QjsVm {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                qjs::JS_FreeContext(self.ctx);
                self.ctx = core::ptr::null_mut();
            }
            if !self.rt.is_null() {
                qjs::JS_FreeRuntime(self.rt);
                self.rt = core::ptr::null_mut();
            }
        }
    }
}
