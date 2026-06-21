#![cfg(feature = "trueos")]
#![allow(dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::signal::Signal;
use embassy_sync::zerocopy_channel::{Channel, Receiver, Sender};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::{Mutex, Once};

use crate as qjs;

pub const MAX_BROWSER_INSTANCE_ID: u32 = 100;
pub const TRUESURFER_TASK_POOL_SIZE: usize = 100;
pub const BOOT_BROWSER_INSTANCE_IDS: [u32; 1] = [1];

pub const HOSTED_KEYBOARD_MOD_SHIFT: u8 = 1 << 0;
pub const HOSTED_KEYBOARD_MOD_CTRL: u8 = 1 << 1;
pub const HOSTED_KEYBOARD_MOD_ALT: u8 = 1 << 2;
pub const HOSTED_KEYBOARD_MOD_META: u8 = 1 << 3;

const TRUESURFER_IMPORT_FILENAME: &[u8] = b"<truesurfer-init>\0";
const TRUESURFER_IMPORT_SOURCE: &[u8] = br#"
globalThis.__trueosTruesurferReady = 0;
globalThis.__trueosTruesurferWarmup = {
  status: 'loading-entry',
  baseUrl: '/qjs/truesurfer/truesurfer.mjs',
};
if (typeof globalThis.importModule !== 'function') {
  globalThis.__trueosTruesurferReady = -1;
  globalThis.__trueosTruesurferWarmup = {
    status: 'error',
    baseUrl: '/qjs/truesurfer/truesurfer.mjs',
    error: 'importModule is not available',
  };
  throw new Error('importModule is not available');
}
globalThis.__trueosTruesurferEntryPromise = Promise.resolve(
  globalThis.importModule('/qjs/truesurfer/truesurfer.mjs'),
).catch((error) => {
  const message = error && error.stack ? String(error.stack) : String(error || 'unknown truesurfer import error');
  globalThis.__trueosTruesurferReady = -1;
  globalThis.__trueosTruesurferWarmup = {
    status: 'error',
    baseUrl: '/qjs/truesurfer/truesurfer.mjs',
    error: message,
  };
  throw error;
});
"#;
const TRUESURFER_READY_PROP: &[u8] = b"__trueosTruesurferReady\0";
const TRUESURFER_ID_PROP: &[u8] = b"__trueosTruesurferBrowserId\0";
const TRUESURFER_OBJ_PROP: &[u8] = b"__trueosTruesurfer\0";
const TRUESURFER_SET_HTML_PROP: &[u8] = b"setHtml\0";
const TRUESURFER_META_URL_PROP: &[u8] = b"url\0";
const TRUESURFER_RESULT_OK_PROP: &[u8] = b"ok\0";
const TRUESURFER_RESULT_BYTES_PROP: &[u8] = b"bytes\0";
const TRUESURFER_RESULT_LINES_PROP: &[u8] = b"lines\0";
const TRUESURFER_RESULT_PARSE_MS_PROP: &[u8] = b"parseMs\0";
const TRUESURFER_RESULT_TITLE_PROP: &[u8] = b"title\0";
const TRUESURFER_RESULT_FAVICON_URL_PROP: &[u8] = b"faviconUrl\0";
const TRUESURFER_RESULT_SHELL_BYTES_PROP: &[u8] = b"shellBytes\0";
const TRUESURFER_RESULT_BODY_BYTES_PROP: &[u8] = b"bodyBytes\0";
const TRUESURFER_RESULT_STYLE_COUNT_PROP: &[u8] = b"styleCount\0";
const TRUESURFER_RESULT_STYLE_BYTES_PROP: &[u8] = b"styleBytes\0";
const TRUESURFER_RESULT_SCRIPT_COUNT_PROP: &[u8] = b"scriptCount\0";
const TRUESURFER_RESULT_SCRIPT_BYTES_PROP: &[u8] = b"scriptBytes\0";
const TRUESURFER_RESULT_ERROR_PROP: &[u8] = b"error\0";
const TRUESURFER_RESULT_RENDER_HASH_PROP: &[u8] = b"renderHash\0";
const TRUESURFER_RESULT_LAYOUT_HASH_PROP: &[u8] = b"layoutHash\0";
const TRUESURFER_RESULT_RENDER_TREE_JSON_PROP: &[u8] = b"renderTreeJson\0";
const TRUESURFER_RESULT_LAYOUT_TRACE_JSON_PROP: &[u8] = b"layoutTraceJson\0";
const TRUESURFER_HTML_QUEUE_DEPTH: usize = 2;
const TRUESURFER_HTML_QUEUE_WAIT_MS: u64 = 2;
const TRUESURFER_BUSY_PUMP_BUDGET: usize = 512;
const TRUESURFER_BUSY_SLEEP_MS: u64 = 1;
const TRUESURFER_RESULT_TEXT_MAX_BYTES: usize = 4 * 1024;
const TRUESURFER_RESULT_HASH_MAX_BYTES: usize = 128;
const TRUESURFER_RESULT_JSON_MAX_BYTES: usize = 1024 * 1024;
const HOSTED_BROWSER_DIRTY_CONTENT: u32 = 1 << 0;
const HOSTED_BROWSER_DIRTY_INTERACTIVE: u32 = 1 << 1;

struct SpinRawMutex(Mutex<()>);

unsafe impl RawMutex for SpinRawMutex {
    const INIT: Self = Self(Mutex::new(()));

    fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.lock();
        f()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostedBrowserSurfaceState {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub content_width: u32,
    pub content_height: u32,
    pub scroll_x: u32,
    pub scroll_y: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostedBrowserInteractiveItem {
    pub item_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostedBrowserInteractiveState {
    pub interactives: alloc::vec::Vec<HostedBrowserInteractiveItem>,
}

#[derive(Clone, Debug)]
pub enum HostedKeyboardEvent {
    Text { text: String },
    Key { key: String, modifiers: u8 },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParseResult {
    pub ok: bool,
    pub url: String,
    pub bytes: u32,
    pub lines: u32,
    pub parse_ms: u32,
    pub title: String,
    pub favicon_url: String,
    pub shell_bytes: u32,
    pub body_bytes: u32,
    pub style_count: u32,
    pub style_bytes: u32,
    pub script_count: u32,
    pub script_bytes: u32,
    pub error: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Ui3RenderTreeFrame {
    pub browser_instance_id: u32,
    pub seq: u32,
    pub url: String,
    pub render_hash: String,
    pub layout_hash: String,
    pub render_tree_json: String,
    pub layout_trace_json: String,
}

#[derive(Clone, Debug)]
struct PendingHtml {
    html: String,
    url: String,
}

#[derive(Clone, Default)]
struct HtmlHandoffSlot {
    html: String,
    url: String,
}

struct BrowserHtmlQueue {
    sender: Mutex<Sender<'static, SpinRawMutex, HtmlHandoffSlot>>,
    receiver: Mutex<Receiver<'static, SpinRawMutex, HtmlHandoffSlot>>,
}

#[derive(Default)]
struct BrowserInstanceState {
    started: bool,
    api_ready: bool,
    last_parse_result: Option<ParseResult>,
    window_id: u32,
    render_tex_id: u32,
    surface_seq: u32,
    interactive_seq: u32,
    ui3_render_tree_seq: u32,
    pending_ui3_render_tree_frame: Option<Ui3RenderTreeFrame>,
    surface_state: HostedBrowserSurfaceState,
}

static TRUESURFER_STATE: Mutex<BTreeMap<u32, BrowserInstanceState>> = Mutex::new(BTreeMap::new());
static BROWSER_RPC_SEQ: AtomicU32 = AtomicU32::new(1);
static TRUESURFER_HTML_QUEUES: Once<Vec<BrowserHtmlQueue>> = Once::new();
static TRUESURFER_HTML_READY: [Signal<SpinRawMutex, ()>; MAX_BROWSER_INSTANCE_ID as usize] =
    [const { Signal::new() }; MAX_BROWSER_INSTANCE_ID as usize];

fn html_handoff_queues() -> &'static Vec<BrowserHtmlQueue> {
    TRUESURFER_HTML_QUEUES.call_once(|| {
        let mut queues = Vec::with_capacity(MAX_BROWSER_INSTANCE_ID as usize);
        for _ in 0..MAX_BROWSER_INSTANCE_ID {
            let slots: &'static mut [HtmlHandoffSlot] = Box::leak(
                vec![HtmlHandoffSlot::default(); TRUESURFER_HTML_QUEUE_DEPTH].into_boxed_slice(),
            );
            let channel: &'static mut Channel<'static, SpinRawMutex, HtmlHandoffSlot> =
                Box::leak(Box::new(Channel::new(slots)));
            let (sender, receiver) = channel.split();
            queues.push(BrowserHtmlQueue {
                sender: Mutex::new(sender),
                receiver: Mutex::new(receiver),
            });
        }
        queues
    })
}

fn html_handoff_queue(browser_instance_id: u32) -> Option<&'static BrowserHtmlQueue> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    html_handoff_queues().get(browser_instance_id.saturating_sub(1) as usize)
}

fn html_ready_signal(browser_instance_id: u32) -> Option<&'static Signal<SpinRawMutex, ()>> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    TRUESURFER_HTML_READY.get(browser_instance_id.saturating_sub(1) as usize)
}

#[inline]
fn browser_valid(browser_instance_id: u32) -> bool {
    (1..=MAX_BROWSER_INSTANCE_ID).contains(&browser_instance_id)
}

#[inline]
fn default_render_tex_id(browser_instance_id: u32) -> u32 {
    9_000u32.saturating_add(browser_instance_id.saturating_sub(1))
}

fn with_browser_state_mut<R>(
    browser_instance_id: u32,
    f: impl FnOnce(&mut BrowserInstanceState) -> R,
) -> Option<R> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    let mut guard = TRUESURFER_STATE.lock();
    let state = guard
        .entry(browser_instance_id)
        .or_insert_with(|| BrowserInstanceState {
            render_tex_id: default_render_tex_id(browser_instance_id),
            surface_state: HostedBrowserSurfaceState {
                viewport_width: 512,
                viewport_height: 512,
                content_width: 512,
                content_height: 1,
                scroll_x: 0,
                scroll_y: 0,
            },
            ..BrowserInstanceState::default()
        });
    Some(f(state))
}

fn with_browser_state<R>(
    browser_instance_id: u32,
    f: impl FnOnce(&BrowserInstanceState) -> R,
) -> Option<R> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    let guard = TRUESURFER_STATE.lock();
    guard.get(&browser_instance_id).map(f)
}

#[inline]
fn signal_hosted_browser_dirty(browser_instance_id: u32, flags: u32) {
    if browser_valid(browser_instance_id) && flags != 0 {
        qjs::platform::ui::signal_hosted_browser_dirty(browser_instance_id, flags);
    }
}

#[inline]
fn log_line(line: String) {
    qjs::trueos_shims::log_info(line.as_str());
}

#[inline]
fn log_error(line: String) {
    qjs::trueos_shims::log_error(line.as_str());
}

pub fn default_browser_started() -> bool {
    with_browser_state(1, |state| state.started).unwrap_or(false)
}

pub fn latest_parse_result_for_browser(browser_instance_id: u32) -> Option<ParseResult> {
    with_browser_state(browser_instance_id, |state| state.last_parse_result.clone()).flatten()
}

pub fn take_ui3_render_tree_frame_for_browser(
    browser_instance_id: u32,
) -> Option<Ui3RenderTreeFrame> {
    with_browser_state_mut(browser_instance_id, |state| state.pending_ui3_render_tree_frame.take())
        .flatten()
}

pub async fn queue_set_html_with_url_for_browser(
    browser_instance_id: u32,
    html: String,
    url: Option<String>,
) -> bool {
    let Some(queue) = html_handoff_queue(browser_instance_id) else {
        return false;
    };
    let Some(ready_signal) = html_ready_signal(browser_instance_id) else {
        return false;
    };

    let html_len = html.len();
    let mut next_html = Some(html);
    let mut next_url = Some(url.unwrap_or_default());

    loop {
        {
            let mut sender = queue.sender.lock();
            if let Some(slot) = sender.try_send() {
                slot.html = next_html.take().unwrap_or_default();
                slot.url = next_url.take().unwrap_or_default();
                sender.send_done();
                ready_signal.signal(());
                log_line(format!(
                    "qjs-truesurfer[{}]: queued html bytes={} depth={} signal=1\n",
                    browser_instance_id,
                    html_len,
                    sender.len()
                ));
                return true;
            }
        }

        Timer::after(EmbassyDuration::from_millis(TRUESURFER_HTML_QUEUE_WAIT_MS)).await;
    }
}

pub fn queue_browser_rpc(_method: String, _args_json: String, _browser_window_id: u32) -> u32 {
    BROWSER_RPC_SEQ.fetch_add(1, Ordering::Relaxed)
}

pub fn take_browser_rpc_result(_id: u32) -> Option<String> {
    None
}

pub fn hosted_surface_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.surface_seq).unwrap_or(0)
}

pub fn hosted_interactive_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.interactive_seq).unwrap_or(0)
}

pub fn hosted_surface_state_for_browser(browser_instance_id: u32) -> HostedBrowserSurfaceState {
    with_browser_state(browser_instance_id, |state| state.surface_state).unwrap_or_default()
}

pub fn hosted_interactive_state_for_browser(
    _browser_instance_id: u32,
) -> HostedBrowserInteractiveState {
    HostedBrowserInteractiveState::default()
}

pub fn set_hosted_viewport_for_browser(
    browser_instance_id: u32,
    viewport_width: u32,
    viewport_height: u32,
    _content_x: i32,
    _content_y: i32,
    content_width: u32,
    content_height: u32,
) -> bool {
    let mut dirty = false;
    let ok = with_browser_state_mut(browser_instance_id, |state| {
        let next = HostedBrowserSurfaceState {
            viewport_width: viewport_width.max(1),
            viewport_height: viewport_height.max(1),
            content_width: content_width.max(viewport_width.max(1)),
            content_height: content_height.max(1),
            scroll_x: state.surface_state.scroll_x,
            scroll_y: state.surface_state.scroll_y,
        };
        if state.surface_state == next {
            return true;
        }
        state.surface_state = next;
        state.surface_seq = state.surface_seq.wrapping_add(1);
        dirty = true;
        true
    })
    .unwrap_or(false);
    if dirty {
        signal_hosted_browser_dirty(browser_instance_id, HOSTED_BROWSER_DIRTY_CONTENT);
    }
    ok
}

pub fn set_hosted_scroll_for_browser(
    browser_instance_id: u32,
    scroll_x: u32,
    scroll_y: u32,
) -> bool {
    let mut dirty = false;
    let ok = with_browser_state_mut(browser_instance_id, |state| {
        if state.surface_state.scroll_x == scroll_x && state.surface_state.scroll_y == scroll_y {
            return true;
        }
        state.surface_state.scroll_x = scroll_x;
        state.surface_state.scroll_y = scroll_y;
        state.surface_seq = state.surface_seq.wrapping_add(1);
        dirty = true;
        true
    })
    .unwrap_or(false);
    if dirty {
        signal_hosted_browser_dirty(browser_instance_id, HOSTED_BROWSER_DIRTY_CONTENT);
    }
    ok
}

pub fn bind_browser_window_to_instance(browser_instance_id: u32, window_id: u32) -> bool {
    with_browser_state_mut(browser_instance_id, |state| {
        state.window_id = window_id;
        true
    })
    .unwrap_or(false)
}

pub fn browser_window_id_for_instance(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.window_id).unwrap_or(0)
}

pub fn set_browser_render_target_tex_id_for_browser(browser_instance_id: u32, tex_id: u32) -> bool {
    with_browser_state_mut(browser_instance_id, |state| {
        state.render_tex_id = tex_id;
        true
    })
    .unwrap_or(false)
}

pub fn render_tex_id_for_browser_instance(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.render_tex_id)
        .unwrap_or_else(|| default_render_tex_id(browser_instance_id))
}

fn publish_ui3_render_tree_frame_for_browser(
    browser_instance_id: u32,
    url: String,
    render_hash: String,
    layout_hash: String,
    render_tree_json: String,
    layout_trace_json: String,
) -> Option<u32> {
    if render_tree_json.is_empty() && layout_trace_json.is_empty() {
        return None;
    }
    let seq = with_browser_state_mut(browser_instance_id, |state| {
        state.ui3_render_tree_seq = state.ui3_render_tree_seq.wrapping_add(1).max(1);
        let seq = state.ui3_render_tree_seq;
        state.pending_ui3_render_tree_frame = Some(Ui3RenderTreeFrame {
            browser_instance_id,
            seq,
            url,
            render_hash,
            layout_hash,
            render_tree_json,
            layout_trace_json,
        });
        seq
    })?;
    signal_hosted_browser_dirty(browser_instance_id, HOSTED_BROWSER_DIRTY_CONTENT);
    Some(seq)
}

pub fn queue_hosted_keyboard_events(
    browser_window_id: u32,
    events: &[HostedKeyboardEvent],
) -> bool {
    if events.is_empty() {
        return true;
    }
    let Some(browser_instance_id) = (1..=MAX_BROWSER_INSTANCE_ID)
        .find(|candidate| browser_window_id_for_instance(*candidate) == browser_window_id)
    else {
        return false;
    };
    let queued = with_browser_state_mut(browser_instance_id, |state| {
        state.interactive_seq = state.interactive_seq.wrapping_add(events.len() as u32);
        true
    })
    .unwrap_or(false);
    if queued {
        signal_hosted_browser_dirty(browser_instance_id, HOSTED_BROWSER_DIRTY_INTERACTIVE);
    }
    queued
}

unsafe fn set_global_i32(ctx: *mut qjs::JSContext, key: &[u8], value: i32) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        key.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, value as f64),
    );
    qjs::js_free_value(ctx, global);
}

unsafe fn set_global_string(ctx: *mut qjs::JSContext, key: &[u8], value: &str) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let value_js = qjs::JS_NewStringLen(ctx, value.as_ptr() as *const c_char, value.len());
    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, value_js);
    qjs::js_free_value(ctx, global);
}

unsafe fn read_global_bool(ctx: *mut qjs::JSContext, key: &[u8]) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let value = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite();
    qjs::js_free_value(ctx, value);
    qjs::js_free_value(ctx, global);
    ok && out != 0.0
}

unsafe fn read_global_string(ctx: *mut qjs::JSContext, key: &[u8]) -> String {
    let global = qjs::JS_GetGlobalObject(ctx);
    let value = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    if value.is_exception() || value.tag == qjs::JS_TAG_UNDEFINED || value.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, value);
        qjs::js_free_value(ctx, global);
        return String::new();
    }
    let out = js_value_to_string(
        ctx,
        value,
        "global",
        TRUESURFER_RESULT_TEXT_MAX_BYTES,
    );
    qjs::js_free_value(ctx, value);
    qjs::js_free_value(ctx, global);
    strip_trueos_host_markers(out.as_str())
}

unsafe fn truesurfer_ready(ctx: *mut qjs::JSContext) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let ready =
        qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_READY_PROP.as_ptr() as *const c_char);
    let mut ready_f = 0.0f64;
    let ready_flag = qjs::JS_ToFloat64(ctx, &mut ready_f as *mut f64, ready) == 0
        && ready_f.is_finite()
        && ready_f >= 1.0;

    let surfer = qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_OBJ_PROP.as_ptr() as *const c_char);
    let set_html = if surfer.is_exception()
        || surfer.tag == qjs::JS_TAG_UNDEFINED
        || surfer.tag == qjs::JS_TAG_NULL
    {
        qjs::JSValue {
            u: qjs::JSValueUnion { int32: 0 },
            tag: qjs::JS_TAG_UNDEFINED,
        }
    } else {
        qjs::JS_GetPropertyStr(ctx, surfer, TRUESURFER_SET_HTML_PROP.as_ptr() as *const c_char)
    };
    let has_set_html = !set_html.is_exception()
        && set_html.tag != qjs::JS_TAG_UNDEFINED
        && set_html.tag != qjs::JS_TAG_NULL;

    qjs::js_free_value(ctx, set_html);
    qjs::js_free_value(ctx, surfer);
    qjs::js_free_value(ctx, ready);
    qjs::js_free_value(ctx, global);
    ready_flag || has_set_html
}

unsafe fn truesurfer_failed(ctx: *mut qjs::JSContext) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let ready =
        qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_READY_PROP.as_ptr() as *const c_char);
    let mut ready_f = 0.0f64;
    let failed = qjs::JS_ToFloat64(ctx, &mut ready_f as *mut f64, ready) == 0
        && ready_f.is_finite()
        && ready_f < 0.0;
    qjs::js_free_value(ctx, ready);
    qjs::js_free_value(ctx, global);
    failed
}

unsafe fn read_result_u32(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &[u8]) -> u32 {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok =
        qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite() && out >= 0.0;
    qjs::js_free_value(ctx, value);
    if ok { out as u32 } else { 0 }
}

unsafe fn read_result_f32(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &[u8]) -> f32 {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite();
    qjs::js_free_value(ctx, value);
    if ok { out as f32 } else { 0.0 }
}

unsafe fn read_result_string(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
    label: &str,
    max_len: usize,
) -> String {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    if value.is_exception() || value.tag == qjs::JS_TAG_UNDEFINED || value.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, value);
        return String::new();
    }
    let out = js_value_to_string(ctx, value, label, max_len);
    qjs::js_free_value(ctx, value);
    strip_trueos_host_markers(out.as_str())
}

unsafe fn js_value_to_string(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValueConst,
    label: &str,
    max_len: usize,
) -> String {
    let mut len = 0usize;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, value, 0);
    if cstr.is_null() {
        return String::new();
    }
    if len > max_len {
        log_error(format!(
            "qjs-truesurfer: rejected oversized JS string field={} len={} max={}\n",
            label, len, max_len
        ));
        return String::new();
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let out = String::from_utf8_lossy(bytes).into_owned();
    qjs::JS_FreeCString(ctx, cstr);
    out
}

fn strip_trueos_host_markers(text: &str) -> String {
    const MARKER: &str = "<truesurfer-";
    const KNOWN_MARKERS: [&str; 11] = [
        "<truesurfer-parse5-trueos-host-core>",
        "<truesurfer-parse5-trueos-host-core",
        "<truesurfer-parse5-trueos-host-cor",
        "<truesurfer-parse5-trueos-host-event>",
        "<truesurfer-parse5-trueos-host-canvas>",
        "<truesurfer-parse5-trueos-host-dom>",
        "<truesurfer-parse5-trueos-host-fetch>",
        "<truesurfer-parse5-trueos-host-capture>",
        "<truesurfer-parse5-trueos-app.js>",
        "<truesurfer-parse5-trueos-app",
        "<truesurfer-init>",
    ];

    let mut cleaned = String::from(text);
    strip_trueos_bare_symbols(&mut cleaned);
    if !cleaned.contains(MARKER) {
        return cleaned;
    }

    for marker in KNOWN_MARKERS {
        while let Some(idx) = cleaned.find(marker) {
            cleaned.replace_range(idx..idx + marker.len(), "");
        }
    }

    if !cleaned.contains(MARKER) {
        return cleaned;
    }

    let mut out = String::with_capacity(cleaned.len());
    let mut rest = cleaned.as_str();
    while let Some(idx) = rest.find(MARKER) {
        out.push_str(&rest[..idx]);
        let marker_tail = &rest[idx..];
        if let Some(end_rel) = marker_tail.find('>') {
            let marker_candidate = &marker_tail[..=end_rel];
            let marker_body = &marker_candidate[1..marker_candidate.len().saturating_sub(1)];
            if marker_body
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' || ch == '_')
            {
                rest = &marker_tail[end_rel + 1..];
                continue;
            }
        }
        out.push_str(MARKER);
        rest = &marker_tail[MARKER.len()..];
    }
    out.push_str(rest);
    out
}

fn strip_trueos_bare_symbols(text: &mut String) {
    const SYMBOLS: [&str; 3] = ["__trueosNum", "__trueosNu", "__trueosN"];
    for symbol in SYMBOLS {
        while let Some(idx) = text.find(symbol) {
            text.replace_range(idx..idx + symbol.len(), "");
        }
    }
}

fn compact_log_text_sample(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut previous_space = false;
    for ch in text.chars() {
        if out.chars().count() >= max_chars {
            break;
        }
        let mapped = match ch {
            '\r' | '\n' | '\t' => ' ',
            '"' | '\\' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        };
        if mapped == ' ' {
            if out.is_empty() || previous_space {
                continue;
            }
            previous_space = true;
            out.push(mapped);
            continue;
        }
        previous_space = false;
        out.push(mapped);
    }
    out
}

unsafe fn read_array_len(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst) -> u32 {
    static LENGTH_PROP: &[u8] = b"length\0";
    read_result_u32(ctx, obj, LENGTH_PROP)
}

fn take_queued_html_for_browser(browser_instance_id: u32) -> Option<PendingHtml> {
    let queue = html_handoff_queue(browser_instance_id)?;
    let mut receiver = queue.receiver.lock();
    let slot = receiver.try_receive()?;
    let pending = PendingHtml {
        html: core::mem::take(&mut slot.html),
        url: core::mem::take(&mut slot.url),
    };
    receiver.receive_done();
    log_line(format!(
        "[surfer] pipeline DIFFBOX browser={} pull html bytes={} url={}\n",
        browser_instance_id,
        pending.html.len(),
        pending.url
    ));
    Some(pending)
}

async fn wait_for_queued_html(browser_instance_id: u32) {
    let Some(signal) = html_ready_signal(browser_instance_id) else {
        return;
    };
    signal.wait().await;
}

unsafe fn dispatch_html(
    _rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
    mut pending: PendingHtml,
) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let surfer = qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_OBJ_PROP.as_ptr() as *const c_char);
    let set_html =
        qjs::JS_GetPropertyStr(ctx, surfer, TRUESURFER_SET_HTML_PROP.as_ptr() as *const c_char);
    let html_js =
        qjs::JS_NewStringLen(ctx, pending.html.as_ptr() as *const c_char, pending.html.len());
    let meta = qjs::JS_NewObject(ctx);
    let url_js =
        qjs::JS_NewStringLen(ctx, pending.url.as_ptr() as *const c_char, pending.url.len());
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        meta,
        TRUESURFER_META_URL_PROP.as_ptr() as *const c_char,
        url_js,
    );
    let args = [html_js, meta];
    let _ = qjs::trueos_shims::trueos_cabi_browser_asset_refs_begin(browser_instance_id);
    log_line(format!(
        "qjs-truesurfer[{}]: setHtml call bytes={} url={}\n",
        browser_instance_id,
        pending.html.len(),
        pending.url
    ));
    let result = qjs::JS_Call(ctx, set_html, surfer, 2, args.as_ptr());
    log_line(format!(
        "qjs-truesurfer[{}]: setHtml returned exception={}\n",
        browser_instance_id,
        if result.is_exception() { 1 } else { 0 }
    ));

    if result.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "truesurfer setHtml");
        let parse_result = ParseResult {
            ok: false,
            url: pending.url.clone(),
            bytes: pending.html.len() as u32,
            lines: pending.html.lines().count() as u32,
            error: String::from("truesurfer setHtml exception"),
            ..ParseResult::default()
        };
        let _ = with_browser_state_mut(browser_instance_id, |state| {
            state.last_parse_result = Some(parse_result.clone());
        });
        qjs::js_free_value(ctx, result);
        qjs::js_free_value(ctx, set_html);
        qjs::js_free_value(ctx, surfer);
        qjs::js_free_value(ctx, global);
        qjs::js_free_value(ctx, args[0]);
        qjs::js_free_value(ctx, args[1]);
        return false;
    }

    log_line(format!("qjs-truesurfer[{}]: result read begin\n", browser_instance_id));
    let parse_result = ParseResult {
        ok: read_result_u32(ctx, result, TRUESURFER_RESULT_OK_PROP) >= 1,
        url: pending.url.clone(),
        bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_BYTES_PROP),
        lines: read_result_u32(ctx, result, TRUESURFER_RESULT_LINES_PROP),
        parse_ms: read_result_u32(ctx, result, TRUESURFER_RESULT_PARSE_MS_PROP),
        title: read_result_string(
            ctx,
            result,
            TRUESURFER_RESULT_TITLE_PROP,
            "title",
            TRUESURFER_RESULT_TEXT_MAX_BYTES,
        ),
        favicon_url: read_result_string(
            ctx,
            result,
            TRUESURFER_RESULT_FAVICON_URL_PROP,
            "faviconUrl",
            TRUESURFER_RESULT_TEXT_MAX_BYTES,
        ),
        shell_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_SHELL_BYTES_PROP),
        body_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_BODY_BYTES_PROP),
        style_count: read_result_u32(ctx, result, TRUESURFER_RESULT_STYLE_COUNT_PROP),
        style_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_STYLE_BYTES_PROP),
        script_count: read_result_u32(ctx, result, TRUESURFER_RESULT_SCRIPT_COUNT_PROP),
        script_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_SCRIPT_BYTES_PROP),
        error: read_result_string(
            ctx,
            result,
            TRUESURFER_RESULT_ERROR_PROP,
            "error",
            TRUESURFER_RESULT_TEXT_MAX_BYTES,
        ),
    };
    log_line(format!(
        "qjs-truesurfer[{}]: result metadata read done ok={} title_bytes={} favicon_bytes={} error_bytes={}\n",
        browser_instance_id,
        if parse_result.ok { 1 } else { 0 },
        parse_result.title.len(),
        parse_result.favicon_url.len(),
        parse_result.error.len()
    ));
    let ui3_render_hash = read_result_string(
        ctx,
        result,
        TRUESURFER_RESULT_RENDER_HASH_PROP,
        "renderHash",
        TRUESURFER_RESULT_HASH_MAX_BYTES,
    );
    let ui3_layout_hash = read_result_string(
        ctx,
        result,
        TRUESURFER_RESULT_LAYOUT_HASH_PROP,
        "layoutHash",
        TRUESURFER_RESULT_HASH_MAX_BYTES,
    );
    let ui3_render_tree_json =
        read_result_string(
            ctx,
            result,
            TRUESURFER_RESULT_RENDER_TREE_JSON_PROP,
            "renderTreeJson",
            TRUESURFER_RESULT_JSON_MAX_BYTES,
        );
    let ui3_layout_trace_json =
        read_result_string(
            ctx,
            result,
            TRUESURFER_RESULT_LAYOUT_TRACE_JSON_PROP,
            "layoutTraceJson",
            TRUESURFER_RESULT_JSON_MAX_BYTES,
        );
    log_line(format!(
        "qjs-truesurfer[{}]: result read done ok={} bytes={} body_bytes={} styles={} scripts={}\n",
        browser_instance_id,
        if parse_result.ok { 1 } else { 0 },
        parse_result.bytes,
        parse_result.body_bytes,
        parse_result.style_count,
        parse_result.script_count
    ));
    let ui3_seq = if parse_result.ok {
        publish_ui3_render_tree_frame_for_browser(
            browser_instance_id,
            parse_result.url.clone(),
            ui3_render_hash,
            ui3_layout_hash,
            ui3_render_tree_json,
            ui3_layout_trace_json,
        )
        .unwrap_or(0)
    } else {
        0
    };
    log_line(format!(
        "qjs-truesurfer[{}]: widget inspect done; ui3 handoff seq={}\n",
        browser_instance_id, ui3_seq
    ));

    let _ = with_browser_state_mut(browser_instance_id, |state| {
        let parse_changed = state
            .last_parse_result
            .as_ref()
            .map(|prev| prev != &parse_result)
            .unwrap_or(true);

        if parse_changed {
            state.last_parse_result = Some(parse_result.clone());
        }
    });

    if parse_result.ok {
        log_line(format!(
            "[TrueSurfer raw] browser={} url={}\n",
            browser_instance_id, parse_result.url,
        ));
        log_line(format!(
            "qjs-truesurfer[{}]: parsed bytes={} title={} ms={} shell_bytes={} body_bytes={} styles={} scripts={} url={}\n",
            browser_instance_id,
            parse_result.bytes,
            parse_result.title,
            parse_result.parse_ms,
            parse_result.shell_bytes,
            parse_result.body_bytes,
            parse_result.style_count,
            parse_result.script_count,
            parse_result.url
        ));
    } else {
        log_error(format!(
            "qjs-truesurfer[{}]: parse failed url={} err={}\n",
            browser_instance_id, parse_result.url, parse_result.error
        ));
    }

    qjs::js_free_value(ctx, result);
    qjs::js_free_value(ctx, set_html);
    qjs::js_free_value(ctx, surfer);
    qjs::js_free_value(ctx, global);
    qjs::js_free_value(ctx, args[0]);
    qjs::js_free_value(ctx, args[1]);
    pending.html.clear();
    pending.url.clear();
    log_line(format!(
        "qjs-truesurfer[{}]: qjs values released reason=widget-inspect-complete\n",
        browser_instance_id
    ));
    log_line(format!(
        "qjs-truesurfer[{}]: pending html released reason=widget-inspect-complete\n",
        browser_instance_id
    ));
    true
}

unsafe fn runtime_has_pending_work(rt: *mut qjs::JSRuntime, ctx: *mut qjs::JSContext) -> bool {
    qjs::JS_IsJobPending(rt) > 0
        || qjs::async_ops::has_pending(ctx)
        || qjs::timers::has_pending(ctx)
        || qjs::workers::has_pending_for_ctx(ctx)
}

#[embassy_executor::task(pool_size = TRUESURFER_TASK_POOL_SIZE)]
pub async fn truesurfer_task(browser_instance_id: u32) {
    if !browser_valid(browser_instance_id) {
        log_error(format!("qjs-truesurfer[{}]: invalid browser instance\n", browser_instance_id));
        return;
    }

    let _ = with_browser_state_mut(browser_instance_id, |state| {
        state.started = true;
        state.api_ready = false;
    });
    log_line(format!("qjs-truesurfer[{}]: starting parser host\n", browser_instance_id));

    unsafe {
        let Some(vm) = qjs::vm::QjsVm::new_node_with_profile(qjs::node::RuntimeProfile::Browser)
        else {
            log_error(format!("qjs-truesurfer[{}]: JS runtime init failed\n", browser_instance_id));
            let _ = with_browser_state_mut(browser_instance_id, |state| {
                state.started = false;
            });
            return;
        };
        let ctx = vm.ctx_ptr();
        let rt = vm.rt_ptr();

        set_global_i32(ctx, TRUESURFER_ID_PROP, browser_instance_id as i32);

        log_line(format!(
            "qjs-truesurfer[{}]: renderer handoff disabled reason=widget-inspect-only\n",
            browser_instance_id
        ));

        let boot = qjs::js_eval_bytes(
            ctx,
            TRUESURFER_IMPORT_SOURCE,
            TRUESURFER_IMPORT_FILENAME.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        );
        if boot.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "truesurfer init");
            qjs::js_free_value(ctx, boot);
            let _ = with_browser_state_mut(browser_instance_id, |state| {
                state.started = false;
            });
            return;
        }
        qjs::js_free_value(ctx, boot);

        let mut last_ready = false;

        loop {
            let mut busy = false;
            let mut runtime_alive = true;
            let mut dispatched_this_cycle = false;

            for _ in 0..TRUESURFER_BUSY_PUMP_BUDGET {
                if !qjs::vm::pump_runtime_once(rt, ctx, "truesurfer") {
                    runtime_alive = false;
                    break;
                }

                let ready = truesurfer_ready(ctx);
                let failed = truesurfer_failed(ctx);
                if ready != last_ready {
                    log_line(format!(
                        "qjs-truesurfer[{}]: ready={}\n",
                        browser_instance_id,
                        if ready { 1 } else { 0 }
                    ));
                    last_ready = ready;
                }
                if failed {
                    log_line(format!("qjs-truesurfer[{}]: startup failed\n", browser_instance_id));
                    runtime_alive = false;
                    break;
                }
                let _ = with_browser_state_mut(browser_instance_id, |state| {
                    state.api_ready = ready;
                });
                let mut dispatched_html = false;
                if ready {
                    while let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                        let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
                        log_line(format!(
                            "qjs-truesurfer[{}]: dispatch returned source=ready-loop\n",
                            browser_instance_id
                        ));
                        dispatched_html = true;
                        dispatched_this_cycle = true;
                    }
                }

                if dispatched_html {
                    busy = false;
                    log_line(format!(
                        "qjs-truesurfer[{}]: dispatch loop park ready=1\n",
                        browser_instance_id
                    ));
                    break;
                }

                busy = !ready || dispatched_html || runtime_has_pending_work(rt, ctx);
                if !busy {
                    break;
                }
            }

            if !runtime_alive {
                break;
            }

            if dispatched_this_cycle {
                log_line(format!(
                    "qjs-truesurfer[{}]: waiting after widget inspect\n",
                    browser_instance_id
                ));
                wait_for_queued_html(browser_instance_id).await;
                continue;
            }

            if !busy && !runtime_has_pending_work(rt, ctx) && truesurfer_ready(ctx) {
                if let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                    let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
                    log_line(format!(
                        "qjs-truesurfer[{}]: dispatch returned source=idle-loop\n",
                        browser_instance_id
                    ));
                    continue;
                }
                wait_for_queued_html(browser_instance_id).await;
                continue;
            }

            Timer::after(EmbassyDuration::from_millis(TRUESURFER_BUSY_SLEEP_MS)).await;
        }

        let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;
    }

    let _ = with_browser_state_mut(browser_instance_id, |state| {
        state.started = false;
        state.api_ready = false;
    });
    log_line(format!("qjs-truesurfer[{}]: parser host ended\n", browser_instance_id));
}
