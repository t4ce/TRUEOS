#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::rc::Rc;
use core::marker::PhantomData;
use embassy_time::{Duration as EmbassyDuration, Timer};

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

        unsafe { qjs::qjs_diag::install_runtime(rt) };

        unsafe { qjs::node::install(rt) };

        let ctx = unsafe { qjs::JS_NewContext(rt) };
        if ctx.is_null() {
            unsafe { qjs::JS_FreeRuntime(rt) };
            return None;
        }

        unsafe { qjs::qjs_diag::install_context(ctx) };

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

pub(crate) unsafe fn drain_pending_jobs(
    rt: *mut qjs::JSRuntime,
    fallback_ctx: *mut qjs::JSContext,
    label: &str,
) -> bool {
    if rt.is_null() {
        return true;
    }
    loop {
        let mut job_ctx: *mut qjs::JSContext = core::ptr::null_mut();
        let rc = qjs::JS_ExecutePendingJob(rt, &mut job_ctx as *mut *mut qjs::JSContext);
        if rc > 0 {
            continue;
        }
        if rc < 0 {
            let ctx = if !job_ctx.is_null() {
                job_ctx
            } else {
                fallback_ctx
            };
            if !ctx.is_null() {
                qjs::qjs_diag::dump_last_exception(ctx, label);
            }
            return false;
        }
        break;
    }
    true
}

pub(crate) unsafe fn pump_runtime_once(
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    label: &str,
) -> bool {
    let mut progress = false;
    progress |= qjs::async_ops::pump(ctx);
    progress |= qjs::workers::pump(ctx);
    progress |= qjs::timers::pump(ctx);
    if !drain_pending_jobs(rt, ctx, label) {
        return false;
    }
    if qjs::JS_IsJobPending(rt) > 0 || qjs::workers::has_pending_for_ctx(ctx) {
        qjs::trueos_shims::trueos_cabi_poll_once();
        if !progress {
            qjs::trueos_shims::trueos_cabi_poll_once();
        }
    }
    true
}

async unsafe fn drain_runtime_until_idle(
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    max_wait_ms: u64,
) -> bool {
    let mut elapsed_ms: u64 = 0;

    loop {
        if !pump_runtime_once(rt, ctx, "teardown") {
            return false;
        }

        let pending = qjs::JS_IsJobPending(rt) > 0
            || qjs::async_ops::has_pending(ctx)
            || qjs::workers::has_pending_for_ctx(ctx);
        if !pending {
            return true;
        }

        if elapsed_ms >= max_wait_ms {
            qjs::trueos_shims::log_error("quickjs: runtime teardown timeout\n");
            return false;
        }

        Timer::after(EmbassyDuration::from_millis(1)).await;
        elapsed_ms = elapsed_ms.saturating_add(1);
    }
}

pub async unsafe fn teardown_main_context(
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    max_wait_ms: u64,
) -> bool {
    if rt.is_null() || ctx.is_null() {
        return true;
    }

    qjs::workers::terminate_all_for_context(ctx);
    let drained = drain_runtime_until_idle(rt, ctx, max_wait_ms).await;
    qjs::async_ops::drain_all_for_context(ctx);
    qjs::workers::drain_all_for_context(ctx);
    qjs::timers::drain_all_for_context(ctx);
    drained
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
