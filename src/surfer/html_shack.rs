extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String as HString;
use spin::Mutex;

const HTML_FETCH_IDLE_MS: u64 = 250;
const HTML_PREVIEW_FRONT_LINES: usize = 5;
const HTML_PREVIEW_LINE_CHARS: usize = 160;
const HTML_SHACK_BROWSER_HANDOFF_ENABLE: bool = true;
const HTML_SHACK_PREVIEW_ENABLE: bool = false;
static HTML_FETCH_IDLE_LOGS: AtomicU32 = AtomicU32::new(0);
static HTML_FETCH_WAITING_FOR_TOKIO_LOGGED: AtomicBool = AtomicBool::new(false);
static HTML_FETCH_TOKIO_READY_LOGGED: AtomicBool = AtomicBool::new(false);

/// A fetched HTML document.
///
/// We keep the source URL with the HTML body so later work can add ids,
/// diffing, and incremental DOM updates without changing the basic shape.
#[derive(Clone, Debug)]
pub struct Html {
    pub url: String,
    pub html: String,
}

impl Html {
    pub fn new(url: impl Into<String>, html: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            html: html.into(),
        }
    }
}

/// Which network road should be used when the shack manager eventually drives
/// the queued request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlRoad {
    Http,
    Https,
}

impl HtmlRoad {
    pub const fn as_scheme(self) -> &'static str {
        match self {
            Self::Http => "http://",
            Self::Https => "https://",
        }
    }
}

pub type HtmlAutoHandoffCallback = Box<dyn FnMut(Html) + Send + 'static>;

/// One queued "get this HTML ready" request.
pub struct HtmlRequest {
    pub url: String,
    pub road: HtmlRoad,
    pub auto_handoff_callback: Option<HtmlAutoHandoffCallback>,
}

impl HtmlRequest {
    pub fn new(
        url: impl Into<String>,
        road: HtmlRoad,
        auto_handoff_callback: Option<HtmlAutoHandoffCallback>,
    ) -> Self {
        Self {
            url: url.into(),
            road,
            auto_handoff_callback,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlShackFileError {
    NoRoot,
    NotFound,
    ReadFailed,
}

/// Queue-like holding box for HTML work.
///
/// Right now this only stores requests cleanly. A later spawned management
/// system can pop from it, fetch the document, and optionally hand the result
/// to the callback.
#[derive(Default)]
pub struct HtmlShack {
    html_request_queue: VecDeque<HtmlRequest>,
    ready_html_queue: VecDeque<Html>,
}

impl HtmlShack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a request so the future shack manager can act on it.
    ///
    /// Returns the queue length after enqueue so callers can cheaply confirm
    /// that the request made it into the box.
    pub fn get_ready(
        &mut self,
        url: impl Into<String>,
        road: HtmlRoad,
        auto_handoff_callback: Option<HtmlAutoHandoffCallback>,
    ) -> usize {
        let request = HtmlRequest::new(url, road, auto_handoff_callback);
        if crate::logflag::HTML_SHACK_VERBOSE {
            crate::log!(
                "html_shack: enqueue url={} road={:?} pending_before={}\n",
                request.url,
                request.road,
                self.html_request_queue.len()
            );
        }
        self.html_request_queue.push_back(request);
        self.html_request_queue.len()
    }

    pub fn pop_latest(&mut self) -> Option<(HtmlRequest, usize)> {
        let dropped = self.html_request_queue.len().saturating_sub(1);
        let latest = self.html_request_queue.pop_back()?;
        self.html_request_queue.clear();
        if crate::logflag::HTML_SHACK_VERBOSE {
            crate::log!(
                "html_shack: pop_latest url={} dropped={} pending_after=0\n",
                latest.url,
                dropped
            );
        }
        Some((latest, dropped))
    }

    pub fn put_ready_html(&mut self, html: Html) -> usize {
        self.ready_html_queue.push_back(html);
        self.ready_html_queue.len()
    }

    pub fn get_ready_inline_html(&mut self, html: impl Into<String>) -> usize {
        self.put_ready_html(Html::new("inline", html))
    }

    pub fn get_ready_file_html(&mut self, file_ref: &str) -> Result<usize, HtmlShackFileError> {
        let path = normalize_file_reference(file_ref);
        let bytes = match crate::r::io::kfs::read_file(path.as_str()) {
            Ok(bytes) => bytes,
            Err(crate::r::io::kfs::FsError::NoRoot) => return Err(HtmlShackFileError::NoRoot),
            Err(crate::r::io::kfs::FsError::NotFound) => return Err(HtmlShackFileError::NotFound),
            Err(_) => return Err(HtmlShackFileError::ReadFailed),
        };

        let html = String::from_utf8_lossy(bytes.as_slice()).into_owned();
        Ok(self.put_ready_html(Html::new(alloc::format!("file://{}", path), html)))
    }
}

static HTML_SHACK: Mutex<Option<HtmlShack>> = Mutex::new(None);

pub fn with_html_shack<R>(f: impl FnOnce(&mut HtmlShack) -> R) -> R {
    let mut guard = HTML_SHACK.lock();
    let shack = guard.get_or_insert_with(HtmlShack::new);
    f(shack)
}

pub fn get_ready_inline_html(html: impl Into<String>) -> usize {
    with_html_shack(|shack| shack.get_ready_inline_html(html))
}

pub fn get_ready_file_html(file_ref: &str) -> Result<usize, HtmlShackFileError> {
    with_html_shack(|shack| shack.get_ready_file_html(file_ref))
}

fn pop_latest_request() -> Option<(HtmlRequest, usize)> {
    with_html_shack(HtmlShack::pop_latest)
}

async fn store_ready_html(html: Html) -> usize {
    let ready_len = with_html_shack(|shack| shack.put_ready_html(html.clone()));
    if HTML_SHACK_BROWSER_HANDOFF_ENABLE {
        let _ = handoff_html_to_truesurfer(html).await;
    } else {
        crate::log!("html_shack: browser_handoff disabled url={}\n", html.url);
    }
    ready_len
}

pub async fn handoff_html_to_truesurfer(html: Html) -> bool {
    let Some(browser_instance_id) = crate::surfer::spawn_truesurfer_tab_with_html() else {
        crate::log!("html_shack: browser_handoff skipped url={} reason=spawn_failed\n", html.url);
        return false;
    };

    let handed_off = crate::surfer::queue_html_for_browser(
        browser_instance_id,
        html.html,
        Some(html.url.clone()),
    )
    .await;
    crate::log!(
        "html_shack: browser_handoff url={} browser={} ok={}\n",
        html.url,
        browser_instance_id,
        if handed_off { 1 } else { 0 }
    );
    handed_off
}

fn normalize_file_reference(path: &str) -> String {
    let trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix('/') {
        return String::from(rest);
    }
    String::from(trimmed)
}

fn resolve_request_url(request: &HtmlRequest) -> String {
    let trimmed = request.url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return String::from(trimmed);
    }

    let mut out = String::from(request.road.as_scheme());
    out.push_str(trimmed);
    out
}

fn preview_line(line: &str) -> &str {
    if line.len() <= HTML_PREVIEW_LINE_CHARS {
        return line;
    }

    let mut end = 0usize;
    for (idx, ch) in line.char_indices() {
        let next = idx + ch.len_utf8();
        if next > HTML_PREVIEW_LINE_CHARS {
            break;
        }
        end = next;
    }

    if end == 0 { "" } else { &line[..end] }
}

fn log_html_preview(url: &str, html: &str) {
    let line_count = html.lines().count();
    crate::log!(
        "html_shack: preserved url={} bytes={} lines={} front={}\n",
        url,
        html.len(),
        line_count,
        HTML_PREVIEW_FRONT_LINES
    );

    for (idx, line) in html.lines().take(HTML_PREVIEW_FRONT_LINES).enumerate() {
        let front = preview_line(line);
        if front.len() == line.len() {
            crate::log!("html_shack: [{}] {}\n", idx + 1, front);
        } else {
            crate::log!("html_shack: [{}] {}...\n", idx + 1, front);
        }
    }
}

#[embassy_executor::task]
pub async fn html_fetch_service() {
    crate::log!(
        "html_shack: fetch service started executor=local transport=shared-tokio latest_wins=1\n"
    );
    loop {
        if !crate::t::shared_tokio_runtime_ready() {
            if crate::logflag::HTML_SHACK_VERBOSE
                && !HTML_FETCH_WAITING_FOR_TOKIO_LOGGED.swap(true, Ordering::AcqRel)
            {
                crate::log!("html_shack: waiting for shared tokio runtime\n");
            }
            if crate::logflag::HTML_SHACK_IDLE_LOGS {
                let n = HTML_FETCH_IDLE_LOGS.fetch_add(1, Ordering::Relaxed);
                if n.is_multiple_of(256) {
                    crate::log!("html_shack: waiting for shared tokio runtime polls={}\n", n + 1);
                }
            }
            Timer::after(EmbassyDuration::from_millis(HTML_FETCH_IDLE_MS)).await;
            continue;
        }
        if crate::logflag::HTML_SHACK_VERBOSE
            && !HTML_FETCH_TOKIO_READY_LOGGED.swap(true, Ordering::AcqRel)
        {
            crate::log!("html_shack: shared tokio runtime ready\n");
        }

        let Some((mut request, dropped_requests)) = pop_latest_request() else {
            if crate::logflag::HTML_SHACK_IDLE_LOGS {
                let n = HTML_FETCH_IDLE_LOGS.fetch_add(1, Ordering::Relaxed);
                if n.is_multiple_of(256) {
                    crate::log!("html_shack: fetch service idle polls={}\n", n + 1);
                }
            }
            Timer::after(EmbassyDuration::from_millis(HTML_FETCH_IDLE_MS)).await;
            continue;
        };

        let fetch_url = resolve_request_url(&request);
        if dropped_requests > 0 {
            crate::log!(
                "html_shack: dropped stale navigation requests count={} newest={}\n",
                dropped_requests,
                fetch_url
            );
        }

        let mut fetch_url_buf: HString<256> = HString::new();
        if fetch_url_buf.push_str(fetch_url.as_str()).is_err() {
            crate::log!("html_shack: drop url={} reason=url too long max=256\n", fetch_url);
            continue;
        }

        if crate::logflag::HTML_SHACK_VERBOSE {
            crate::log!("html_shack: fetch begin url={}\n", fetch_url);
        }
        match crate::t::net::fetch_html_best_effort_shared("html_shack", fetch_url_buf).await {
            Ok(html) => {
                if HTML_SHACK_PREVIEW_ENABLE {
                    log_html_preview(fetch_url.as_str(), html.as_str());
                }
                let ready = Html::new(fetch_url.as_str(), html);
                let ready_len = store_ready_html(ready.clone()).await;
                crate::log!("html_shack: ready url={} ready_queue={}\n", ready.url, ready_len);

                let _ = request.auto_handoff_callback.take();
            }
            Err(err) => {
                crate::log!("html_shack: fetch failed url={} err={}\n", fetch_url, err);
            }
        }
    }
}
