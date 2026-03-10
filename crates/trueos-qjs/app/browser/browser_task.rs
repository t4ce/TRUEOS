#![cfg(feature = "trueos")]
use alloc::collections::VecDeque;
use alloc::string::String;
use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
use crate as qjs;

mod ai_api;
mod helpers;

static BROWSER_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static PENDING_HTML: Mutex<Option<String>> = Mutex::new(None);
static PENDING_AI_INPUT: Mutex<VecDeque<AiInputEntry>> = Mutex::new(VecDeque::new());

#[derive(Clone)]
pub struct AiInputEntry {
    pub text: String,
    pub web_search: bool,
    pub new_conversation: bool,
    pub computer_use: bool,
}

pub fn queue_set_html(next_html: String) -> bool {
    if !BROWSER_TASK_STARTED.load(Ordering::SeqCst) {
        return false;
    }
    *PENDING_HTML.lock() = Some(next_html);
    true
}

pub fn queue_ai_input(next: AiInputEntry) -> bool {
    if !BROWSER_TASK_STARTED.load(Ordering::SeqCst) {
        return false;
    }
    PENDING_AI_INPUT.lock().push_back(next);
    true
}

unsafe fn apply_pending_html(ctx: *mut qjs::JSContext) {
    let Some(next_html) = PENDING_HTML.lock().take() else {
        return;
    };

    let html_lit = helpers::js_single_quoted_literal(next_html.as_str());
    let mut src = String::new();
    src.push_str("(function(){const __g=(typeof globalThis!=='undefined')?globalThis:this;const __h=");
    src.push_str(&html_lit);
    src.push_str(";if(__g.__trueosBrowser&&typeof __g.__trueosBrowser.setHtml==='function'){__g.__trueosBrowser.setHtml(__h);}else{__g.__trueosUiHtml=__h;}})();");
    let filename = b"<browser-set-html>\0";
    let _ = helpers::eval_or_log(
        ctx,
        src.as_bytes(),
        filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
        "browser setHtml",
    );
}

unsafe fn apply_pending_ai_input(ctx: *mut qjs::JSContext) {
    let Some(next) = PENDING_AI_INPUT.lock().pop_front() else {
        return;
    };

    let text_lit = helpers::js_single_quoted_literal(next.text.as_str());
    let mut src = String::new();
    src.push_str("(function(){const __g=(typeof globalThis!=='undefined')?globalThis:this;");
    src.push_str("if(typeof __g.__trueosAiInputPush!=='function')return;");
    src.push_str("__g.__trueosAiInputPush({text:");
    src.push_str(&text_lit);
    src.push_str(",webSearch:");
    src.push_str(if next.web_search { "true" } else { "false" });
    src.push_str(",newConversation:");
    src.push_str(if next.new_conversation { "true" } else { "false" });
    src.push_str(",computerUse:");
    src.push_str(if next.computer_use { "true" } else { "false" });
    src.push_str("});})();");

    let _ = helpers::eval_or_log(
        ctx,
        src.as_bytes(),
        b"<browser-ai-input>\0".as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
        "browser ai input",
    );
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

unsafe fn install_globals(ctx: *mut qjs::JSContext) -> bool {
    let init_filename = b"<browser-globals>\0";
    let html_lit = helpers::js_single_quoted_literal(qjs::ui_html::UI_HTML);
    let mut init_src = String::new();
    init_src.push_str("\nconst G = (typeof globalThis !== 'undefined') ? globalThis : this;\n");
    init_src.push_str("G.__trueosUiHtml = ");
    init_src.push_str(&html_lit);
    init_src.push_str(";\n");
    init_src.push_str(
        r#"
if (typeof G.__trueosBrowserAutoStartAi === 'undefined') {
    G.__trueosBrowserAutoStartAi = { specifier: '/qjs/ai/ai_pc.mjs' };
}
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
"#,
    );

    if !helpers::eval_or_log(
        ctx,
        init_src.as_bytes(),
        init_filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
        "browser globals",
    ) {
        return false;
    }

    ai_api::install_globals(ctx)
}

#[embassy_executor::task]
pub async fn boot_browser() {
    if BROWSER_TASK_STARTED.swap(true, Ordering::SeqCst) {
        qjs::trueos_shims::log_info("qjs-browser: already running\n");
        return;
    }
    qjs::trueos_shims::log_info("qjs-browser: starting browser2 bootstrap\n");
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                qjs::trueos_shims::log_info("qjs-browser: JS runtime init failed\n");
                BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
                return;
            }
        };
        let ctx = vm.ctx_ptr();
        let rt = vm.rt_ptr();

                qjs::node::install_globals(ctx);
                qjs::layout::install_layout_api(ctx);

                if !install_globals(ctx) {
                        BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
                        return;
                }

                let import_filename = b"<browser-init>\0";
                let import_src = br#"
        import('/qjs/browser/browser.mjs').catch((e) => {
            try { console.log('[browser.mjs] import failed', String(e && e.stack ? e.stack : e)); } catch (_) {}
        });
        "#;
                if !helpers::eval_or_log(
                    ctx,
                    import_src,
                    import_filename.as_ptr() as *const c_char,
                    qjs::JS_EVAL_TYPE_GLOBAL,
                    "browser init",
                ) {
                    BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
                    return;
                }

        loop {
            apply_pending_html(ctx);
            apply_pending_ai_input(ctx);
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }
        qjs::trueos_shims::log_info("qjs-browser: stopped\n");
        BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
    }
}
