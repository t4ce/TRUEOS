extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use spin::Mutex;

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
        self.html_request_queue
            .push_back(HtmlRequest::new(url, road, auto_handoff_callback));
        self.html_request_queue.len()
    }

    pub fn pop_next(&mut self) -> Option<HtmlRequest> {
        self.html_request_queue.pop_front()
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

    pub fn pop_ready_html(&mut self) -> Option<Html> {
        self.ready_html_queue.pop_front()
    }

    pub fn queued_len(&self) -> usize {
        self.html_request_queue.len()
    }

    pub fn ready_len(&self) -> usize {
        self.ready_html_queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.html_request_queue.is_empty() && self.ready_html_queue.is_empty()
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

fn normalize_file_reference(path: &str) -> String {
    let trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix('/') {
        return String::from(rest);
    }
    String::from(trimmed)
}
