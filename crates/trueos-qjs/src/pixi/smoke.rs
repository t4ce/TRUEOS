#![cfg(feature = "trueos")]

use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static PIXI_SMOKE_TASK_STARTED: AtomicBool = AtomicBool::new(false);

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

#[embassy_executor::task]
pub async fn boot_pixi_scene_smoke_task() {
    if PIXI_SMOKE_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-pixi-smoke: already running\n");
        return;
    }

    log_str("qjs-pixi-smoke: starting (render bridge on)\n");
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-pixi-smoke: JS_NewRuntime failed\n");
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
if (typeof G.Intl === 'undefined') {
  const LOCALES = {
    en: { code: 'en', decimalSeparator: '.', groupingSeparator: ',', datePattern: 'MM/dd/yyyy', timePattern: 'HH:mm:ss', firstDayOfWeek: 7 },
    de: { code: 'de', decimalSeparator: ',', groupingSeparator: '.', datePattern: 'dd.MM.yyyy', timePattern: 'HH:mm:ss', firstDayOfWeek: 1 },
    fr: { code: 'fr', decimalSeparator: ',', groupingSeparator: ' ', datePattern: 'dd/MM/yyyy', timePattern: 'HH:mm:ss', firstDayOfWeek: 1 },
    pt: { code: 'pt', decimalSeparator: ',', groupingSeparator: '.', datePattern: 'dd/MM/yyyy', timePattern: 'HH:mm:ss', firstDayOfWeek: 1 },
    sv: { code: 'sv', decimalSeparator: ',', groupingSeparator: ' ', datePattern: 'yyyy-MM-dd', timePattern: 'HH:mm:ss', firstDayOfWeek: 1 },
  };
  const norm = (raw) => {
    if (raw === undefined || raw === null || raw === '') return 'en';
    const s = String(raw).replace(/_/g, '-').toLowerCase();
    const b = s.split('-')[0];
    return LOCALES[b] ? b : 'en';
  };
  const mk = (kind, loc) => {
    const p = LOCALES[norm(loc)];
    return {
      format(v) { return String(v === undefined ? '' : v); },
      resolvedOptions() {
        return {
          locale: p.code,
          kind,
          numberingSystem: 'latn',
          calendar: 'gregory',
          decimalSeparator: p.decimalSeparator,
          groupingSeparator: p.groupingSeparator,
          datePattern: p.datePattern,
          timePattern: p.timePattern,
          firstDayOfWeek: p.firstDayOfWeek
        };
      }
    };
  };
  G.Intl = {
    NumberFormat(loc) { return mk('number', loc); },
    DateTimeFormat(loc) { return mk('dateTime', loc); },
    Collator(loc) { return mk('generic', loc); },
    PluralRules(loc) { return mk('generic', loc); },
    RelativeTimeFormat(loc) { return mk('generic', loc); },
    ListFormat(loc) { return mk('generic', loc); },
    DisplayNames(loc) { return mk('generic', loc); },
    Locale(loc) { const p = LOCALES[norm(loc)]; this.baseName = p.code; },
    getCanonicalLocales(loc) { const p = LOCALES[norm(loc)]; return [p.code]; }
  };
}
import * as cmd from 'cmd_stream';
const PIXI = await import('/qjs/vendor/pixi.mjs');
const W = Number((G.window && G.window.innerWidth) || 1280);
const H = Number((G.window && G.window.innerHeight) || 800);
const CX = W * 0.5;
const CY = H * 0.5;

const root = new PIXI.Container();
const tex = PIXI.Texture.WHITE;
const bg = new PIXI.Sprite(tex);
bg.anchor.set(0.5, 0.5);
bg.width = 128;
bg.height = 128;
bg.tint = 0x3BA7FF;
bg.alpha = 0.8;

const fg = new PIXI.Sprite(tex);
fg.anchor.set(0.5, 0.5);
fg.width = 72;
fg.height = 72;
fg.tint = 0xFF9B3D;
fg.alpha = 0.9;

root.addChild(bg);
root.addChild(fg);

G.__pixi_smoke = {
  root,
  bg,
  fg,
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
  s.root.rotation = Math.sin(s.t * 0.5) * 0.25;
  s.bg.rotation -= 0.0125;
  s.fg.rotation += 0.03;
  s.fg.x = Math.cos(s.t * 1.7) * 56.0;
  s.fg.y = Math.sin(s.t * 1.2) * 36.0;
  s.fg.alpha = 0.55 + 0.45 * Math.abs(Math.sin(s.t * 1.3));

  const rootRot = s.root.rotation;
  const c = Math.cos(rootRot);
  const si = Math.sin(rootRot);
  const bgx = CX;
  const bgy = CY;
  const fgx = CX + (s.fg.x * c - s.fg.y * si);
  const fgy = CY + (s.fg.x * si + s.fg.y * c);
  const bgr = rootRot + s.bg.rotation;
  const fgr = rootRot + s.fg.rotation;

  const out = new Uint8Array(12 * 6 * 2);
  const dv = new DataView(out.buffer);
  let off = 0;
  off = emitQuad(dv, out, off, bgx, bgy, s.bg.width, s.bg.height, bgr, s.bg.tint >>> 0, (s.bg.alpha * 255) | 0);
  off = emitQuad(dv, out, off, fgx, fgy, s.fg.width, s.fg.height, fgr, s.fg.tint >>> 0, (s.fg.alpha * 255) | 0);

  cmd.setViewport(W | 0, H | 0);
  cmd.setBlendEnabled(true);
  cmd.setClearRgb(0x101824);
  cmd.beginFrame();
  cmd.drawTrianglesU8(out.subarray(0, off));
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

    log_str("qjs-pixi-smoke: stopped\n");
    PIXI_SMOKE_TASK_STARTED.store(false, Ordering::SeqCst);
}
