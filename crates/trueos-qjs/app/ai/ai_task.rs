#![cfg(feature = "trueos")]

use core::ffi::c_char;

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

unsafe fn drain_pending_jobs(rt: *mut qjs::JSRuntime, fallback_ctx: *mut qjs::JSContext) -> bool {
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
                qjs::qjs_diag::dump_last_exception(ctx, "ai pending-job");
            }
            return false;
        }
        break;
    }
    true
}

unsafe fn pump_runtime_once(rt: *mut qjs::JSRuntime, ctx: *mut qjs::JSContext) -> bool {
    let mut progress = false;
    progress |= qjs::async_ops::pump(ctx);
    progress |= qjs::workers::pump(ctx);
    progress |= qjs::timers::pump(ctx);
    if !drain_pending_jobs(rt, ctx) {
        return false;
    }
    if qjs::JS_IsJobPending(rt) > 0
        || qjs::async_ops::has_pending(ctx)
        || qjs::workers::has_pending_for_ctx(ctx)
    {
        qjs::trueos_shims::trueos_cabi_poll_once();
        if !progress {
            qjs::trueos_shims::trueos_cabi_poll_once();
        }
    }
    true
}

#[embassy_executor::task]
pub async fn run_once() {
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                qjs::trueos_shims::log_error("qjs-ai: JS runtime init failed\n");
                return;
            }
        };
        let ctx = vm.ctx_ptr();
        let rt = vm.rt_ptr();

        'run: {
            qjs::node::install_globals(ctx);

            let worker_smoke_filename = b"<ai-worker-smoke>\0";
            let worker_smoke_src = br#"
import { Worker } from "node:worker_threads";

const w = new Worker(`
    import { parentPort } from "node:worker_threads";
    console.log("qjs-ai worker: hello from embassy task");
    parentPort.onMessage((msg) => {
        console.log("qjs-ai worker recv", msg);
        parentPort.postMessage("hello-from-worker");
    });
`);

w.onMessage((msg) => {
    console.log("qjs-ai main recv", msg);
    w.terminate();
});

w.postMessage("hello-from-main");
"#;
            let worker_smoke = qjs::js_eval_bytes(
                ctx,
                worker_smoke_src,
                worker_smoke_filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_MODULE,
            );
            if worker_smoke.is_exception() {
                qjs::qjs_diag::dump_last_exception(ctx, "qjs-ai worker smoke");
                qjs::js_free_value(ctx, worker_smoke);
                break 'run;
            }
            qjs::js_free_value(ctx, worker_smoke);

            // Some vendored node polyfills assume Blob/File/btoa/atob exist as globals.
            let shim_filename = b"<ai-shims>\0";
            let shim_src = br#"
        var __g = (typeof globalThis !== 'undefined') ? globalThis : this;
        if (typeof __g.Blob !== 'function') {
            __g.Blob = function Blob(_parts, opts) {
                this.size = 0;
                this.type = (opts && opts.type) ? String(opts.type) : '';
            };
            __g.Blob.prototype.arrayBuffer = function () { return Promise.resolve(new ArrayBuffer(0)); };
            __g.Blob.prototype.text = function () { return Promise.resolve(''); };
            __g.Blob.prototype.slice = function () { return this; };
        }
        if (typeof __g.File !== 'function') {
            __g.File = function File(parts, name, opts) {
                __g.Blob.call(this, parts, opts);
                this.name = String(name || 'file');
                this.lastModified = (opts && opts.lastModified) ? opts.lastModified : 0;
            };
            __g.File.prototype = Object.create(__g.Blob.prototype);
            __g.File.prototype.constructor = __g.File;
        }
        if (typeof __g.btoa !== 'function') {
            __g.btoa = function (_s) { return ''; };
        }
        if (typeof __g.atob !== 'function') {
            __g.atob = function (_s) { return ''; };
        }
"#;
            let shim = qjs::js_eval_bytes(
                ctx,
                shim_src,
                shim_filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_GLOBAL,
            );
            if shim.is_exception() {
                qjs::qjs_diag::dump_last_exception(ctx, "qjs-ai shims");
                qjs::js_free_value(ctx, shim);
                break 'run;
            }
            qjs::js_free_value(ctx, shim);

            let filename = b"<ai-init-module>\0";
            let src = b"import * as __ai from \"/qjs/ai/ai.mjs\";";

            let boot = qjs::js_eval_bytes(
                ctx,
                src,
                filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_MODULE,
            );
            if boot.is_exception() {
                qjs::qjs_diag::dump_last_exception(ctx, "qjs-ai init");
                qjs::js_free_value(ctx, boot);
                break 'run;
            }
            qjs::js_free_value(ctx, boot);

            let mut elapsed_ms: u64 = 0;
            let mut idle_ticks: u32 = 0;
            let step_ms: u64 = 20;
            let timeout_ms: u64 = 90_000;

            loop {
                if !pump_runtime_once(rt, ctx) {
                    break;
                }

                let idle_now = qjs::JS_IsJobPending(rt) <= 0
                    && !qjs::async_ops::has_pending(ctx)
                    && !qjs::workers::has_pending_for_ctx(ctx);
                if idle_now {
                    idle_ticks = idle_ticks.saturating_add(1);
                    if idle_ticks >= 25 {
                        break;
                    }
                } else {
                    idle_ticks = 0;
                }

                if elapsed_ms >= timeout_ms {
                    qjs::trueos_shims::log_error("qjs-ai: timeout waiting for /qjs/ai/ai.mjs\n");
                    break;
                }

                Timer::after(EmbassyDuration::from_millis(step_ms)).await;
                elapsed_ms = elapsed_ms.saturating_add(step_ms);
            }
        }

        let _ = qjs::vm::teardown_main_context(rt, ctx, 2_000).await;
    }
}
