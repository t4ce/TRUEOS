#![cfg(feature = "trueos")]
use crate as qjs;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

mod ai_api;
mod helpers;

unsafe extern "C" {
    fn trueos_cabi_ui2_primary_browser_window_id() -> u32;
}

pub const PRIMARY_BROWSER_INSTANCE_ID: u32 = 1;
const STATIC_BROWSER_VIEWPORT_W: u32 = 512;
const STATIC_BROWSER_VIEWPORT_H: u32 = 512;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BrowserHostTarget {
    pub instance_id: u32,
    pub window_id: u32,
}

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
    pub content_width: u32,
    pub content_height: u32,
    pub content_top_y: u32,
    pub scroll_x: u32,
    pub scroll_y: u32,
    pub regions: Vec<HostedBrowserRegion>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HostedScrollRequest {
    scroll_x: u32,
    scroll_y: u32,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedBrowserTextRow {
    pub depth: u32,
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedBrowserTextState {
    pub seq: u32,
    pub rows: Vec<HostedBrowserTextRow>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct HostedViewportRequest {
    browser_instance_id: u32,
    viewport_width: u32,
    viewport_height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
}

#[derive(Clone, Debug)]
struct PendingHtmlEntry {
    browser_instance_id: u32,
    html: String,
    url: Option<String>,
}

#[derive(Clone, Debug)]
struct BrowserRpcRequest {
    id: u32,
    browser_instance_id: u32,
    browser_window_id: u32,
    method: String,
    args_json: String,
}

#[derive(Clone)]
struct BrowserRpcResult {
    id: u32,
    payload_json: String,
}

#[derive(Clone, Debug)]
struct BrowserHostState {
    instance_id: u32,
    started: bool,
    window_id: u32,
    pending_html: Option<PendingHtmlEntry>,
    pending_qjs_input: VecDeque<QjsInputEntry>,
    pending_browser_rpc: VecDeque<BrowserRpcRequest>,
    active_browser_rpc_id: Option<u32>,
    pending_hosted_viewport: Option<HostedViewportRequest>,
    pending_hosted_scroll: Option<HostedScrollRequest>,
    applied_hosted_viewport: Option<HostedViewportRequest>,
    hosted_surface_state: HostedBrowserSurfaceState,
    hosted_surface_seq: u32,
    hosted_interactive_state: HostedBrowserInteractiveState,
    hosted_interactive_seq: u32,
    hosted_text_state: HostedBrowserTextState,
    hosted_text_seq: u32,
}

impl BrowserHostState {
    fn new(instance_id: u32) -> Self {
        Self {
            instance_id,
            started: false,
            window_id: 0,
            pending_html: None,
            pending_qjs_input: VecDeque::new(),
            pending_browser_rpc: VecDeque::new(),
            active_browser_rpc_id: None,
            pending_hosted_viewport: None,
            pending_hosted_scroll: None,
            applied_hosted_viewport: None,
            hosted_surface_state: HostedBrowserSurfaceState::default(),
            hosted_surface_seq: 0,
            hosted_interactive_state: HostedBrowserInteractiveState::default(),
            hosted_interactive_seq: 0,
            hosted_text_state: HostedBrowserTextState::default(),
            hosted_text_seq: 0,
        }
    }
}

static BROWSER_HOST_STATES: Mutex<Vec<BrowserHostState>> = Mutex::new(Vec::new());
static BROWSER_RPC_RESULTS: Mutex<VecDeque<BrowserRpcResult>> = Mutex::new(VecDeque::new());
static BROWSER_RPC_SEQ: AtomicU32 = AtomicU32::new(1);

pub const HOSTED_KEYBOARD_MOD_SHIFT: u8 = 1 << 0;
pub const HOSTED_KEYBOARD_MOD_CTRL: u8 = 1 << 1;
pub const HOSTED_KEYBOARD_MOD_ALT: u8 = 1 << 2;
pub const HOSTED_KEYBOARD_MOD_META: u8 = 1 << 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostedKeyboardEvent {
    Text { text: String },
    Key { key: String, modifiers: u8 },
}

fn append_json_string(dst: &mut String, value: &str) {
    dst.push('"');
    for ch in value.chars() {
        match ch {
            '"' => dst.push_str("\\\""),
            '\\' => dst.push_str("\\\\"),
            '\u{0008}' => dst.push_str("\\b"),
            '\u{000C}' => dst.push_str("\\f"),
            '\n' => dst.push_str("\\n"),
            '\r' => dst.push_str("\\r"),
            '\t' => dst.push_str("\\t"),
            ch if ch <= '\u{001F}' => {
                dst.push_str(alloc::format!("\\u{:04X}", ch as u32).as_str());
            }
            _ => dst.push(ch),
        }
    }
    dst.push('"');
}

fn append_keyboard_modifiers_json(dst: &mut String, modifiers: u8) {
    dst.push('[');
    let mut first = true;
    for (bit, name) in [
        (HOSTED_KEYBOARD_MOD_SHIFT, "Shift"),
        (HOSTED_KEYBOARD_MOD_CTRL, "Ctrl"),
        (HOSTED_KEYBOARD_MOD_ALT, "Alt"),
        (HOSTED_KEYBOARD_MOD_META, "Meta"),
    ] {
        if (modifiers & bit) == 0 {
            continue;
        }
        if !first {
            dst.push(',');
        }
        first = false;
        append_json_string(dst, name);
    }
    dst.push(']');
}

pub fn queue_hosted_keyboard_events(
    browser_window_id: u32,
    events: &[HostedKeyboardEvent],
) -> bool {
    if events.is_empty() {
        return true;
    }
    let browser_instance_id = browser_instance_id_for_window(browser_window_id);
    if browser_instance_id == 0 || !browser_started(browser_instance_id) {
        return false;
    }

    let mut args_json = String::from("[[");
    for (idx, event) in events.iter().enumerate() {
        if idx != 0 {
            args_json.push(',');
        }
        match event {
            HostedKeyboardEvent::Text { text } => {
                args_json.push_str("{\"type\":\"text\",\"text\":");
                append_json_string(&mut args_json, text.as_str());
                args_json.push('}');
            }
            HostedKeyboardEvent::Key { key, modifiers } => {
                args_json.push_str("{\"type\":\"key\",\"key\":");
                append_json_string(&mut args_json, key.as_str());
                args_json.push_str(",\"modifiers\":");
                append_keyboard_modifiers_json(&mut args_json, *modifiers);
                args_json.push('}');
            }
        }
    }
    args_json.push_str("],{\"logOnly\":false}]");
    queue_browser_rpc_for_browser(
        browser_instance_id,
        String::from("keyboard"),
        args_json,
        browser_window_id,
    ) != 0
}

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

#[inline]
fn normalize_browser_instance_id(instance_id: u32) -> u32 {
    if instance_id == 0 {
        PRIMARY_BROWSER_INSTANCE_ID
    } else {
        instance_id
    }
}

fn ensure_browser_host_state(
    states: &mut Vec<BrowserHostState>,
    instance_id: u32,
) -> &mut BrowserHostState {
    let instance_id = normalize_browser_instance_id(instance_id);
    if let Some(index) = states
        .iter()
        .position(|state| state.instance_id == instance_id)
    {
        return &mut states[index];
    }
    states.push(BrowserHostState::new(instance_id));
    states
        .last_mut()
        .expect("browser host state list must contain the newly inserted state")
}

fn with_browser_host_state_mut<R>(
    instance_id: u32,
    f: impl FnOnce(&mut BrowserHostState) -> R,
) -> R {
    let mut states = BROWSER_HOST_STATES.lock();
    let state = ensure_browser_host_state(&mut states, instance_id);
    f(state)
}

fn with_browser_host_state<R>(instance_id: u32, f: impl FnOnce(&BrowserHostState) -> R) -> R {
    let mut states = BROWSER_HOST_STATES.lock();
    let state = ensure_browser_host_state(&mut states, instance_id);
    f(state)
}

fn browser_started(instance_id: u32) -> bool {
    with_browser_host_state(instance_id, |state| state.started)
}

pub fn bind_browser_window_to_instance(browser_instance_id: u32, browser_window_id: u32) -> bool {
    let browser_instance_id = normalize_browser_instance_id(browser_instance_id);
    with_browser_host_state_mut(browser_instance_id, |state| {
        state.window_id = browser_window_id;
    });
    true
}

pub fn browser_window_id_for_instance(browser_instance_id: u32) -> u32 {
    let browser_instance_id = normalize_browser_instance_id(browser_instance_id);
    with_browser_host_state(browser_instance_id, |state| {
        if state.window_id != 0 {
            return state.window_id;
        }
        if browser_instance_id == PRIMARY_BROWSER_INSTANCE_ID {
            unsafe { trueos_cabi_ui2_primary_browser_window_id() }
        } else {
            0
        }
    })
}

pub fn browser_instance_id_for_window(browser_window_id: u32) -> u32 {
    if browser_window_id == 0 {
        return PRIMARY_BROWSER_INSTANCE_ID;
    }
    let states = BROWSER_HOST_STATES.lock();
    if let Some(state) = states
        .iter()
        .find(|state| state.window_id == browser_window_id)
    {
        return state.instance_id;
    }
    let primary_window_id = unsafe { trueos_cabi_ui2_primary_browser_window_id() };
    if browser_window_id == primary_window_id {
        PRIMARY_BROWSER_INSTANCE_ID
    } else {
        0
    }
}

#[inline]
pub fn primary_browser_instance_id() -> u32 {
    PRIMARY_BROWSER_INSTANCE_ID
}

#[inline]
pub fn primary_browser_started() -> bool {
    browser_started(PRIMARY_BROWSER_INSTANCE_ID)
}

#[inline]
pub fn active_browser_instance_id() -> u32 {
    if browser_started(PRIMARY_BROWSER_INSTANCE_ID) {
        return PRIMARY_BROWSER_INSTANCE_ID;
    }
    let states = BROWSER_HOST_STATES.lock();
    states
        .iter()
        .find(|state| state.started)
        .map(|state| state.instance_id)
        .unwrap_or(0)
}

#[inline]
pub fn active_browser_target() -> BrowserHostTarget {
    let instance_id = active_browser_instance_id();
    BrowserHostTarget {
        instance_id,
        window_id: browser_window_id_for_instance(instance_id),
    }
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

#[derive(Clone, Debug)]
pub struct QjsInputEntry {
    pub code: String,
    pub repl: bool,
}

pub fn queue_set_html(next_html: String) -> bool {
    queue_set_html_with_url(next_html, None)
}

pub fn queue_set_html_with_url(next_html: String, next_url: Option<String>) -> bool {
    queue_set_html_with_url_for_browser(PRIMARY_BROWSER_INSTANCE_ID, next_html, next_url)
}

pub fn queue_set_html_with_url_for_browser(
    browser_instance_id: u32,
    next_html: String,
    next_url: Option<String>,
) -> bool {
    let browser_instance_id = normalize_browser_instance_id(browser_instance_id);
    if !browser_started(browser_instance_id) {
        return false;
    }
    with_browser_host_state_mut(browser_instance_id, |state| {
        state.pending_html = Some(PendingHtmlEntry {
            browser_instance_id,
            html: next_html,
            url: next_url,
        });
    });
    true
}

pub fn queue_qjs_input(next: QjsInputEntry) -> bool {
    queue_qjs_input_for_browser(PRIMARY_BROWSER_INSTANCE_ID, next)
}

pub fn queue_qjs_input_for_browser(browser_instance_id: u32, next: QjsInputEntry) -> bool {
    let browser_instance_id = normalize_browser_instance_id(browser_instance_id);
    if !browser_started(browser_instance_id) {
        return false;
    }
    with_browser_host_state_mut(browser_instance_id, |state| {
        state.pending_qjs_input.push_back(next);
    });
    true
}

pub fn queue_browser_rpc(method: String, args_json: String, browser_window_id: u32) -> u32 {
    queue_browser_rpc_for_browser(
        PRIMARY_BROWSER_INSTANCE_ID,
        method,
        args_json,
        browser_window_id,
    )
}

pub fn queue_browser_rpc_for_browser(
    browser_instance_id: u32,
    method: String,
    args_json: String,
    browser_window_id: u32,
) -> u32 {
    let browser_instance_id = normalize_browser_instance_id(browser_instance_id);
    if !browser_started(browser_instance_id) {
        return 0;
    }
    let id = BROWSER_RPC_SEQ.fetch_add(1, Ordering::Relaxed);
    with_browser_host_state_mut(browser_instance_id, |state| {
        state.pending_browser_rpc.push_back(BrowserRpcRequest {
            id,
            browser_instance_id,
            browser_window_id,
            method,
            args_json,
        });
    });
    id
}

pub fn take_browser_rpc_result(id: u32) -> Option<String> {
    if id == 0 {
        return None;
    }
    let mut guard = BROWSER_RPC_RESULTS.lock();
    let pos = guard.iter().position(|entry| entry.id == id)?;
    let entry = guard.remove(pos)?;
    Some(entry.payload_json)
}

pub fn set_hosted_viewport(
    viewport_width: u32,
    viewport_height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
) -> bool {
    set_hosted_viewport_for_browser(
        PRIMARY_BROWSER_INSTANCE_ID,
        viewport_width,
        viewport_height,
        content_x,
        content_y,
        content_width,
        content_height,
    )
}

pub fn set_hosted_viewport_for_browser(
    browser_instance_id: u32,
    viewport_width: u32,
    viewport_height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
) -> bool {
    let browser_instance_id = normalize_browser_instance_id(browser_instance_id);
    if !browser_started(browser_instance_id) {
        return false;
    }
    let next = HostedViewportRequest {
        browser_instance_id,
        viewport_width: viewport_width.max(1),
        viewport_height: viewport_height.max(1),
        content_x,
        content_y,
        content_width: content_width.max(1),
        content_height: content_height.max(1),
    };
    let should_skip = with_browser_host_state(browser_instance_id, |state| {
        state.applied_hosted_viewport.as_ref() == Some(&next)
            && state.pending_hosted_viewport.is_none()
    });
    if should_skip {
        return true;
    }
    with_browser_host_state_mut(browser_instance_id, |state| {
        state.pending_hosted_viewport = Some(next);
    });
    true
}

pub fn set_hosted_scroll_y(scroll_y: u32) -> bool {
    set_hosted_scroll_y_for_browser(PRIMARY_BROWSER_INSTANCE_ID, scroll_y)
}

pub fn set_hosted_scroll_y_for_browser(browser_instance_id: u32, scroll_y: u32) -> bool {
    let current_scroll_x = with_browser_host_state(browser_instance_id, |state| {
        state
            .pending_hosted_scroll
            .map(|pending| pending.scroll_x)
            .unwrap_or(state.hosted_surface_state.scroll_x)
    });
    set_hosted_scroll_for_browser(browser_instance_id, current_scroll_x, scroll_y)
}

pub fn set_hosted_scroll_for_browser(
    browser_instance_id: u32,
    scroll_x: u32,
    scroll_y: u32,
) -> bool {
    let _ = (browser_instance_id, scroll_x, scroll_y);
    false
}

pub fn hosted_surface_state() -> HostedBrowserSurfaceState {
    hosted_surface_state_for_browser(PRIMARY_BROWSER_INSTANCE_ID)
}

pub fn hosted_surface_state_for_browser(browser_instance_id: u32) -> HostedBrowserSurfaceState {
    with_browser_host_state(browser_instance_id, |state| {
        state.hosted_surface_state.clone()
    })
}

pub fn hosted_surface_seq() -> u32 {
    hosted_surface_seq_for_browser(PRIMARY_BROWSER_INSTANCE_ID)
}

pub fn hosted_surface_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_host_state(browser_instance_id, |state| state.hosted_surface_seq)
}

pub fn hosted_interactive_state() -> HostedBrowserInteractiveState {
    hosted_interactive_state_for_browser(PRIMARY_BROWSER_INSTANCE_ID)
}

pub fn hosted_interactive_state_for_browser(
    browser_instance_id: u32,
) -> HostedBrowserInteractiveState {
    with_browser_host_state(browser_instance_id, |state| {
        state.hosted_interactive_state.clone()
    })
}

pub fn hosted_interactive_seq() -> u32 {
    hosted_interactive_seq_for_browser(PRIMARY_BROWSER_INSTANCE_ID)
}

pub fn hosted_interactive_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_host_state(browser_instance_id, |state| state.hosted_interactive_seq)
}

pub fn hosted_text_state_for_browser(browser_instance_id: u32) -> HostedBrowserTextState {
    with_browser_host_state(browser_instance_id, |state| state.hosted_text_state.clone())
}

pub fn hosted_text_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_host_state(browser_instance_id, |state| state.hosted_text_seq)
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
    let browser =
        qjs::JS_GetPropertyStr(ctx, global, b"__trueosBrowser\0".as_ptr() as *const c_char);
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

unsafe fn apply_pending_browser_rpc(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    if with_browser_host_state(browser_instance_id, |state| {
        state.active_browser_rpc_id.is_some()
    }) {
        return;
    }

    let Some(next) = with_browser_host_state_mut(browser_instance_id, |state| {
        state.pending_browser_rpc.pop_front()
    }) else {
        return;
    };

    if next.browser_instance_id != browser_instance_id {
        BROWSER_RPC_RESULTS.lock().push_back(BrowserRpcResult {
            id: next.id,
            payload_json: alloc::format!(
                "{{\"ok\":false,\"error\":\"browser rpc target unavailable: requested instance {}, active {}\"}}",
                next.browser_instance_id,
                browser_instance_id
            ),
        });
        return;
    }
    let active_window_id = browser_window_id_for_instance(browser_instance_id);
    if active_window_id == 0 {
        BROWSER_RPC_RESULTS.lock().push_back(BrowserRpcResult {
            id: next.id,
            payload_json: String::from(
                "{\"ok\":false,\"error\":\"browser rpc unavailable: no active browser\"}",
            ),
        });
        return;
    }
    if next.browser_window_id != 0 && next.browser_window_id != active_window_id {
        BROWSER_RPC_RESULTS.lock().push_back(BrowserRpcResult {
            id: next.id,
            payload_json: alloc::format!(
                "{{\"ok\":false,\"error\":\"browser rpc target unavailable: requested {}, active {}\"}}",
                next.browser_window_id,
                active_window_id
            ),
        });
        return;
    }

    let method_lit = helpers::js_single_quoted_literal(next.method.as_str());
    let args_lit = helpers::js_single_quoted_literal(next.args_json.as_str());
    let mut src = String::new();
    src.push_str("(function(){const __g=(typeof globalThis!=='undefined')?globalThis:this;");
    src.push_str("__g.__trueosBrowserRpcDoneId=0;__g.__trueosBrowserRpcDonePayload='';");
    src.push_str("const __id=");
    src.push_str(alloc::format!("{}", next.id).as_str());
    src.push_str(";const __method=");
    src.push_str(method_lit.as_str());
    src.push_str(";const __argsJson=");
    src.push_str(args_lit.as_str());
    src.push_str(";Promise.resolve().then(()=>{const __browser=__g.__trueosBrowser;");
    src.push_str("if(!__browser||typeof __browser[__method]!=='function'){throw new Error('browser rpc unavailable: '+__method);} ");
    src.push_str("const __args=JSON.parse(__argsJson);return __browser[__method](...__args);})");
    src.push_str(".then((__result)=>{let __payload='';try{__payload=JSON.stringify({ok:true,result:(__result===undefined?null:__result)});}catch(__err){__payload=JSON.stringify({ok:false,error:String(__err&&__err.message?__err.message:__err)});}__g.__trueosBrowserRpcDonePayload=__payload;__g.__trueosBrowserRpcDoneId=__id;})");
    src.push_str(".catch((__err)=>{__g.__trueosBrowserRpcDonePayload=JSON.stringify({ok:false,error:String(__err&&__err.stack?__err.stack:__err)});__g.__trueosBrowserRpcDoneId=__id;});})();");

    if !helpers::eval_or_log(
        ctx,
        src.as_bytes(),
        b"<browser-rpc>\0".as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
        "browser rpc",
    ) {
        BROWSER_RPC_RESULTS.lock().push_back(BrowserRpcResult {
            id: next.id,
            payload_json: String::from("{\"ok\":false,\"error\":\"browser rpc eval failed\"}"),
        });
        return;
    }

    with_browser_host_state_mut(browser_instance_id, |state| {
        state.active_browser_rpc_id = Some(next.id);
    });
}

unsafe fn collect_browser_rpc_result(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    let Some(active_id) =
        with_browser_host_state(browser_instance_id, |state| state.active_browser_rpc_id)
    else {
        return;
    };

    let global = qjs::JS_GetGlobalObject(ctx);
    if global.is_exception() {
        qjs::js_free_value(ctx, global);
        return;
    }

    let done_id = js_prop_f64(ctx, global, b"__trueosBrowserRpcDoneId\0")
        .map(|value| {
            if value.is_finite() && value >= 0.0 {
                value as u32
            } else {
                0
            }
        })
        .unwrap_or(0);
    if done_id != active_id {
        qjs::js_free_value(ctx, global);
        return;
    }

    let payload =
        js_prop_string(ctx, global, b"__trueosBrowserRpcDonePayload\0").unwrap_or_else(|| {
            String::from("{\"ok\":false,\"error\":\"browser rpc missing payload\"}")
        });
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosBrowserRpcDoneId\0".as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, 0.0),
    );
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        b"__trueosBrowserRpcDonePayload\0".as_ptr() as *const c_char,
        qjs::JS_NewStringLen(ctx, b"".as_ptr() as *const c_char, 0),
    );
    qjs::js_free_value(ctx, global);

    with_browser_host_state_mut(browser_instance_id, |state| {
        state.active_browser_rpc_id = None;
    });
    BROWSER_RPC_RESULTS.lock().push_back(BrowserRpcResult {
        id: active_id,
        payload_json: payload,
    });
}

unsafe fn apply_pending_hosted_viewport(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    let Some(next) = with_browser_host_state_mut(browser_instance_id, |state| {
        state.pending_hosted_viewport.take()
    }) else {
        return;
    };
    if next.browser_instance_id != browser_instance_id {
        return;
    }
    let Some(browser) = browser_api_value(ctx) else {
        with_browser_host_state_mut(browser_instance_id, |state| {
            state.pending_hosted_viewport = Some(next);
        });
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
        with_browser_host_state_mut(browser_instance_id, |state| {
            state.pending_hosted_viewport = Some(next);
        });
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
        with_browser_host_state_mut(browser_instance_id, |state| {
            state.pending_hosted_viewport = Some(next);
        });
    } else {
        with_browser_host_state_mut(browser_instance_id, |state| {
            state.applied_hosted_viewport = Some(next);
        });
    }
    qjs::js_free_value(ctx, result);
    qjs::js_free_value(ctx, content_rect);
    qjs::js_free_value(ctx, viewport);
    qjs::js_free_value(ctx, func);
    qjs::js_free_value(ctx, browser);
}

unsafe fn apply_pending_hosted_scroll(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    let _ = ctx;
    with_browser_host_state_mut(browser_instance_id, |state| {
        state.pending_hosted_scroll = None;
    });
}

unsafe fn sync_hosted_surface_state(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    let Some(browser) = browser_api_value(ctx) else {
        return;
    };
    let func = qjs::JS_GetPropertyStr(ctx, browser, b"getSurfaceState\0".as_ptr() as *const c_char);
    if func.is_exception() || func.tag == qjs::JS_TAG_UNDEFINED || func.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, func);
        qjs::js_free_value(ctx, browser);
        return;
    }
    let result = qjs::JS_Call(ctx, func, browser, 0, core::ptr::null());
    qjs::js_free_value(ctx, func);
    qjs::js_free_value(ctx, browser);
    if result.is_exception()
        || result.tag == qjs::JS_TAG_UNDEFINED
        || result.tag == qjs::JS_TAG_NULL
    {
        if result.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "browser getSurfaceState");
        }
        qjs::js_free_value(ctx, result);
        return;
    }

    let mut next = HostedBrowserSurfaceState {
        seq: 0,
        cache_revision: js_prop_f64(ctx, result, b"cacheRevision\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        cache_width: js_prop_f64(ctx, result, b"cacheWidth\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        tile_height: js_prop_f64(ctx, result, b"tileHeight\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        viewport_width: js_prop_f64(ctx, result, b"viewportWidth\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        viewport_height: js_prop_f64(ctx, result, b"viewportHeight\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        content_width: js_prop_f64(ctx, result, b"contentWidth\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        content_height: js_prop_f64(ctx, result, b"contentHeight\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        content_top_y: js_prop_f64(ctx, result, b"contentTopY\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        scroll_x: js_prop_f64(ctx, result, b"scrollX\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        scroll_y: js_prop_f64(ctx, result, b"scrollY\0")
            .unwrap_or(0.0)
            .max(0.0) as u32,
        regions: Vec::new(),
    };

    let regions = qjs::JS_GetPropertyStr(ctx, result, b"regions\0".as_ptr() as *const c_char);
    if !regions.is_exception()
        && regions.tag != qjs::JS_TAG_UNDEFINED
        && regions.tag != qjs::JS_TAG_NULL
    {
        let len = js_prop_f64(ctx, regions, b"length\0")
            .unwrap_or(0.0)
            .max(0.0) as usize;
        for idx in 0..len {
            let item = qjs::JS_GetPropertyUint32(ctx, regions, idx as u32);
            if item.is_exception()
                || item.tag == qjs::JS_TAG_UNDEFINED
                || item.tag == qjs::JS_TAG_NULL
            {
                qjs::js_free_value(ctx, item);
                continue;
            }
            next.regions.push(HostedBrowserRegion {
                tex_id: js_prop_f64(ctx, item, b"texId\0").unwrap_or(0.0).max(0.0) as u32,
                doc_y: js_prop_f64(ctx, item, b"docY\0").unwrap_or(0.0).max(0.0) as u32,
                width: js_prop_f64(ctx, item, b"width\0").unwrap_or(0.0).max(0.0) as u32,
                height: js_prop_f64(ctx, item, b"height\0").unwrap_or(0.0).max(0.0) as u32,
                revision: js_prop_f64(ctx, item, b"revision\0")
                    .unwrap_or(0.0)
                    .max(0.0) as u32,
                dirty: js_prop_f64(ctx, item, b"dirty\0").unwrap_or(0.0) != 0.0,
            });
            qjs::js_free_value(ctx, item);
        }
    }
    qjs::js_free_value(ctx, regions);
    qjs::js_free_value(ctx, result);

    with_browser_host_state_mut(browser_instance_id, |state| {
        let prev = state.hosted_surface_state.clone();
        next.seq = prev.seq;
        if prev != next {
            let mut seq = state.hosted_surface_seq.wrapping_add(1);
            if seq == 0 {
                seq = 1;
            }
            next.seq = seq;
            state.hosted_surface_seq = seq;
            state.hosted_surface_state = next;
        }
    });
}

unsafe fn sync_hosted_interactive_state(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
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
    if result.is_exception()
        || result.tag == qjs::JS_TAG_UNDEFINED
        || result.tag == qjs::JS_TAG_NULL
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

    with_browser_host_state_mut(browser_instance_id, |state| {
        let prev = state.hosted_interactive_state.clone();
        next.seq = prev.seq;
        if prev != next {
            let mut seq = state.hosted_interactive_seq.wrapping_add(1);
            if seq == 0 {
                seq = 1;
            }
            next.seq = seq;
            state.hosted_interactive_seq = seq;
            state.hosted_interactive_state = next;
        }
    });
}

unsafe fn sync_hosted_text_state(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    let Some(browser) = browser_api_value(ctx) else {
        return;
    };
    let func = qjs::JS_GetPropertyStr(ctx, browser, b"getTextRows\0".as_ptr() as *const c_char);
    if func.is_exception() || func.tag == qjs::JS_TAG_UNDEFINED || func.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, func);
        qjs::js_free_value(ctx, browser);
        return;
    }
    let result = qjs::JS_Call(ctx, func, browser, 0, core::ptr::null());
    qjs::js_free_value(ctx, func);
    qjs::js_free_value(ctx, browser);
    if result.is_exception()
        || result.tag == qjs::JS_TAG_UNDEFINED
        || result.tag == qjs::JS_TAG_NULL
    {
        if result.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "browser getTextRows");
        }
        qjs::js_free_value(ctx, result);
        return;
    }

    let mut next = HostedBrowserTextState {
        seq: 0,
        rows: Vec::new(),
    };
    let len = js_prop_f64(ctx, result, b"length\0").unwrap_or(0.0).max(0.0) as usize;
    let max_rows = core::cmp::min(len, 256);
    for idx in 0..max_rows {
        let item = qjs::JS_GetPropertyUint32(ctx, result, idx as u32);
        if item.is_exception()
            || item.tag == qjs::JS_TAG_UNDEFINED
            || item.tag == qjs::JS_TAG_NULL
        {
            qjs::js_free_value(ctx, item);
            continue;
        }
        let text = js_prop_string(ctx, item, b"text\0").unwrap_or_default();
        let kind = js_prop_string(ctx, item, b"kind\0").unwrap_or_else(|| String::from("text"));
        let depth = js_prop_f64(ctx, item, b"depth\0").unwrap_or(0.0).max(0.0) as u32;
        next.rows.push(HostedBrowserTextRow { depth, kind, text });
        qjs::js_free_value(ctx, item);
    }
    qjs::js_free_value(ctx, result);

    with_browser_host_state_mut(browser_instance_id, |state| {
        let prev = state.hosted_text_state.clone();
        next.seq = prev.seq;
        if prev != next {
            let mut seq = state.hosted_text_seq.wrapping_add(1);
            if seq == 0 {
                seq = 1;
            }
            next.seq = seq;
            state.hosted_text_seq = seq;
            state.hosted_text_state = next;
        }
    });
}

unsafe fn apply_pending_html(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    let Some(next) =
        with_browser_host_state_mut(browser_instance_id, |state| state.pending_html.take())
    else {
        return;
    };
    if next.browser_instance_id != browser_instance_id {
        return;
    }

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

unsafe fn apply_pending_qjs_input(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    let Some(next) = with_browser_host_state_mut(browser_instance_id, |state| {
        state.pending_qjs_input.pop_front()
    }) else {
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

unsafe fn install_globals(ctx: *mut qjs::JSContext, browser_instance_id: u32) -> bool {
    let init_filename = b"<browser-globals>\0";
    let mut init_src = String::new();
    let text_widths = browser_text_widths_by_char();
    init_src.push_str("\nconst G = (typeof globalThis !== 'undefined') ? globalThis : this;\n");
    qjs::html::append_embedded_browser_globals_js(&mut init_src);
    init_src.push_str("G.__trueosBrowserTextWidthByChar = ");
    append_js_u8_array(&mut init_src, &text_widths);
    init_src.push_str(";\n");
    init_src.push_str("G.__trueosBrowserDefaultFontPx = 16;\n");
    init_src.push_str("G.__trueosBrowserInstanceId = ");
    init_src.push_str(alloc::format!("{}", browser_instance_id).as_str());
    init_src.push_str(";\n");
    init_src.push_str(
        r#"
if (!G.window) G.window = G;
if (typeof G.window.innerWidth !== 'number') G.window.innerWidth = "#,
    );
    init_src.push_str(alloc::format!("{}", STATIC_BROWSER_VIEWPORT_W).as_str());
    init_src.push_str(
        r#";
if (typeof G.window.innerHeight !== 'number') G.window.innerHeight = "#,
    );
    init_src.push_str(alloc::format!("{}", STATIC_BROWSER_VIEWPORT_H).as_str());
    init_src.push_str(
        r#";
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
G.__trueosBrowserRpcDoneId = 0;
G.__trueosBrowserRpcDonePayload = '';
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

#[embassy_executor::task(pool_size = 5)]
pub async fn boot_browser(browser_instance_id: u32) {
    let browser_instance_id = normalize_browser_instance_id(browser_instance_id);
    let already_running = with_browser_host_state_mut(browser_instance_id, |state| {
        let was_running = state.started;
        if !was_running {
            state.started = true;
        }
        was_running
    });
    if already_running {
        qjs::trueos_shims::log_info(
            alloc::format!("qjs-browser[{}]: already running\n", browser_instance_id).as_str(),
        );
        return;
    }
    qjs::trueos_shims::log_info(
        alloc::format!(
            "qjs-browser[{}]: starting browser bootstrap\n",
            browser_instance_id
        )
        .as_str(),
    );
    unsafe {
        let vm = match qjs::vm::QjsVm::new_node() {
            Some(vm) => vm,
            None => {
                qjs::trueos_shims::log_info(
                    alloc::format!(
                        "qjs-browser[{}]: JS runtime init failed\n",
                        browser_instance_id
                    )
                    .as_str(),
                );
                with_browser_host_state_mut(browser_instance_id, |state| {
                    state.started = false;
                });
                return;
            }
        };
        let ctx = vm.ctx_ptr();
        let rt = vm.rt_ptr();

        qjs::node::install_globals(ctx);
        qjs::layout::install_layout_api(ctx);

        if !install_globals(ctx, browser_instance_id) {
            with_browser_host_state_mut(browser_instance_id, |state| {
                state.started = false;
            });
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
            with_browser_host_state_mut(browser_instance_id, |state| {
                state.started = false;
            });
            return;
        }

        loop {
            apply_pending_html(ctx, browser_instance_id);
            apply_pending_qjs_input(ctx, browser_instance_id);
            apply_pending_browser_rpc(ctx, browser_instance_id);
            apply_pending_hosted_viewport(ctx, browser_instance_id);
            apply_pending_hosted_scroll(ctx, browser_instance_id);
            if !qjs::vm::pump_runtime_once(
                rt,
                ctx,
                alloc::format!("browser-{}", browser_instance_id).as_str(),
            ) {
                break;
            }
            collect_browser_rpc_result(ctx, browser_instance_id);
            sync_hosted_surface_state(ctx, browser_instance_id);
            sync_hosted_interactive_state(ctx, browser_instance_id);
            sync_hosted_text_state(ctx, browser_instance_id);
            Timer::after(EmbassyDuration::from_millis(16)).await;
        }
        qjs::trueos_shims::log_info(
            alloc::format!("qjs-browser[{}]: stopped\n", browser_instance_id).as_str(),
        );
        with_browser_host_state_mut(browser_instance_id, |state| {
            state.started = false;
            state.active_browser_rpc_id = None;
        });
    }
}
