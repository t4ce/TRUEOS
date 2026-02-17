#![cfg(feature = "trueos")]

use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static PIXI_UI_TASK_STARTED: AtomicBool = AtomicBool::new(false);

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

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
            let ctx = if !job_ctx.is_null() { job_ctx } else { fallback_ctx };
            if !ctx.is_null() {
                qjs::qjs_diag::dump_last_exception(ctx, "pixi-ui pending-job");
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

unsafe fn eval_or_log(
    ctx: *mut qjs::JSContext,
    src: &[u8],
    filename: *const c_char,
    flags: i32,
    label: &str,
) -> bool {
    let val = qjs::js_eval_bytes(ctx, src, filename, flags);
    if val.is_exception() {
        log_str("qjs-pixi-ui: ");
        log_str(label);
        log_str(" JS_Eval exception\n");
        qjs::qjs_diag::dump_last_exception(ctx, "pixi-ui eval");
        return false;
    }
    qjs::js_free_value(ctx, val);
    true
}

#[embassy_executor::task]
pub async fn boot_pixi_ui_task() {
    if PIXI_UI_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-pixi-ui: already running\n");
        return;
    }

    log_str("qjs-pixi-ui: starting (20Hz)\n");
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-pixi-ui: JS_NewRuntime failed\n");
                PIXI_UI_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        let init_filename_jsdelivr = b"<pixi-ui-init-v2-jsdelivr>\0";
        let init_filename_esmsh = b"<pixi-ui-init-v2-esmsh>\0";
        let init_filename_unpkg = b"<pixi-ui-init-v2-unpkg>\0";
        let init_script_jsdelivr = br#"import * as PIXI from 'https://cdn.jsdelivr.net/npm/pixi.js@7.4.3/+esm';
var G = (typeof globalThis !== 'undefined') ? globalThis : this;
var W = Number((G.window && G.window.innerWidth) || 1280);
var H = Number((G.window && G.window.innerHeight) || 800);
var canvas = (G.document && G.document.createElement) ? G.document.createElement('canvas') : null;
if (!canvas) throw new Error('no canvas available');
canvas.width = W;
canvas.height = H;
var renderer = new PIXI.Renderer({
  view: canvas,
  width: W,
  height: H,
  backgroundColor: 0x1f3f7a,
  antialias: false
});
var stage = new PIXI.Container();
var rect = new PIXI.Graphics();
rect.beginFill(0xffe45e);
rect.drawRect(-120, -80, 240, 160);
rect.endFill();
rect.x = W * 0.5;
rect.y = H * 0.5;
stage.addChild(rect);
G.__trueos_pixi_ui_tick = function(angleRad) {
  rect.rotation = angleRad;
  renderer.render(stage);
};"#;
        let init_script_esmsh = br#"import * as PIXI from 'https://esm.sh/pixi.js@7.4.3';
var G = (typeof globalThis !== 'undefined') ? globalThis : this;
var W = Number((G.window && G.window.innerWidth) || 1280);
var H = Number((G.window && G.window.innerHeight) || 800);
var canvas = (G.document && G.document.createElement) ? G.document.createElement('canvas') : null;
if (!canvas) throw new Error('no canvas available');
canvas.width = W;
canvas.height = H;
var renderer = new PIXI.Renderer({
  view: canvas,
  width: W,
  height: H,
  backgroundColor: 0x1f3f7a,
  antialias: false
});
var stage = new PIXI.Container();
var rect = new PIXI.Graphics();
rect.beginFill(0xffe45e);
rect.drawRect(-120, -80, 240, 160);
rect.endFill();
rect.x = W * 0.5;
rect.y = H * 0.5;
stage.addChild(rect);
G.__trueos_pixi_ui_tick = function(angleRad) {
  rect.rotation = angleRad;
  renderer.render(stage);
};"#;
        let init_script_unpkg = br#"import * as PIXI from 'https://unpkg.com/pixi.js@7.4.3/dist/pixi.mjs';
var G = (typeof globalThis !== 'undefined') ? globalThis : this;
var W = Number((G.window && G.window.innerWidth) || 1280);
var H = Number((G.window && G.window.innerHeight) || 800);
var canvas = (G.document && G.document.createElement) ? G.document.createElement('canvas') : null;
if (!canvas) throw new Error('no canvas available');
canvas.width = W;
canvas.height = H;
var renderer = new PIXI.Renderer({
  view: canvas,
  width: W,
  height: H,
  backgroundColor: 0x1f3f7a,
  antialias: false
});
var stage = new PIXI.Container();
var rect = new PIXI.Graphics();
rect.beginFill(0xffe45e);
rect.drawRect(-120, -80, 240, 160);
rect.endFill();
rect.x = W * 0.5;
rect.y = H * 0.5;
stage.addChild(rect);
G.__trueos_pixi_ui_tick = function(angleRad) {
  rect.rotation = angleRad;
  renderer.render(stage);
};"#;

        let mut init_ok = false;
        init_ok |= eval_or_log(
            ctx,
            init_script_jsdelivr,
            init_filename_jsdelivr.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
            "init(jsdelivr)",
        );
        if !init_ok {
            log_str("qjs-pixi-ui: retrying Pixi import via esm.sh\n");
            init_ok |= eval_or_log(
                ctx,
                init_script_esmsh,
                init_filename_esmsh.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_MODULE,
                "init(esm.sh)",
            );
        }
        if !init_ok {
            log_str("qjs-pixi-ui: retrying Pixi import via unpkg\n");
            init_ok |= eval_or_log(
                ctx,
                init_script_unpkg,
                init_filename_unpkg.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_MODULE,
                "init(unpkg)",
            );
        }

        if !init_ok {
            drop(vm);
            PIXI_UI_TASK_STARTED.store(false, Ordering::SeqCst);
            return;
        }

        for _ in 0..200 {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        let tick_filename = b"<pixi-ui-tick>\0";
        let tick_script = b"var G=(typeof globalThis!=='undefined')?globalThis:this; G.__trueos_pixi_ui_a=(G.__trueos_pixi_ui_a||0)+0.03; if (G.__trueos_pixi_ui_tick) G.__trueos_pixi_ui_tick(G.__trueos_pixi_ui_a);";

        loop {
            if !eval_or_log(
                ctx,
                tick_script,
                tick_filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_GLOBAL,
                "tick",
            ) {
                break;
            }
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

        qjs::workers::terminate_all_for_context(ctx);
        let _ = pump_runtime_once(rt, ctx);
        qjs::async_ops::drain_all_for_context(ctx);
        qjs::workers::drain_all_for_context(ctx);
        qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
        drop(vm);
    }

    log_str("qjs-pixi-ui: stopped\n");
    PIXI_UI_TASK_STARTED.store(false, Ordering::SeqCst);
}
