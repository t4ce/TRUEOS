#![cfg(feature = "trueos")]
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use parry2d::math::{Isometry, Vector};
use parry2d::query;
use parry2d::shape::{Ball, Cuboid};
use spin::Mutex;
use crate as qjs;

mod ai_api;
mod helpers;

static BROWSER_TASK_STARTED: AtomicBool = AtomicBool::new(false);
#[derive(Clone)]
struct PendingHtmlEntry {
    html: String,
    url: Option<String>,
}

static PENDING_HTML: Mutex<Option<PendingHtmlEntry>> = Mutex::new(None);
static PENDING_AI_INPUT: Mutex<VecDeque<AiInputEntry>> = Mutex::new(VecDeque::new());

const PARRY_CURSOR_RADIUS: f32 = 6.0;
const MAX_PARRY_CURSOR_EVENTS_PER_TICK: usize = 32;

fn append_js_u8_array(dst: &mut String, values: &[u8]) {
    dst.push('[');
    for (idx, value) in values.iter().enumerate() {
        if idx != 0 {
            dst.push(',');
        }
        dst.push_str(alloc::format!("{}", value).as_str());
    }
    dst.push(']');
}

fn browser_text_widths_by_char() -> [u8; 256] {
    let mut out = [0u8; 256];
    let Some(atlas) = qjs::font_atlas_large_view() else {
        out.fill(8);
        return out;
    };

    for (ch, width) in out.iter_mut().enumerate() {
        let mut slot = atlas.index.get(ch).copied().unwrap_or(u16::MAX);
        if slot == u16::MAX {
            slot = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
        }
        *width = atlas
            .widths
            .get(slot as usize)
            .copied()
            .unwrap_or(atlas.cell_w as u8);
    }

    out
}

#[derive(Clone)]
struct ParryInteractiveRect {
    path: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Copy, Clone)]
struct ParryCursorCircle {
    slot_id: u32,
    x: f32,
    y: f32,
}

#[derive(Copy, Clone)]
struct CursorButtonEvent {
    slot_id: u32,
    x: f32,
    y: f32,
    buttons_down: u32,
    previous_buttons_down: u32,
}

#[derive(Clone)]
pub struct AiInputEntry {
    pub text: String,
    pub web_search: bool,
    pub file_search: bool,
    pub new_conversation: bool,
    pub computer_use: bool,
}

pub fn queue_set_html(next_html: String) -> bool {
    queue_set_html_with_url(next_html, None)
}

pub fn queue_set_html_with_url(next_html: String, next_url: Option<String>) -> bool {
    if !BROWSER_TASK_STARTED.load(Ordering::SeqCst) {
        return false;
    }
    *PENDING_HTML.lock() = Some(PendingHtmlEntry {
        html: next_html,
        url: next_url,
    });
    true
}

pub fn queue_ai_input(next: AiInputEntry) -> bool {
    if !BROWSER_TASK_STARTED.load(Ordering::SeqCst) {
        return false;
    }
    PENDING_AI_INPUT.lock().push_back(next);
    true
}

#[inline]
unsafe fn js_prop_f64(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &[u8]) -> Option<f64> {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    if value.is_exception() || value.tag == qjs::JS_TAG_UNDEFINED || value.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, value);
        return None;
    }
    let mut out = 0.0f64;
    let ok = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0;
    qjs::js_free_value(ctx, value);
    if ok { Some(out) } else { None }
}

#[inline]
unsafe fn js_prop_string(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
) -> Option<String> {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    if value.is_exception() || value.tag == qjs::JS_TAG_UNDEFINED || value.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, value);
        return None;
    }
    let mut len: usize = 0;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, value, 0);
    if cstr.is_null() {
        qjs::js_free_value(ctx, value);
        return None;
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let out = core::str::from_utf8(bytes).ok().map(String::from);
    qjs::JS_FreeCString(ctx, cstr);
    qjs::js_free_value(ctx, value);
    out
}

#[inline]
unsafe fn set_event_num_prop(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
    value: f64,
) {
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        obj,
        key.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, value),
    );
}

#[inline]
unsafe fn browser_viewport_center(ctx: *mut qjs::JSContext) -> (f32, f32) {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return (640.0, 400.0);
    }
    let width = js_prop_f64(ctx, global, b"innerWidth\0").unwrap_or(1280.0) as f32;
    let height = js_prop_f64(ctx, global, b"innerHeight\0").unwrap_or(800.0) as f32;
    qjs::js_free_value(ctx, global);
    (width * 0.5, height * 0.5)
}

unsafe fn browser_pop_cursor_button_event(ctx: *mut qjs::JSContext) -> Option<CursorButtonEvent> {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return None;
    }
    let browser = qjs::JS_GetPropertyStr(ctx, global, b"__trueosBrowser\0".as_ptr() as *const c_char);
    qjs::js_free_value(ctx, global);
    if browser.is_exception() || browser.tag == qjs::JS_TAG_UNDEFINED || browser.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, browser);
        return None;
    }
    let pop = qjs::JS_GetPropertyStr(ctx, browser, b"popCursorButtonEvent\0".as_ptr() as *const c_char);
    if pop.is_exception() || pop.tag == qjs::JS_TAG_UNDEFINED || pop.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, pop);
        qjs::js_free_value(ctx, browser);
        return None;
    }

    let result = qjs::JS_Call(ctx, pop, browser, 0, core::ptr::null());
    qjs::js_free_value(ctx, pop);
    if result.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "browser popCursorButtonEvent");
        qjs::js_free_value(ctx, result);
        qjs::js_free_value(ctx, browser);
        return None;
    }
    if result.tag == qjs::JS_TAG_UNDEFINED || result.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, result);
        qjs::js_free_value(ctx, browser);
        return None;
    }

    let event = CursorButtonEvent {
        slot_id: js_prop_f64(ctx, result, b"slotId\0").unwrap_or(0.0).max(0.0) as u32,
        x: js_prop_f64(ctx, result, b"x\0").unwrap_or(0.0) as f32,
        y: js_prop_f64(ctx, result, b"y\0").unwrap_or(0.0) as f32,
        buttons_down: js_prop_f64(ctx, result, b"buttonsDown\0").unwrap_or(0.0).max(0.0) as u32,
        previous_buttons_down: js_prop_f64(ctx, result, b"previousButtonsDown\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
    };
    qjs::js_free_value(ctx, result);
    qjs::js_free_value(ctx, browser);
    Some(event)
}

unsafe fn browser_collect_interactive_rects(ctx: *mut qjs::JSContext) -> Vec<ParryInteractiveRect> {
    let mut out = Vec::new();
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return out;
    }
    let interactives = qjs::JS_GetPropertyStr(
        ctx,
        global,
        b"__trueosBrowserThemeLayoutInteractives\0".as_ptr() as *const c_char,
    );
    qjs::js_free_value(ctx, global);
    if interactives.is_exception() || interactives.tag == qjs::JS_TAG_UNDEFINED || interactives.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, interactives);
        return out;
    }

    let len = js_prop_f64(ctx, interactives, b"length\0").unwrap_or(0.0).max(0.0) as usize;
    for idx in 0..len {
        let item = qjs::JS_GetPropertyUint32(ctx, interactives, idx as u32);
        if item.is_exception() || item.tag == qjs::JS_TAG_UNDEFINED || item.tag == qjs::JS_TAG_NULL {
            qjs::js_free_value(ctx, item);
            continue;
        }

        let Some(path) = js_prop_string(ctx, item, b"path\0") else {
            qjs::js_free_value(ctx, item);
            continue;
        };
        let rect = ParryInteractiveRect {
            path,
            x: js_prop_f64(ctx, item, b"x\0").unwrap_or(0.0) as f32,
            y: js_prop_f64(ctx, item, b"y\0").unwrap_or(0.0) as f32,
            width: js_prop_f64(ctx, item, b"width\0").unwrap_or(0.0).max(0.0) as f32,
            height: js_prop_f64(ctx, item, b"height\0").unwrap_or(0.0).max(0.0) as f32,
        };
        qjs::js_free_value(ctx, item);
        if rect.width <= 0.0 || rect.height <= 0.0 {
            continue;
        }
        out.push(rect);
    }

    qjs::js_free_value(ctx, interactives);
    out
}

unsafe fn browser_dispatch_dom_click(
    ctx: *mut qjs::JSContext,
    path: &str,
    slot_id: u32,
    x: f32,
    y: f32,
) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return false;
    }
    let browser = qjs::JS_GetPropertyStr(ctx, global, b"__trueosBrowser\0".as_ptr() as *const c_char);
    qjs::js_free_value(ctx, global);
    if browser.is_exception() || browser.tag == qjs::JS_TAG_UNDEFINED || browser.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, browser);
        return false;
    }
    let dispatch = qjs::JS_GetPropertyStr(ctx, browser, b"dispatchDomClick\0".as_ptr() as *const c_char);
    if dispatch.is_exception() || dispatch.tag == qjs::JS_TAG_UNDEFINED || dispatch.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, dispatch);
        qjs::js_free_value(ctx, browser);
        return false;
    }

    let path_js = qjs::JS_NewStringLen(ctx, path.as_ptr() as *const c_char, path.len());
    let event_js = qjs::JS_NewObject(ctx);
    set_event_num_prop(ctx, event_js, b"slotId\0", slot_id as f64);
    set_event_num_prop(ctx, event_js, b"x\0", x as f64);
    set_event_num_prop(ctx, event_js, b"y\0", y as f64);

    let args = [path_js, event_js];
    let result = qjs::JS_Call(ctx, dispatch, browser, args.len() as i32, args.as_ptr());
    let ok = !result.is_exception();
    if result.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "browser dispatchDomClick");
    }
    qjs::js_free_value(ctx, result);
    qjs::js_free_value(ctx, event_js);
    qjs::js_free_value(ctx, path_js);
    qjs::js_free_value(ctx, dispatch);
    qjs::js_free_value(ctx, browser);
    ok
}

unsafe fn process_parry_clicks(ctx: *mut qjs::JSContext, cursors: &mut Vec<ParryCursorCircle>) {
    let (center_x, center_y) = browser_viewport_center(ctx);
    for _ in 0..MAX_PARRY_CURSOR_EVENTS_PER_TICK {
        let Some(event) = browser_pop_cursor_button_event(ctx) else {
            break;
        };
        if event.slot_id == 0 || !(event.previous_buttons_down != 0 && event.buttons_down == 0) {
            continue;
        }

        let circle = if let Some(existing) = cursors.iter_mut().find(|cursor| cursor.slot_id == event.slot_id) {
            existing
        } else {
            cursors.push(ParryCursorCircle {
                slot_id: event.slot_id,
                x: center_x,
                y: center_y,
            });
            let Some(last) = cursors.last_mut() else {
                continue;
            };
            last
        };
        circle.x = event.x;
        circle.y = event.y;

        let cursor_iso = Isometry::translation(circle.x, circle.y);
        let cursor_shape = Ball::new(PARRY_CURSOR_RADIUS);
        let interactives = browser_collect_interactive_rects(ctx);
        let mut hit_path: Option<String> = None;
        for interactive in interactives {
            let half_w = (interactive.width * 0.5).max(0.5);
            let half_h = (interactive.height * 0.5).max(0.5);
            let interactive_iso = Isometry::translation(interactive.x + half_w, interactive.y + half_h);
            let interactive_shape = Cuboid::new(Vector::new(half_w, half_h));
            let Ok(Some(contact)) = query::contact(&cursor_iso, &cursor_shape, &interactive_iso, &interactive_shape, 0.0) else {
                continue;
            };
            if contact.dist <= 0.0 {
                hit_path = Some(interactive.path);
            }
        }

        if let Some(path) = hit_path {
            let _ = browser_dispatch_dom_click(ctx, path.as_str(), event.slot_id, event.x, event.y);
        }
    }
}

unsafe fn apply_pending_html(ctx: *mut qjs::JSContext) {
    let Some(next) = PENDING_HTML.lock().take() else {
        return;
    };

    let html_lit = helpers::js_single_quoted_literal(next.html.as_str());
    let url_lit = next.url.as_ref().map(|url| helpers::js_single_quoted_literal(url.as_str()));
    let mut src = String::new();
    src.push_str("(function(){const __g=(typeof globalThis!=='undefined')?globalThis:this;const __h=");
    src.push_str(&html_lit);
    if let Some(url_lit) = url_lit {
        src.push_str(";const __u=");
        src.push_str(&url_lit);
        src.push_str(";__g.__trueosBrowserUrl=__u;__g.__trueosBrowserCurrentUrl=__u;");
        src.push_str("if(__g.__trueosBrowser&&typeof __g.__trueosBrowser.setCurrentPageUrl==='function'){__g.__trueosBrowser.setCurrentPageUrl(__u);} ");
    } else {
        src.push_str(";");
    }
    src.push_str("if(__g.__trueosBrowser&&typeof __g.__trueosBrowser.setHtml==='function'){__g.__trueosBrowser.setHtml(__h);}else{__g.__trueosUiHtml=__h;}})();");
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
    src.push_str(",fileSearch:");
    src.push_str(if next.file_search { "true" } else { "false" });
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
    let mut init_src = String::new();
    let text_widths = browser_text_widths_by_char();
    init_src.push_str("\nconst G = (typeof globalThis !== 'undefined') ? globalThis : this;\n");
    qjs::html::append_embedded_browser_globals_js(&mut init_src);
    init_src.push_str("G.__trueosBrowserTextWidthByChar = ");
    append_js_u8_array(&mut init_src, &text_widths);
    init_src.push_str(";\n");
    init_src.push_str("G.__trueosBrowserDefaultFontPx = 16;\n");
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

        let mut parry_cursors: Vec<ParryCursorCircle> = Vec::new();

        loop {
            apply_pending_html(ctx);
            apply_pending_ai_input(ctx);
            process_parry_clicks(ctx, &mut parry_cursors);
            if !pump_runtime_once(rt, ctx) {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }
        qjs::trueos_shims::log_info("qjs-browser: stopped\n");
        BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
    }
}
