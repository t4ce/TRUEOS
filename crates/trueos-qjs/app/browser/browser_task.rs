#![cfg(feature = "trueos")]
use crate as qjs;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

mod ai_api;
mod helpers;

static BROWSER_TASK_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedBrowserRegion {
    pub tex_id: u32,
    pub doc_y: u32,
    pub width: u32,
    pub height: u32,
    pub revision: u32,
    pub dirty: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedBrowserSurfaceState {
    pub seq: u32,
    pub cache_revision: u32,
    pub cache_width: u32,
    pub tile_height: u32,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub content_height: u32,
    pub content_top_y: u32,
    pub scroll_y: u32,
    pub regions: Vec<HostedBrowserRegion>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedBrowserInteractive {
    pub item_id: u32,
    pub kind_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedBrowserInteractiveState {
    pub seq: u32,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub interactives: Vec<HostedBrowserInteractive>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HostedViewportRequest {
    viewport_width: u32,
    viewport_height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
}

#[derive(Clone)]
struct PendingHtmlEntry {
    html: String,
    url: Option<String>,
}

static PENDING_HTML: Mutex<Option<PendingHtmlEntry>> = Mutex::new(None);
static PENDING_AI_INPUT: Mutex<VecDeque<AiInputEntry>> = Mutex::new(VecDeque::new());
static PENDING_QJS_INPUT: Mutex<VecDeque<QjsInputEntry>> = Mutex::new(VecDeque::new());
static PENDING_HOSTED_VIEWPORT: Mutex<Option<HostedViewportRequest>> = Mutex::new(None);
static PENDING_HOSTED_SCROLL_Y: Mutex<Option<u32>> = Mutex::new(None);
static APPLIED_HOSTED_VIEWPORT: Mutex<Option<HostedViewportRequest>> = Mutex::new(None);
static HOSTED_SURFACE_STATE: Mutex<HostedBrowserSurfaceState> =
    Mutex::new(HostedBrowserSurfaceState {
        seq: 0,
        cache_revision: 0,
        cache_width: 0,
        tile_height: 0,
        viewport_width: 0,
        viewport_height: 0,
        content_height: 0,
        content_top_y: 0,
        scroll_y: 0,
        regions: Vec::new(),
    });
static HOSTED_SURFACE_SEQ: AtomicU32 = AtomicU32::new(0);
static HOSTED_INTERACTIVE_STATE: Mutex<HostedBrowserInteractiveState> =
    Mutex::new(HostedBrowserInteractiveState {
        seq: 0,
        viewport_width: 0,
        viewport_height: 0,
        interactives: Vec::new(),
    });
static HOSTED_INTERACTIVE_SEQ: AtomicU32 = AtomicU32::new(0);

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
pub struct AiInputEntry {
    pub text: String,
    pub web_search: bool,
    pub file_search: bool,
    pub new_conversation: bool,
    pub computer_use: bool,
}

#[derive(Clone)]
pub struct QjsInputEntry {
    pub code: String,
    pub repl: bool,
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

pub fn queue_qjs_input(next: QjsInputEntry) -> bool {
    if !BROWSER_TASK_STARTED.load(Ordering::SeqCst) {
        return false;
    }
    PENDING_QJS_INPUT.lock().push_back(next);
    true
}

pub fn set_hosted_viewport(
    viewport_width: u32,
    viewport_height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
) -> bool {
    if !BROWSER_TASK_STARTED.load(Ordering::SeqCst) {
        return false;
    }
    let next = HostedViewportRequest {
        viewport_width: viewport_width.max(1),
        viewport_height: viewport_height.max(1),
        content_x,
        content_y,
        content_width: content_width.max(1),
        content_height: content_height.max(1),
    };
    {
        let applied = APPLIED_HOSTED_VIEWPORT.lock();
        if applied.as_ref() == Some(&next) {
            let pending = PENDING_HOSTED_VIEWPORT.lock();
            if pending.is_none() {
                return true;
            }
        }
    }
    *PENDING_HOSTED_VIEWPORT.lock() = Some(next);
    true
}

pub fn set_hosted_scroll_y(scroll_y: u32) -> bool {
    if !BROWSER_TASK_STARTED.load(Ordering::SeqCst) {
        return false;
    }
    *PENDING_HOSTED_SCROLL_Y.lock() = Some(scroll_y);
    true
}

pub fn hosted_surface_state() -> HostedBrowserSurfaceState {
    HOSTED_SURFACE_STATE.lock().clone()
}

pub fn hosted_surface_seq() -> u32 {
    HOSTED_SURFACE_SEQ.load(Ordering::Acquire)
}

pub fn hosted_interactive_state() -> HostedBrowserInteractiveState {
    HOSTED_INTERACTIVE_STATE.lock().clone()
}

pub fn hosted_interactive_seq() -> u32 {
    HOSTED_INTERACTIVE_SEQ.load(Ordering::Acquire)
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

unsafe fn browser_api_value(ctx: *mut qjs::JSContext) -> Option<qjs::JSValue> {
    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        return None;
    }
    let browser = qjs::JS_GetPropertyStr(ctx, global, b"__trueosBrowser\0".as_ptr() as *const c_char);
    qjs::js_free_value(ctx, global);
    if browser.is_exception()
        || browser.tag == qjs::JS_TAG_UNDEFINED
        || browser.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, browser);
        return None;
    }
    Some(browser)
}

unsafe fn apply_pending_hosted_viewport(ctx: *mut qjs::JSContext) {
    let Some(next) = PENDING_HOSTED_VIEWPORT.lock().take() else {
        return;
    };
    let Some(browser) = browser_api_value(ctx) else {
        *PENDING_HOSTED_VIEWPORT.lock() = Some(next);
        return;
    };
    let func = qjs::JS_GetPropertyStr(
        ctx,
        browser,
        b"setViewportOverride\0".as_ptr() as *const c_char,
    );
    if func.is_exception() || func.tag == qjs::JS_TAG_UNDEFINED || func.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, func);
        qjs::js_free_value(ctx, browser);
        *PENDING_HOSTED_VIEWPORT.lock() = Some(next);
        return;
    }

    let viewport = qjs::JS_NewObject(ctx);
    set_event_num_prop(ctx, viewport, b"width\0", next.viewport_width as f64);
    set_event_num_prop(ctx, viewport, b"height\0", next.viewport_height as f64);

    let content_rect = qjs::JS_NewObject(ctx);
    set_event_num_prop(ctx, content_rect, b"x\0", next.content_x as f64);
    set_event_num_prop(ctx, content_rect, b"y\0", next.content_y as f64);
    set_event_num_prop(ctx, content_rect, b"width\0", next.content_width as f64);
    set_event_num_prop(ctx, content_rect, b"height\0", next.content_height as f64);

    let args = [viewport, content_rect];
    let result = qjs::JS_Call(ctx, func, browser, args.len() as i32, args.as_ptr());
    if result.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "browser setViewportOverride");
        *PENDING_HOSTED_VIEWPORT.lock() = Some(next);
    } else {
        *APPLIED_HOSTED_VIEWPORT.lock() = Some(next);
    }
    qjs::js_free_value(ctx, result);
    qjs::js_free_value(ctx, content_rect);
    qjs::js_free_value(ctx, viewport);
    qjs::js_free_value(ctx, func);
    qjs::js_free_value(ctx, browser);
}

unsafe fn apply_pending_hosted_scroll_y(ctx: *mut qjs::JSContext) {
    let Some(next_scroll_y) = PENDING_HOSTED_SCROLL_Y.lock().take() else {
        return;
    };
    let Some(browser) = browser_api_value(ctx) else {
        *PENDING_HOSTED_SCROLL_Y.lock() = Some(next_scroll_y);
        return;
    };
    let func = qjs::JS_GetPropertyStr(ctx, browser, b"setScroll\0".as_ptr() as *const c_char);
    if func.is_exception() || func.tag == qjs::JS_TAG_UNDEFINED || func.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, func);
        qjs::js_free_value(ctx, browser);
        *PENDING_HOSTED_SCROLL_Y.lock() = Some(next_scroll_y);
        return;
    }

    let args = [qjs::JS_NewFloat64(ctx, next_scroll_y as f64)];
    let result = qjs::JS_Call(ctx, func, browser, args.len() as i32, args.as_ptr());
    if result.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "browser setScroll");
        *PENDING_HOSTED_SCROLL_Y.lock() = Some(next_scroll_y);
    }
    qjs::js_free_value(ctx, result);
    qjs::js_free_value(ctx, args[0]);
    qjs::js_free_value(ctx, func);
    qjs::js_free_value(ctx, browser);
}

unsafe fn sync_hosted_surface_state(ctx: *mut qjs::JSContext) {
    let Some(browser) = browser_api_value(ctx) else {
        return;
    };
    let func = qjs::JS_GetPropertyStr(
        ctx,
        browser,
        b"getSurfaceState\0".as_ptr() as *const c_char,
    );
    if func.is_exception() || func.tag == qjs::JS_TAG_UNDEFINED || func.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, func);
        qjs::js_free_value(ctx, browser);
        return;
    }
    let result = qjs::JS_Call(ctx, func, browser, 0, core::ptr::null());
    qjs::js_free_value(ctx, func);
    qjs::js_free_value(ctx, browser);
    if result.is_exception() || result.tag == qjs::JS_TAG_UNDEFINED || result.tag == qjs::JS_TAG_NULL {
        if result.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "browser getSurfaceState");
        }
        qjs::js_free_value(ctx, result);
        return;
    }

    let mut next = HostedBrowserSurfaceState {
        seq: 0,
        cache_revision: js_prop_f64(ctx, result, b"cacheRevision\0").unwrap_or(0.0).max(0.0) as u32,
        cache_width: js_prop_f64(ctx, result, b"cacheWidth\0").unwrap_or(0.0).max(0.0) as u32,
        tile_height: js_prop_f64(ctx, result, b"tileHeight\0").unwrap_or(0.0).max(0.0) as u32,
        viewport_width: js_prop_f64(ctx, result, b"viewportWidth\0").unwrap_or(0.0).max(0.0) as u32,
        viewport_height: js_prop_f64(ctx, result, b"viewportHeight\0").unwrap_or(0.0).max(0.0) as u32,
        content_height: js_prop_f64(ctx, result, b"contentHeight\0").unwrap_or(0.0).max(0.0) as u32,
        content_top_y: js_prop_f64(ctx, result, b"contentTopY\0").unwrap_or(0.0).max(0.0) as u32,
        scroll_y: js_prop_f64(ctx, result, b"scrollY\0").unwrap_or(0.0).max(0.0) as u32,
        regions: Vec::new(),
    };

    let regions = qjs::JS_GetPropertyStr(ctx, result, b"regions\0".as_ptr() as *const c_char);
    if !regions.is_exception() && regions.tag != qjs::JS_TAG_UNDEFINED && regions.tag != qjs::JS_TAG_NULL {
        let len = js_prop_f64(ctx, regions, b"length\0").unwrap_or(0.0).max(0.0) as usize;
        for idx in 0..len {
            let item = qjs::JS_GetPropertyUint32(ctx, regions, idx as u32);
            if item.is_exception() || item.tag == qjs::JS_TAG_UNDEFINED || item.tag == qjs::JS_TAG_NULL {
                qjs::js_free_value(ctx, item);
                continue;
            }
            next.regions.push(HostedBrowserRegion {
                tex_id: js_prop_f64(ctx, item, b"texId\0").unwrap_or(0.0).max(0.0) as u32,
                doc_y: js_prop_f64(ctx, item, b"docY\0").unwrap_or(0.0).max(0.0) as u32,
                width: js_prop_f64(ctx, item, b"width\0").unwrap_or(0.0).max(0.0) as u32,
                height: js_prop_f64(ctx, item, b"height\0").unwrap_or(0.0).max(0.0) as u32,
                revision: js_prop_f64(ctx, item, b"revision\0").unwrap_or(0.0).max(0.0) as u32,
                dirty: js_prop_f64(ctx, item, b"dirty\0").unwrap_or(0.0) != 0.0,
            });
            qjs::js_free_value(ctx, item);
        }
    }
    qjs::js_free_value(ctx, regions);
    qjs::js_free_value(ctx, result);

    let mut shared = HOSTED_SURFACE_STATE.lock();
    let prev = shared.clone();
    next.seq = prev.seq;
    if prev != next {
        let mut seq = HOSTED_SURFACE_SEQ.load(Ordering::Acquire).wrapping_add(1);
        if seq == 0 {
            seq = 1;
        }
        next.seq = seq;
        HOSTED_SURFACE_SEQ.store(seq, Ordering::Release);
        *shared = next;
    }
}

unsafe fn sync_hosted_interactive_state(ctx: *mut qjs::JSContext) {
    let Some(browser) = browser_api_value(ctx) else {
        return;
    };
    let func = qjs::JS_GetPropertyStr(
        ctx,
        browser,
        b"getInteractiveState\0".as_ptr() as *const c_char,
    );
    if func.is_exception() || func.tag == qjs::JS_TAG_UNDEFINED || func.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, func);
        qjs::js_free_value(ctx, browser);
        return;
    }
    let result = qjs::JS_Call(ctx, func, browser, 0, core::ptr::null());
    qjs::js_free_value(ctx, func);
    qjs::js_free_value(ctx, browser);
    if result.is_exception() || result.tag == qjs::JS_TAG_UNDEFINED || result.tag == qjs::JS_TAG_NULL
    {
        if result.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "browser getInteractiveState");
        }
        qjs::js_free_value(ctx, result);
        return;
    }

    let mut next = HostedBrowserInteractiveState {
        seq: 0,
        viewport_width: js_prop_f64(ctx, result, b"viewportWidth\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        viewport_height: js_prop_f64(ctx, result, b"viewportHeight\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        interactives: Vec::new(),
    };

    let interactives =
        qjs::JS_GetPropertyStr(ctx, result, b"interactives\0".as_ptr() as *const c_char);
    if !interactives.is_exception()
        && interactives.tag != qjs::JS_TAG_UNDEFINED
        && interactives.tag != qjs::JS_TAG_NULL
    {
        let len = js_prop_f64(ctx, interactives, b"length\0")
            .unwrap_or(0.0)
            .max(0.0) as usize;
        for idx in 0..len {
            let item = qjs::JS_GetPropertyUint32(ctx, interactives, idx as u32);
            if item.is_exception()
                || item.tag == qjs::JS_TAG_UNDEFINED
                || item.tag == qjs::JS_TAG_NULL
            {
                qjs::js_free_value(ctx, item);
                continue;
            }
            next.interactives.push(HostedBrowserInteractive {
                item_id: js_prop_f64(ctx, item, b"itemId\0").unwrap_or(0.0).max(0.0) as u32,
                kind_id: js_prop_f64(ctx, item, b"kindId\0").unwrap_or(0.0).max(0.0) as u32,
                x: js_prop_f64(ctx, item, b"x\0").unwrap_or(0.0).max(0.0) as u32,
                y: js_prop_f64(ctx, item, b"y\0").unwrap_or(0.0).max(0.0) as u32,
                width: js_prop_f64(ctx, item, b"width\0").unwrap_or(0.0).max(0.0) as u32,
                height: js_prop_f64(ctx, item, b"height\0").unwrap_or(0.0).max(0.0) as u32,
            });
            qjs::js_free_value(ctx, item);
        }
    }
    qjs::js_free_value(ctx, interactives);
    qjs::js_free_value(ctx, result);

    let mut shared = HOSTED_INTERACTIVE_STATE.lock();
    let prev = shared.clone();
    next.seq = prev.seq;
    if prev != next {
        let mut seq = HOSTED_INTERACTIVE_SEQ.load(Ordering::Acquire).wrapping_add(1);
        if seq == 0 {
            seq = 1;
        }
        next.seq = seq;
        HOSTED_INTERACTIVE_SEQ.store(seq, Ordering::Release);
        *shared = next;
    }
}

unsafe fn apply_pending_html(ctx: *mut qjs::JSContext) {
    let Some(next) = PENDING_HTML.lock().take() else {
        return;
    };

    let html_lit = helpers::js_single_quoted_literal(next.html.as_str());
    let url_lit = next
        .url
        .as_ref()
        .map(|url| helpers::js_single_quoted_literal(url.as_str()));
    let mut src = String::new();
    src.push_str(
        "(function(){const __g=(typeof globalThis!=='undefined')?globalThis:this;const __h=",
    );
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
    src.push_str(if next.new_conversation {
        "true"
    } else {
        "false"
    });
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

unsafe fn apply_pending_qjs_input(ctx: *mut qjs::JSContext) {
    let Some(next) = PENDING_QJS_INPUT.lock().pop_front() else {
        return;
    };

    let code_lit = helpers::js_single_quoted_literal(next.code.as_str());
    let mut src = String::new();
    src.push_str("(function(){const __g=(typeof globalThis!=='undefined')?globalThis:this;");
    src.push_str("if(typeof __g.__trueosQjsInputPush!=='function')return;");
    src.push_str("__g.__trueosQjsInputPush({code:");
    src.push_str(&code_lit);
    src.push_str(",repl:");
    src.push_str(if next.repl { "true" } else { "false" });
    src.push_str("});})();");

    let _ = helpers::eval_or_log(
        ctx,
        src.as_bytes(),
        b"<browser-qjs-input>\0".as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
        "browser qjs input",
    );
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
if (!G.window) G.window = G;
if (typeof G.window.innerWidth !== 'number') G.window.innerWidth = 1280;
if (typeof G.window.innerHeight !== 'number') G.window.innerHeight = 800;
if (!G.__trueosBrowserViewport || typeof G.__trueosBrowserViewport !== 'object') {
    G.__trueosBrowserViewport = {
        width: G.window.innerWidth,
        height: G.window.innerHeight,
    };
}
if (!G.__trueosBrowserContentRect || typeof G.__trueosBrowserContentRect !== 'object') {
    G.__trueosBrowserContentRect = {
        x: 0,
        y: 0,
        width: G.__trueosBrowserViewport.width,
        height: G.__trueosBrowserViewport.height,
    };
}
G.__trueosBrowserHostedByUi2 = true;
if (typeof G.addEventListener !== 'function') G.addEventListener = () => {};
if (typeof G.removeEventListener !== 'function') G.removeEventListener = () => {};
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
    let spawner = unsafe { Spawner::for_current_executor().await };
    if !qjs::async_fs::ensure_service_started(&spawner) {
        qjs::trueos_shims::log_error("qjs-browser: async-fs service start failed\n");
        BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
        return;
    }
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
        import('/qjs/browser/browser_bootstrap.mjs').catch((e) => {
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
            apply_pending_qjs_input(ctx);
            apply_pending_hosted_viewport(ctx);
            apply_pending_hosted_scroll_y(ctx);
            if !qjs::vm::pump_runtime_once(rt, ctx, "browser") {
                break;
            }
            sync_hosted_surface_state(ctx);
            sync_hosted_interactive_state(ctx);
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }
        qjs::trueos_shims::log_info("qjs-browser: stopped\n");
        BROWSER_TASK_STARTED.store(false, Ordering::SeqCst);
    }
}
