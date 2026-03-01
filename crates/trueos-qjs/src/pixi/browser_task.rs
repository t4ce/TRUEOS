#![cfg(feature = "trueos")]

use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_gfx_present_owner_set(owner: u32);
}

static WEBGPU_BROWSER_TASK_STARTED: AtomicBool = AtomicBool::new(false);

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
                qjs::qjs_diag::dump_last_exception(ctx, "pixi-browser pending-job");
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
        log_str("qjs-browser: ");
        log_str(label);
        log_str(" JS_Eval exception\n");
        qjs::qjs_diag::dump_last_exception(ctx, "browser eval");
        return false;
    }
    qjs::js_free_value(ctx, val);
    true
}

#[embassy_executor::task]
pub async fn boot_browser() {
    if WEBGPU_BROWSER_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-browser: already running\n");
        return;
    }

    log_str("qjs-browser: starting (render bridge on)\n");
    unsafe { trueos_cabi_gfx_present_owner_set(1) };
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-browser: JS_NewRuntime failed\n");
                trueos_cabi_gfx_present_owner_set(0);
                WEBGPU_BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        // Install native mouse bridge helpers on the global object so JS can poll
        // and dispatch pointer events without needing a browser host runtime.
        let global = qjs::JS_GetGlobalObject(ctx);
        qjs::browser::install_mouse_api(ctx, global);
        qjs::js_free_value(ctx, global);

        let init_filename = b"<pixi-browser-init>\0";
        let init_script = br#"
const G = (typeof globalThis !== 'undefined') ? globalThis : this;

if (!G.window) G.window = G;
if (typeof G.window.innerWidth !== 'number') G.window.innerWidth = 1280;
if (typeof G.window.innerHeight !== 'number') G.window.innerHeight = 800;
if (!G.window.devicePixelRatio) G.window.devicePixelRatio = 1;
if (!G.requestAnimationFrame) {
  G.requestAnimationFrame = (cb) => {
    Promise.resolve().then(() => cb(Date.now()));
    return 1;
  };
}
if (!G.cancelAnimationFrame) G.cancelAnimationFrame = () => {};

const mkNode = () => ({
  style: {},
  children: [],
  parentNode: null,
  ownerDocument: null,
  eventMode: 'none',
  appendChild(ch) { this.children.push(ch); ch.parentNode = this; return ch; },
  removeChild(ch) { this.children = this.children.filter((x) => x !== ch); ch.parentNode = null; return ch; },
  addEventListener() {},
  removeEventListener() {},
  dispatchEvent() { return true; },
  setAttribute() {},
  getAttribute() { return null; },
  contains(node) { if (node === this) return true; for (const c of this.children) { if (c && typeof c.contains === 'function' && c.contains(node)) return true; } return false; },
  getBoundingClientRect() { return { x: 0, y: 0, left: 0, top: 0, width: this.width || 0, height: this.height || 0 }; },
});

if (!G.document) {
  const doc = mkNode();
  doc.ownerDocument = doc;
  doc.documentElement = mkNode();
  doc.head = mkNode();
  doc.body = mkNode();
  doc.documentElement.ownerDocument = doc;
  doc.head.ownerDocument = doc;
  doc.body.ownerDocument = doc;
  doc.documentElement.appendChild(doc.head);
  doc.documentElement.appendChild(doc.body);

  const mkCanvas = () => {
    const c = mkNode();
    c.tagName = 'CANVAS';
    c.width = G.window.innerWidth | 0;
    c.height = G.window.innerHeight | 0;
    c.getContext = (kind) => {
      if (kind !== '2d') return null;
      return {
        font: '16px sans-serif',
        measureText: (s) => ({ width: (String(s).length || 0) * 8 }),
        clearRect() {},
        fillRect() {},
        beginPath() {},
        moveTo() {},
        lineTo() {},
        stroke() {},
        fill() {},
      };
    };
    return c;
  };

  doc.createElement = (tag) => {
    const t = String(tag || '').toLowerCase();
    const n = t === 'canvas' ? mkCanvas() : mkNode();
    n.tagName = String(tag || '').toUpperCase();
    n.ownerDocument = doc;
    return n;
  };
  doc.getElementById = () => null;
  doc.addEventListener = () => {};
  doc.removeEventListener = () => {};
  doc.dispatchEvent = () => true;
  G.document = doc;
}

if (!G.fetch) {
  G.fetch = async () => ({ text: async () => '<html><body><h1>TRUEOS Browser</h1></body></html>' });
}

await import('/qjs/browser/main.mjs');
"#;

        if !eval_or_log(
            ctx,
            init_script,
            init_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
            "browser-init",
        ) {
            qjs::workers::terminate_all_for_context(ctx);
            let _ = pump_runtime_once(rt, ctx);
            qjs::async_ops::drain_all_for_context(ctx);
            qjs::workers::drain_all_for_context(ctx);
            qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
            drop(vm);
            trueos_cabi_gfx_present_owner_set(0);
            WEBGPU_BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
            return;
        }

        let mouse_pump_filename = b"<pixi-browser-mouse-pump>\0";
        let mouse_pump_script =
            b"var G=(typeof globalThis!=='undefined')?globalThis:this; if (G.__trueos_mouse_pump) G.__trueos_mouse_pump();";

        // Browser loop: poll host events + async jobs + mouse bridge.
        loop {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            let _ = eval_or_log(
                ctx,
                mouse_pump_script,
                mouse_pump_filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_GLOBAL,
                "mouse-pump",
            );
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }

        qjs::workers::terminate_all_for_context(ctx);
        let _ = pump_runtime_once(rt, ctx);
        qjs::async_ops::drain_all_for_context(ctx);
        qjs::workers::drain_all_for_context(ctx);
        qjs::JS_SetContextOpaque(ctx, core::ptr::null_mut());
        drop(vm);
    }

    unsafe { trueos_cabi_gfx_present_owner_set(0) };
    WEBGPU_BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
    log_str("qjs-browser: stopped\n");
}
