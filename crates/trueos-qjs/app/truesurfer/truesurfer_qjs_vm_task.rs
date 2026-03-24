#![cfg(feature = "trueos")]

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

pub const MAX_BROWSER_INSTANCE_ID: u32 = 50;
pub const BOOT_BROWSER_INSTANCE_IDS: [u32; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

pub const HOSTED_KEYBOARD_MOD_SHIFT: u8 = 1 << 0;
pub const HOSTED_KEYBOARD_MOD_CTRL: u8 = 1 << 1;
pub const HOSTED_KEYBOARD_MOD_ALT: u8 = 1 << 2;
pub const HOSTED_KEYBOARD_MOD_META: u8 = 1 << 3;

const TRUESURFER_IMPORT_FILENAME: &[u8] = b"<truesurfer-init>\0";
const TRUESURFER_IMPORT_SOURCE: &[u8] = b"import '/qjs/truesurfer/truesurfer.mjs';";
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
const TRUESURFER_RESULT_SHELL_BYTES_PROP: &[u8] = b"shellBytes\0";
const TRUESURFER_RESULT_BODY_BYTES_PROP: &[u8] = b"bodyBytes\0";
const TRUESURFER_RESULT_TEXT_ROWS_PROP: &[u8] = b"textRows\0";
const TRUESURFER_RESULT_STYLE_COUNT_PROP: &[u8] = b"styleCount\0";
const TRUESURFER_RESULT_STYLE_BYTES_PROP: &[u8] = b"styleBytes\0";
const TRUESURFER_RESULT_SCRIPT_COUNT_PROP: &[u8] = b"scriptCount\0";
const TRUESURFER_RESULT_SCRIPT_BYTES_PROP: &[u8] = b"scriptBytes\0";
const TRUESURFER_RESULT_ERROR_PROP: &[u8] = b"error\0";
const TRUESURFER_TEXT_ROW_TEXT_PROP: &[u8] = b"text\0";
const TRUESURFER_TEXT_ROW_INDENT_PROP: &[u8] = b"indentPx\0";
const TRUESURFER_HTML_QUEUE_DEPTH: usize = 2;
const TRUESURFER_HTML_QUEUE_WAIT_MS: u64 = 2;
const TRUESURFER_BUSY_PUMP_BUDGET: usize = 512;
const TRUESURFER_BUSY_SLEEP_MS: u64 = 1;
const TRUESURFER_IDLE_SLEEP_MS: u64 = 16;

struct SpinRawMutex(Mutex<()>);

unsafe impl RawMutex for SpinRawMutex {
    const INIT: Self = Self(Mutex::new(()));

    fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.lock();
        f()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HostedBrowserSurfaceState {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub content_width: u32,
    pub content_height: u32,
    pub scroll_x: u32,
    pub scroll_y: u32,
}

#[derive(Clone, Debug, Default)]
pub struct HostedBrowserInteractiveItem {
    pub item_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default)]
pub struct HostedBrowserInteractiveState {
    pub interactives: alloc::vec::Vec<HostedBrowserInteractiveItem>,
}

#[derive(Clone, Debug, Default)]
pub struct HostedBrowserTextRow {
    pub text: String,
    pub indent_px: u32,
}

#[derive(Clone, Debug, Default)]
pub struct HostedBrowserTextState {
    pub rows: Vec<HostedBrowserTextRow>,
}

#[derive(Clone, Debug)]
pub enum HostedKeyboardEvent {
    Text { text: String },
    Key { key: String, modifiers: u8 },
}

#[derive(Clone, Debug, Default)]
pub struct ParseResult {
    pub ok: bool,
    pub url: String,
    pub bytes: u32,
    pub lines: u32,
    pub parse_ms: u32,
    pub title: String,
    pub shell_bytes: u32,
    pub body_bytes: u32,
    pub style_count: u32,
    pub style_bytes: u32,
    pub script_count: u32,
    pub script_bytes: u32,
    pub error: String,
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
    text_state: HostedBrowserTextState,
    window_id: u32,
    render_tex_id: u32,
    surface_seq: u32,
    interactive_seq: u32,
    text_seq: u32,
    surface_state: HostedBrowserSurfaceState,
}

static TRUESURFER_STATE: Mutex<BTreeMap<u32, BrowserInstanceState>> = Mutex::new(BTreeMap::new());
static BROWSER_RPC_SEQ: AtomicU32 = AtomicU32::new(1);
static TRUESURFER_HTML_QUEUES: Once<Vec<BrowserHtmlQueue>> = Once::new();
static TRUESURFER_HTML_READY: [Signal<SpinRawMutex, ()>; MAX_BROWSER_INSTANCE_ID as usize] =
    [const { Signal::new() }; MAX_BROWSER_INSTANCE_ID as usize];

fn text_row(text: &str, indent_px: u32) -> HostedBrowserTextRow {
    HostedBrowserTextRow {
        text: String::from(text),
        indent_px,
    }
}

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
    let state = guard.entry(browser_instance_id).or_insert_with(|| BrowserInstanceState {
        text_state: HostedBrowserTextState::default(),
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

        Timer::after(EmbassyDuration::from_millis(
            TRUESURFER_HTML_QUEUE_WAIT_MS,
        ))
        .await;
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

pub fn hosted_text_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.text_seq).unwrap_or(0)
}

pub fn hosted_surface_state_for_browser(browser_instance_id: u32) -> HostedBrowserSurfaceState {
    with_browser_state(browser_instance_id, |state| state.surface_state).unwrap_or_default()
}

pub fn hosted_interactive_state_for_browser(
    _browser_instance_id: u32,
) -> HostedBrowserInteractiveState {
    HostedBrowserInteractiveState::default()
}

pub fn hosted_text_state_for_browser(browser_instance_id: u32) -> HostedBrowserTextState {
    with_browser_state(browser_instance_id, |state| state.text_state.clone()).unwrap_or_default()
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
    with_browser_state_mut(browser_instance_id, |state| {
        state.surface_state.viewport_width = viewport_width.max(1);
        state.surface_state.viewport_height = viewport_height.max(1);
        state.surface_state.content_width = content_width.max(viewport_width.max(1));
        state.surface_state.content_height = content_height.max(1);
        state.surface_seq = state.surface_seq.wrapping_add(1);
        true
    })
    .unwrap_or(false)
}

pub fn set_hosted_scroll_for_browser(browser_instance_id: u32, scroll_x: u32, scroll_y: u32) -> bool {
    with_browser_state_mut(browser_instance_id, |state| {
        state.surface_state.scroll_x = scroll_x;
        state.surface_state.scroll_y = scroll_y;
        state.surface_seq = state.surface_seq.wrapping_add(1);
        true
    })
    .unwrap_or(false)
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

pub fn queue_hosted_keyboard_events(browser_window_id: u32, events: &[HostedKeyboardEvent]) -> bool {
    let Some(browser_instance_id) = BOOT_BROWSER_INSTANCE_IDS.iter().copied().find(|candidate| {
        browser_window_id_for_instance(*candidate) == browser_window_id
    }) else {
        return false;
    };
    with_browser_state_mut(browser_instance_id, |state| {
        state.interactive_seq = state.interactive_seq.wrapping_add(events.len() as u32);
        true
    })
    .unwrap_or(false)
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

unsafe fn truesurfer_ready(ctx: *mut qjs::JSContext) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let ready = qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_READY_PROP.as_ptr() as *const c_char);
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

unsafe fn read_result_u32(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &[u8]) -> u32 {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite() && out >= 0.0;
    qjs::js_free_value(ctx, value);
    if ok { out as u32 } else { 0 }
}

unsafe fn read_result_string(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
) -> String {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    if value.is_exception() || value.tag == qjs::JS_TAG_UNDEFINED || value.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, value);
        return String::new();
    }
    let cstr = qjs::js_to_cstring(ctx, value);
    if cstr.is_null() {
        qjs::js_free_value(ctx, value);
        return String::new();
    }
    let out = core::ffi::CStr::from_ptr(cstr)
        .to_str()
        .ok()
        .map(String::from)
        .unwrap_or_default();
    qjs::JS_FreeCString(ctx, cstr);
    qjs::js_free_value(ctx, value);
    out
}

unsafe fn read_array_len(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst) -> u32 {
    static LENGTH_PROP: &[u8] = b"length\0";
    read_result_u32(ctx, obj, LENGTH_PROP)
}

unsafe fn read_text_rows(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst) -> HostedBrowserTextState {
    let rows_value = qjs::JS_GetPropertyStr(
        ctx,
        obj,
        TRUESURFER_RESULT_TEXT_ROWS_PROP.as_ptr() as *const c_char,
    );
    if rows_value.is_exception()
        || rows_value.tag == qjs::JS_TAG_UNDEFINED
        || rows_value.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, rows_value);
        return HostedBrowserTextState::default();
    }

    let row_count = read_array_len(ctx, rows_value).min(32);
    let mut rows = Vec::with_capacity(row_count as usize);
    for idx in 0..row_count {
        let row_value = qjs::JS_GetPropertyUint32(ctx, rows_value, idx);
        if row_value.is_exception()
            || row_value.tag == qjs::JS_TAG_UNDEFINED
            || row_value.tag == qjs::JS_TAG_NULL
        {
            qjs::js_free_value(ctx, row_value);
            continue;
        }
        let text = read_result_string(ctx, row_value, TRUESURFER_TEXT_ROW_TEXT_PROP);
        if text.is_empty() {
            qjs::js_free_value(ctx, row_value);
            continue;
        }
        let indent_px = read_result_u32(ctx, row_value, TRUESURFER_TEXT_ROW_INDENT_PROP);
        rows.push(HostedBrowserTextRow { text, indent_px });
        qjs::js_free_value(ctx, row_value);
    }

    qjs::js_free_value(ctx, rows_value);
    HostedBrowserTextState { rows }
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
    pending: PendingHtml,
) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let surfer = qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_OBJ_PROP.as_ptr() as *const c_char);
    let set_html = qjs::JS_GetPropertyStr(ctx, surfer, TRUESURFER_SET_HTML_PROP.as_ptr() as *const c_char);
    let html_js = qjs::JS_NewStringLen(ctx, pending.html.as_ptr() as *const c_char, pending.html.len());
    let meta = qjs::JS_NewObject(ctx);
    let url_js = qjs::JS_NewStringLen(ctx, pending.url.as_ptr() as *const c_char, pending.url.len());
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        meta,
        TRUESURFER_META_URL_PROP.as_ptr() as *const c_char,
        url_js,
    );
    let args = [html_js, meta];
    let result = qjs::JS_Call(ctx, set_html, surfer, 2, args.as_ptr());

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

    let parse_result = ParseResult {
        ok: read_result_u32(ctx, result, TRUESURFER_RESULT_OK_PROP) >= 1,
        url: pending.url.clone(),
        bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_BYTES_PROP),
        lines: read_result_u32(ctx, result, TRUESURFER_RESULT_LINES_PROP),
        parse_ms: read_result_u32(ctx, result, TRUESURFER_RESULT_PARSE_MS_PROP),
        title: read_result_string(ctx, result, TRUESURFER_RESULT_TITLE_PROP),
        shell_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_SHELL_BYTES_PROP),
        body_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_BODY_BYTES_PROP),
        style_count: read_result_u32(ctx, result, TRUESURFER_RESULT_STYLE_COUNT_PROP),
        style_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_STYLE_BYTES_PROP),
        script_count: read_result_u32(ctx, result, TRUESURFER_RESULT_SCRIPT_COUNT_PROP),
        script_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_SCRIPT_BYTES_PROP),
        error: read_result_string(ctx, result, TRUESURFER_RESULT_ERROR_PROP),
    };
    let text_state = read_text_rows(ctx, result);

    let _ = with_browser_state_mut(browser_instance_id, |state| {
        state.last_parse_result = Some(parse_result.clone());
        state.text_state = text_state.clone();
        state.text_seq = state.text_seq.wrapping_add(1);
        if !state.text_state.rows.is_empty() {
            state.surface_state.content_height = ((state.text_state.rows.len() as u32) * 18)
                .saturating_add(20)
                .max(state.surface_state.viewport_height);
        }
    });

    if parse_result.ok {
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
            browser_instance_id,
            parse_result.url,
            parse_result.error
        ));
    }

    qjs::js_free_value(ctx, result);
    qjs::js_free_value(ctx, set_html);
    qjs::js_free_value(ctx, surfer);
    qjs::js_free_value(ctx, global);
    qjs::js_free_value(ctx, args[0]);
    qjs::js_free_value(ctx, args[1]);
    true
}

unsafe fn runtime_has_pending_work(rt: *mut qjs::JSRuntime, ctx: *mut qjs::JSContext) -> bool {
    qjs::JS_IsJobPending(rt) > 0
        || qjs::async_ops::has_pending(ctx)
        || qjs::timers::has_pending(ctx)
        || qjs::workers::has_pending_for_ctx(ctx)
}

#[embassy_executor::task(pool_size = 100)]
pub async fn truesurfer_task(browser_instance_id: u32) {
    if !browser_valid(browser_instance_id) {
        log_error(format!(
            "qjs-truesurfer[{}]: invalid browser instance\n",
            browser_instance_id
        ));
        return;
    }

    let _ = with_browser_state_mut(browser_instance_id, |state| {
        state.started = true;
        state.api_ready = false;
    });
    log_line(format!(
        "qjs-truesurfer[{}]: starting parser host\n",
        browser_instance_id
    ));

    unsafe {
        let Some(vm) = qjs::vm::QjsVm::new_node() else {
            log_error(format!(
                "qjs-truesurfer[{}]: JS runtime init failed\n",
                browser_instance_id
            ));
            let _ = with_browser_state_mut(browser_instance_id, |state| {
                state.started = false;
            });
            return;
        };
        let ctx = vm.ctx_ptr();
        let rt = vm.rt_ptr();

        qjs::node::install_globals(ctx);
        set_global_i32(ctx, TRUESURFER_ID_PROP, browser_instance_id as i32);

        let boot = qjs::js_eval_bytes(
            ctx,
            TRUESURFER_IMPORT_SOURCE,
            TRUESURFER_IMPORT_FILENAME.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_MODULE,
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

            for _ in 0..TRUESURFER_BUSY_PUMP_BUDGET {
                if !qjs::vm::pump_runtime_once(rt, ctx, "truesurfer") {
                    runtime_alive = false;
                    break;
                }

                let ready = truesurfer_ready(ctx);
                if ready != last_ready {
                    log_line(format!(
                        "qjs-truesurfer[{}]: ready={}\n",
                        browser_instance_id,
                        if ready { 1 } else { 0 }
                    ));
                    last_ready = ready;
                }
                let _ = with_browser_state_mut(browser_instance_id, |state| {
                    state.api_ready = ready;
                });
                let mut dispatched_html = false;
                if ready {
                    while let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                        let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
                        dispatched_html = true;
                    }
                }

                busy = !ready || dispatched_html || runtime_has_pending_work(rt, ctx);
                if !busy {
                    break;
                }
            }

            if !runtime_alive {
                break;
            }

            if !busy && !runtime_has_pending_work(rt, ctx) && truesurfer_ready(ctx) {
                if let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                    let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
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
    log_line(format!(
        "qjs-truesurfer[{}]: parser host ended\n",
        browser_instance_id
    ));
}