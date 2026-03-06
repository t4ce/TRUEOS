#![cfg(feature = "trueos")]

use alloc::string::String;
use core::ffi::c_char;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_gfx_present_owner_set(owner: u32);
}

static BROWSER_TASK_STARTED: AtomicBool = AtomicBool::new(false);

#[inline]
fn log_str(s: &str) {
    if s.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, s.as_ptr(), s.len()) };
}

fn js_single_quoted_literal(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 32);
    out.push('\'');
    for ch in src.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
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
                qjs::qjs_diag::dump_last_exception(ctx, "browser pending-job");
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
    if BROWSER_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-browser: already running\n");
        return;
    }

    unsafe { trueos_cabi_gfx_present_owner_set(1) };
    log_str("qjs-browser: starting fresh browser.mjs path\n");

    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                log_str("qjs-browser: JS runtime init failed\n");
                trueos_cabi_gfx_present_owner_set(0);
                BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };

        let rt = vm.rt_ptr();
        let ctx = vm.ctx_ptr();
        qjs::node::install_globals(ctx);

        // Fresh path: install only the Rust layout drawer API for JS usage.
        qjs::layout::install_layout_api(ctx);

        let init_filename = b"<browser-init>\0";
        let html_lit = js_single_quoted_literal(qjs::ui_html::UI_HTML);
        let mut init_src = String::new();
        init_src.push_str(
            "\nconst G = (typeof globalThis !== 'undefined') ? globalThis : this;\n",
        );
        init_src.push_str("G.__trueosUiHtml = ");
        init_src.push_str(&html_lit);
        init_src.push_str(";\n");
        init_src.push_str("G.__trueosThemeNodeH = ");
        let _ = write!(&mut init_src, "{}", qjs::default_theme::NODE_H);
        init_src.push_str(";\n");
        init_src.push_str("G.__trueosThemeHierarchyIndent = ");
        let _ = write!(
            &mut init_src,
            "{}",
            qjs::default_theme::HIERARCHY_INDENT
        );
        init_src.push_str(";\n");
        init_src.push_str("G.__trueosThemeCursorSize = 12;\n");
        init_src.push_str("G.__trueosThemeIframeMinW = ");
        let _ = write!(&mut init_src, "{}", qjs::default_theme::IFRAME_MIN_W);
        init_src.push_str(";\n");
        init_src.push_str(
            r#"
if (!G.window) G.window = G;
if (typeof G.window.innerWidth !== 'number') G.window.innerWidth = 1280;
if (typeof G.window.innerHeight !== 'number') G.window.innerHeight = 800;
if (typeof G.addEventListener !== 'function') G.addEventListener = () => {};
if (typeof G.removeEventListener !== 'function') G.removeEventListener = () => {};
if (typeof G.requestAnimationFrame !== 'function') {
  G.requestAnimationFrame = (cb) => {
    try { if (typeof cb === 'function') cb(Date.now()); } catch (_) {}
    return 1;
  };
}
if (typeof G.cancelAnimationFrame !== 'function') G.cancelAnimationFrame = () => {};
if (typeof G.setTimeout !== 'function') G.setTimeout = () => 1;
if (typeof G.clearTimeout !== 'function') G.clearTimeout = () => {};
import('/qjs/browser/browser.mjs').catch((e) => {
  try { console.log('[browser.mjs] import failed', String(e && e.stack ? e.stack : e)); } catch (_) {}
});
"#,
                );
        if !eval_or_log(
            ctx,
                        init_src.as_bytes(),
            init_filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
            "browser init",
        ) {
            trueos_cabi_gfx_present_owner_set(0);
            BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
            return;
        }

        loop {
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }

        log_str("qjs-browser: stopped\n");
        trueos_cabi_gfx_present_owner_set(0);
        BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
    }
}
