use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

static PIXI_HEX_TASK_STARTED: AtomicBool = AtomicBool::new(false);

unsafe fn drain_pending_jobs(rt: *mut trueos_qjs::JSRuntime, fallback_ctx: *mut trueos_qjs::JSContext) -> bool {
    if rt.is_null() {
        return true;
    }
    loop {
        let mut job_ctx: *mut trueos_qjs::JSContext = core::ptr::null_mut();
        let rc = trueos_qjs::JS_ExecutePendingJob(rt, &mut job_ctx as *mut *mut trueos_qjs::JSContext);
        if rc > 0 {
            continue;
        }
        if rc < 0 {
            let ctx = if !job_ctx.is_null() { job_ctx } else { fallback_ctx };
            if !ctx.is_null() {
                trueos_qjs::qjs_diag::dump_last_exception(ctx, "pixi-hex pending-job");
            }
            return false;
        }
        break;
    }
    true
}

unsafe fn pump_runtime_once(rt: *mut trueos_qjs::JSRuntime, ctx: *mut trueos_qjs::JSContext) -> bool {
    let mut progress = false;
    progress |= trueos_qjs::async_ops::pump(ctx);
    progress |= trueos_qjs::workers::pump(ctx);
    if !drain_pending_jobs(rt, ctx) {
        return false;
    }
    if trueos_qjs::JS_IsJobPending(rt) > 0
        || trueos_qjs::async_ops::has_pending(ctx)
        || trueos_qjs::workers::has_pending_for_ctx(ctx)
    {
        trueos_qjs::trueos_shims::trueos_cabi_poll_once();
        if !progress {
            trueos_qjs::trueos_shims::trueos_cabi_poll_once();
        }
    }
    true
}

unsafe fn eval_or_log(
    ctx: *mut trueos_qjs::JSContext,
    src: &[u8],
    filename: *const c_char,
    flags: i32,
    label: &str,
) -> bool {
    let val = trueos_qjs::JS_Eval(
        ctx,
        src.as_ptr() as *const c_char,
        src.len(),
        filename,
        flags,
    );
    if val.is_exception() {
        crate::log!("qjs-pixi-hex: {} JS_Eval exception\n", label);
        trueos_qjs::qjs_diag::dump_last_exception(ctx, "pixi-hex eval");
        return false;
    }
    trueos_qjs::js_free_value(ctx, val);
    true
}

#[embassy_executor::task]
pub async fn boot_pixi_hexagon_task() {
    if PIXI_HEX_TASK_STARTED.swap(true, Ordering::SeqCst) {
        crate::log!("qjs-pixi-hex: already running\n");
        return;
    }

    crate::log!("qjs-pixi-hex: starting (20Hz)\n");
    unsafe {
        let vm = match trueos_qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                crate::log!("qjs-pixi-hex: JS_NewRuntime failed\n");
                PIXI_HEX_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        trueos_qjs::node::install_globals(ctx);

        let init_filename = b"<pixi-hex-init>\0";
        let init_script = br#"
import * as PIXI from 'pixi.js@7.4.0';
import * as cmd from 'cmd_stream';

const W = Number((globalThis.window && window.innerWidth) || 1280);
const H = Number((globalThis.window && window.innerHeight) || 800);
const CX = W * 0.5;
const CY = H * 0.5;
const R = Math.max(24, Math.floor(Math.min(W, H) * 0.18));
const CLEAR = 0x081830;
const FILL = 0x3ddc97;

function packHex(a) {
  const pts = [];
  for (let i = 0; i < 6; i++) {
    const ang = a + i * (Math.PI / 3);
    pts.push([CX + Math.cos(ang) * R, CY + Math.sin(ang) * R]);
  }
  const out = new Uint8Array(6 * 3 * 12);
  const dv = new DataView(out.buffer);
  let off = 0;
  const v = (x, y) => {
    dv.setFloat32(off + 0, x, true);
    dv.setFloat32(off + 4, y, true);
    out[off + 8] = (FILL >>> 16) & 0xff;
    out[off + 9] = (FILL >>> 8) & 0xff;
    out[off + 10] = FILL & 0xff;
    out[off + 11] = 0;
    off += 12;
  };
  for (let i = 0; i < 6; i++) {
    const p0 = pts[i];
    const p1 = pts[(i + 1) % 6];
    v(CX, CY);
    v(p0[0], p0[1]);
    v(p1[0], p1[1]);
  }
  return out;
}

globalThis.__trueos_pixi_hex_tick = (angleRad) => {
  cmd.setViewport(W | 0, H | 0);
  cmd.setBlendEnabled(false);
  cmd.setClearRgb(CLEAR >>> 0);
  cmd.beginFrame();
  cmd.drawTrianglesU8(packHex(angleRad));
  cmd.endFrame();
};

try {
  const app = new PIXI.Application({
    width: W,
    height: H,
    antialias: true,
    autoStart: false,
    clearBeforeRender: true,
    backgroundAlpha: 0,
  });
  const g = new PIXI.Graphics();
  app.stage.addChild(g);
  globalThis.__trueos_pixi_hex_tick = (angleRad) => {
    g.clear();
    g.lineStyle(2, 0xffffff, 1);
    g.beginFill(FILL, 1);
    for (let i = 0; i < 6; i++) {
      const ang = angleRad + i * (Math.PI / 3);
      const x = CX + Math.cos(ang) * R;
      const y = CY + Math.sin(ang) * R;
      if (i === 0) g.moveTo(x, y);
      else g.lineTo(x, y);
    }
    g.closePath();
    g.endFill();
    app.renderer.render(app.stage);
  };
} catch (_e) {
  // Keep cmd_stream fallback active when Pixi setup is unavailable.
}
0
"#;

        if !eval_or_log(
            ctx,
            init_script,
            init_filename.as_ptr() as *const c_char,
            trueos_qjs::JS_EVAL_TYPE_MODULE,
            "init",
        ) {
            drop(vm);
            PIXI_HEX_TASK_STARTED.store(false, Ordering::SeqCst);
            return;
        }

        // Give module imports/promises time to settle before first tick.
        for _ in 0..50 {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        let tick_filename = b"<pixi-hex-tick>\0";
        let tick_script = b"globalThis.__trueos_hex_a=(globalThis.__trueos_hex_a||0)+0.05; if (globalThis.__trueos_pixi_hex_tick) globalThis.__trueos_pixi_hex_tick(globalThis.__trueos_hex_a);";

        loop {
            if !eval_or_log(
                ctx,
                tick_script,
                tick_filename.as_ptr() as *const c_char,
                trueos_qjs::JS_EVAL_TYPE_GLOBAL,
                "tick",
            ) {
                break;
            }
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }

        trueos_qjs::workers::terminate_all_for_context(ctx);
        let _ = pump_runtime_once(rt, ctx);
        trueos_qjs::async_ops::drain_all_for_context(ctx);
        trueos_qjs::workers::drain_all_for_context(ctx);
        trueos_qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
        drop(vm);
    }

    crate::log!("qjs-pixi-hex: stopped\n");
    PIXI_HEX_TASK_STARTED.store(false, Ordering::SeqCst);
}
