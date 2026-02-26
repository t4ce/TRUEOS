#![cfg(feature = "trueos")]

use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_gfx_present_owner_set(owner: u32);
}

static PIXI_SMOKE_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static PIXI_CDN_PRELOAD_DONE: AtomicBool = AtomicBool::new(false);

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
                qjs::qjs_diag::dump_last_exception(ctx, "pixi-smoke pending-job");
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
        log_str("qjs-pixi-smoke: ");
        log_str(label);
        log_str(" JS_Eval exception\n");
        qjs::qjs_diag::dump_last_exception(ctx, "pixi-smoke eval");
        return false;
    }
    qjs::js_free_value(ctx, val);
    true
}

pub async fn preload_pixi_cdn_once() -> bool {
    if PIXI_CDN_PRELOAD_DONE.load(Ordering::Acquire) {
        return true;
    }
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-pixi-preload: JS_NewRuntime failed\n");
                return false;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        let preload_filename = b"<pixi-cdn-preload>\0";
        let preload_script = br#"
await import('/qjs/cdn/8d2f5f0bba6a6702.mjs');
"#;
        if !eval_or_log(
            ctx,
            preload_script,
            preload_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
            "cdn-preload",
        ) {
            drop(vm);
            return false;
        }
        for _ in 0..120 {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
        qjs::workers::terminate_all_for_context(ctx);
        let _ = pump_runtime_once(rt, ctx);
        qjs::async_ops::drain_all_for_context(ctx);
        qjs::workers::drain_all_for_context(ctx);
        qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
        drop(vm);
    }
    PIXI_CDN_PRELOAD_DONE.store(true, Ordering::Release);
    log_str("qjs-pixi-preload: cached /qjs/cdn/8d2f5f0bba6a6702.mjs\n");
    true
}

#[embassy_executor::task]
pub async fn boot_pixi_scene_smoke_task() {
    if PIXI_SMOKE_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-pixi-smoke: already running\n");
        return;
    }

    log_str("qjs-pixi-smoke: starting (render bridge on)\n");
    unsafe { trueos_cabi_gfx_present_owner_set(1) };
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-pixi-smoke: JS_NewRuntime failed\n");
                unsafe { trueos_cabi_gfx_present_owner_set(0) };
                PIXI_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        let init_filename = b"<pixi-smoke-init>\0";
        let init_script = br#"
const G = (typeof globalThis !== 'undefined') ? globalThis : this;

import * as cmd from 'cmd_stream';
const PIXI = await import('/qjs/vendor/pixi.mjs');
const W = Number((G.window && G.window.innerWidth) || 1280);
const H = Number((G.window && G.window.innerHeight) || 800);
const CX = W * 0.5;
const CY = H * 0.5;
const RING_COUNT = 8;

const root = new PIXI.Container();
const tex = PIXI.Texture.WHITE;
const bg = new PIXI.Sprite(tex);
bg.anchor.set(0.5, 0.5);
bg.width = 420;
bg.height = 260;
bg.tint = 0x2A84FF;
bg.alpha = 0.58;

const fg = new PIXI.Sprite(tex);
fg.anchor.set(0.5, 0.5);
fg.width = 360;
fg.height = 220;
fg.tint = 0xFF8B2E;
fg.alpha = 0.56;

root.addChild(bg);
root.addChild(fg);

const title = new PIXI.Text({
  text: 'True OS',
  style: {
    fontFamily: 'sans-serif',
    fontSize: 34,
    fill: 0x101010,
  },
});
title.x = 44;
title.y = 20;
title.alpha = 1.0;
root.addChild(title);

const orbitTexts = ['true', 'os', 'pixi', 'qjs', 'virgl', 'wgpu', 'demo', 's'];

const MAX_QUADS = 2 + RING_COUNT;
const out = new Uint8Array(12 * 6 * MAX_QUADS);
const dv = new DataView(out.buffer);
const atlasTex = cmd.createAtlasTexture(1);

G.__pixi_smoke = {
  root,
  bg,
  fg,
  title,
  atlasTex,
  orbitTexts,
  out,
  dv,
  t: 0.0,
  frame: 0,
};

function writeVertex(dv, out, off, x, y, rgb, alpha) {
  const nx = (2.0 * (x / W)) - 1.0;
  const ny = 1.0 - (2.0 * (y / H));
  dv.setFloat32(off + 0, nx, true);
  dv.setFloat32(off + 4, ny, true);
  out[off + 8] = (rgb >>> 16) & 0xff;
  out[off + 9] = (rgb >>> 8) & 0xff;
  out[off + 10] = rgb & 0xff;
  out[off + 11] = alpha & 0xff;
  return off + 12;
}

function emitQuad(dv, out, off, cx, cy, w, h, rot, rgb, alpha) {
  const hw = w * 0.5;
  const hh = h * 0.5;
  const c = Math.cos(rot);
  const s = Math.sin(rot);
  const p0x = cx + (-hw * c - -hh * s);
  const p0y = cy + (-hw * s + -hh * c);
  const p1x = cx + ( hw * c - -hh * s);
  const p1y = cy + ( hw * s + -hh * c);
  const p2x = cx + ( hw * c -  hh * s);
  const p2y = cy + ( hw * s +  hh * c);
  const p3x = cx + (-hw * c -  hh * s);
  const p3y = cy + (-hw * s +  hh * c);
  off = writeVertex(dv, out, off, p0x, p0y, rgb, alpha);
  off = writeVertex(dv, out, off, p1x, p1y, rgb, alpha);
  off = writeVertex(dv, out, off, p2x, p2y, rgb, alpha);
  off = writeVertex(dv, out, off, p0x, p0y, rgb, alpha);
  off = writeVertex(dv, out, off, p2x, p2y, rgb, alpha);
  off = writeVertex(dv, out, off, p3x, p3y, rgb, alpha);
  return off;
}

G.__pixi_smoke_tick = function(dt) {
  const s = G.__pixi_smoke;
  if (!s) return;
  s.t += dt;
  s.frame = (s.frame + 1) | 0;
  s.title.alpha = 1.0;

  cmd.setViewport(W | 0, H | 0);
  cmd.setPremultipliedAlpha(false);
  cmd.setBlendMode(0);
  cmd.setBlendEnabled(true);
  cmd.setClearRgb(0xFFFFFF);
  cmd.beginFrame();
  cmd.drawAtlasText(
    s.atlasTex,
    1,
    s.title.x | 0,
    s.title.y | 0,
    String(s.title.text || 'T'),
    Number((s.title.style && s.title.style.fontSize) || 34),
    Number((s.title.style && s.title.style.fill) || 0x101010),
    (s.title.alpha * 255) | 0
  );
  for (let i = 0; i < s.orbitTexts.length; i++) {
    const a = (s.t * 0.9) + (i * (6.283185307179586 / s.orbitTexts.length));
    const rr = 120 + ((i % 3) * 18);
    const x = CX + Math.cos(a) * rr;
    const y = CY + Math.sin(a * 1.07) * (rr * 0.6);
    const alpha = (145 + (90 * (0.5 + 0.5 * Math.sin(a * 1.7)))) | 0;
    cmd.drawAtlasText(
      s.atlasTex,
      1,
      x | 0,
      y | 0,
      s.orbitTexts[i],
      12,
      0x101010,
      alpha
    );
  }
  cmd.endFrame();
};
"#;

        if !eval_or_log(
            ctx,
            init_script,
            init_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
            "init",
        ) {
            drop(vm);
            unsafe { trueos_cabi_gfx_present_owner_set(0) };
            PIXI_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
            return;
        }

        // Let module jobs/imports settle before ticks.
        for _ in 0..100 {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        let tick_filename = b"<pixi-smoke-tick>\0";
        let tick_script = b"var G=(typeof globalThis!=='undefined')?globalThis:this; if (G.__pixi_smoke_tick) G.__pixi_smoke_tick(0.05);";

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

    unsafe { trueos_cabi_gfx_present_owner_set(0) };
    log_str("qjs-pixi-smoke: stopped\n");
    PIXI_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
}
