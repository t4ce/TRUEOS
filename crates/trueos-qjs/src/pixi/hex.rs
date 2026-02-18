#![cfg(feature = "trueos")]

use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static STREAM_GFX_SMOKE_TASK_STARTED: AtomicBool = AtomicBool::new(false);

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
            let ctx = if !job_ctx.is_null() {
                job_ctx
            } else {
                fallback_ctx
            };
            if !ctx.is_null() {
                qjs::qjs_diag::dump_last_exception(ctx, "stream-gfx-smoke pending-job");
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
        log_str("qjs-stream-gfx-smoke: ");
        log_str(label);
        log_str(" JS_Eval exception\n");
        qjs::qjs_diag::dump_last_exception(ctx, "stream-gfx-smoke eval");
        return false;
    }
    qjs::js_free_value(ctx, val);
    true
}

#[embassy_executor::task]
pub async fn boot_stream_gfx_smoke_task() {
    if STREAM_GFX_SMOKE_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-stream-gfx-smoke: already running\n");
        return;
    }

    log_str("qjs-stream-gfx-smoke: starting (20Hz)\n");
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-stream-gfx-smoke: JS_NewRuntime failed\n");
                STREAM_GFX_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        let init_filename = b"<stream-gfx-smoke-init-v10>\0";
        log_str("qjs-stream-gfx-smoke: init script v10\n");
        let init_script = br#"import * as cmd from 'cmd_stream';
var G = (typeof globalThis !== 'undefined') ? globalThis : this;
var W = Number((G.window && G.window.innerWidth) || 1280);
var H = Number((G.window && G.window.innerHeight) || 800);
var CX = W * 0.5;
var CY = H * 0.5;
var R = Math.max(60, Math.floor(Math.min(W, H) * 0.28));
var CLEAR = 0x1f3f7a;
var FILL = 0xffe45e;
function packHex(a) {
  var pts = [];
  for (var i = 0; i < 6; i++) {
    var ang = a + i * (Math.PI / 3);
    pts.push([CX + Math.cos(ang) * R, CY + Math.sin(ang) * R]);
  }
  var out = new Uint8Array(6 * 3 * 12);
  var dv = new DataView(out.buffer);
  var off = 0;
  function v(x, y) {
    var nx = (2.0 * (x / W)) - 1.0;
    var ny = 1.0 - (2.0 * (y / H));
    dv.setFloat32(off + 0, nx, true);
    dv.setFloat32(off + 4, ny, true);
    out[off + 8] = (FILL >>> 16) & 0xff;
    out[off + 9] = (FILL >>> 8) & 0xff;
    out[off + 10] = FILL & 0xff;
    out[off + 11] = 0;
    off += 12;
  }
  for (var j = 0; j < 6; j++) {
    var p0 = pts[j];
    var p1 = pts[(j + 1) % 6];
    v(CX, CY);
    v(p0[0], p0[1]);
    v(p1[0], p1[1]);
  }
  return out;
}
G.__trueos_stream_gfx_smoke_tick = function(angleRad) {
  cmd.setViewport(W | 0, H | 0);
  cmd.setBlendEnabled(false);
  cmd.setClearRgb(CLEAR >>> 0);
  cmd.beginFrame();
  cmd.drawTrianglesU8(packHex(angleRad));
  cmd.endFrame();
};"#;

        if !eval_or_log(
            ctx,
            init_script,
            init_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
            "init",
        ) {
            drop(vm);
            STREAM_GFX_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
            return;
        }

        for _ in 0..50 {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        let tick_filename = b"<stream-gfx-smoke-tick>\0";
        let tick_script = b"var G=(typeof globalThis!=='undefined')?globalThis:this; G.__trueos_stream_gfx_smoke_a=(G.__trueos_stream_gfx_smoke_a||0)+0.05; if (G.__trueos_stream_gfx_smoke_tick) G.__trueos_stream_gfx_smoke_tick(G.__trueos_stream_gfx_smoke_a);";

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

    log_str("qjs-stream-gfx-smoke: stopped\n");
    STREAM_GFX_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
}
