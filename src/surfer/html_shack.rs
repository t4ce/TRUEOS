extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeSet;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::pin::Pin;
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::{Context, Poll};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use hyper::body::{Body, Bytes, Frame, SizeHint};
use hyper::io;
use hyper::rt::{Read, ReadBufCursor, Write};
use spin::Mutex;
use v::vnet as api;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, TlsTimeouts, register_tls_app_queues};
use crate::r::net::{NetProfile, Queue};

pub(crate) const HTML_FETCH_WORKERS: usize = 10;
const HTML_FETCH_IDLE_MS: u64 = 250;
const HTML_FETCH_CONNECT_TIMEOUT_MS: u64 = 10_000;
const HTML_FETCH_DNS_TIMEOUT_MS: u64 = 5_000;
const HTML_FETCH_BODY_TIMEOUT_MS: u64 = 35_000;
const HTML_FETCH_MAX_BYTES: usize = 4 * 1024 * 1024;
const HTML_FETCH_MAX_REDIRECTS: usize = 3;
const HTML_PREVIEW_FRONT_LINES: usize = 5;
const HTML_PREVIEW_LINE_CHARS: usize = 160;
const HTML_SHACK_BROWSER_HANDOFF_ENABLE: bool = true;
const HTML_SHACK_PREVIEW_ENABLE: bool = false;
static HTML_FETCH_IDLE_LOGS: AtomicU32 = AtomicU32::new(0);
static HTML_FETCH_WORKER_SEQ: AtomicU32 = AtomicU32::new(0);
static HTML_BYTE_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);
static HTML_HTTPS_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);

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

struct ByteFetchRequest {
    id: u32,
    url: String,
    timeout_ms: u64,
    max_bytes: usize,
    method: ByteFetchMethod,
}

#[derive(Clone)]
enum ByteFetchMethod {
    Get,
    Post {
        content_type: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
}

pub struct ByteFetch {
    pub url: String,
    pub bytes: Vec<u8>,
}

struct ByteFetchCompletion {
    id: u32,
    result: Result<ByteFetch, String>,
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
    byte_request_queue: VecDeque<ByteFetchRequest>,
    ready_byte_queue: VecDeque<ByteFetchCompletion>,
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

    fn push_byte_fetch(&mut self, request: ByteFetchRequest) -> usize {
        if crate::logflag::HTML_SHACK_VERBOSE {
            crate::log!(
                "html_shack: enqueue byte_fetch id={} url={} pending_before={}\n",
                request.id,
                request.url,
                self.byte_request_queue.len()
            );
        }
        self.byte_request_queue.push_back(request);
        self.byte_request_queue.len()
    }

    fn pop_byte_fetch(&mut self) -> Option<ByteFetchRequest> {
        self.byte_request_queue.pop_front()
    }

    fn remove_byte_fetch(&mut self, id: u32) -> bool {
        if let Some(idx) = self
            .byte_request_queue
            .iter()
            .position(|request| request.id == id)
        {
            self.byte_request_queue.remove(idx);
            true
        } else {
            false
        }
    }

    fn put_byte_fetch_result(&mut self, completion: ByteFetchCompletion) -> usize {
        self.ready_byte_queue.push_back(completion);
        self.ready_byte_queue.len()
    }

    fn take_byte_fetch_result(&mut self, id: u32) -> Option<Result<ByteFetch, String>> {
        let idx = self
            .ready_byte_queue
            .iter()
            .position(|completion| completion.id == id)?;
        self.ready_byte_queue
            .remove(idx)
            .map(|completion| completion.result)
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
    let html = prepare_ready_inline_html(html);
    with_html_shack(|shack| shack.put_ready_html(html))
}

pub fn prepare_ready_inline_html(html: impl Into<String>) -> Html {
    Html::new("inline", html)
}

pub fn get_ready_file_html(file_ref: &str) -> Result<usize, HtmlShackFileError> {
    let html = prepare_ready_file_html(file_ref)?;
    Ok(with_html_shack(|shack| shack.put_ready_html(html)))
}

pub fn prepare_ready_file_html(file_ref: &str) -> Result<Html, HtmlShackFileError> {
    let path = normalize_file_reference(file_ref);
    let bytes = match crate::r::io::kfs::read_file(path.as_str()) {
        Ok(bytes) => bytes,
        Err(crate::r::io::kfs::FsError::NoRoot) => return Err(HtmlShackFileError::NoRoot),
        Err(crate::r::io::kfs::FsError::NotFound) => return Err(HtmlShackFileError::NotFound),
        Err(_) => return Err(HtmlShackFileError::ReadFailed),
    };

    let source = String::from_utf8_lossy(bytes.as_slice()).into_owned();
    Ok(Html::new(alloc::format!("file://{}", path), source))
}

fn pop_latest_request() -> Option<(HtmlRequest, usize)> {
    with_html_shack(HtmlShack::pop_latest)
}

fn pop_byte_fetch_request() -> Option<ByteFetchRequest> {
    with_html_shack(HtmlShack::pop_byte_fetch)
}

fn put_byte_fetch_result(id: u32, result: Result<ByteFetch, String>) -> usize {
    with_html_shack(|shack| shack.put_byte_fetch_result(ByteFetchCompletion { id, result }))
}

pub async fn fetch_bytes_via_pool(
    url: impl Into<String>,
    timeout_ms: u64,
    max_bytes: usize,
) -> Result<ByteFetch, String> {
    let url = url.into();
    if url.trim().is_empty() {
        return Err(String::from("empty url"));
    }
    if url.len() > 256 {
        return Err(String::from("url too long"));
    }

    let id = HTML_BYTE_FETCH_SEQ.fetch_add(1, Ordering::AcqRel);
    fetch_bytes_via_pool_method(id, url, timeout_ms, max_bytes, ByteFetchMethod::Get).await
}

pub async fn post_bytes_via_pool(
    url: impl Into<String>,
    content_type: impl Into<String>,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout_ms: u64,
    max_bytes: usize,
) -> Result<ByteFetch, String> {
    let url = url.into();
    if url.trim().is_empty() {
        return Err(String::from("empty url"));
    }
    if url.len() > 256 {
        return Err(String::from("url too long"));
    }

    let mut owned_headers = Vec::new();
    for (name, value) in headers {
        if name.trim().is_empty() {
            return Err(String::from("empty header name"));
        }
        owned_headers.push((String::from(*name), String::from(*value)));
    }

    let id = HTML_BYTE_FETCH_SEQ.fetch_add(1, Ordering::AcqRel);
    fetch_bytes_via_pool_method(
        id,
        url,
        timeout_ms,
        max_bytes,
        ByteFetchMethod::Post {
            content_type: content_type.into(),
            headers: owned_headers,
            body: Vec::from(body),
        },
    )
    .await
}

async fn fetch_bytes_via_pool_method(
    id: u32,
    url: String,
    timeout_ms: u64,
    max_bytes: usize,
    method: ByteFetchMethod,
) -> Result<ByteFetch, String> {
    with_html_shack(|shack| {
        shack.push_byte_fetch(ByteFetchRequest {
            id,
            url,
            timeout_ms,
            max_bytes,
            method,
        })
    });

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
    loop {
        if let Some(result) = with_html_shack(|shack| shack.take_byte_fetch_result(id)) {
            return result;
        }
        if Instant::now() >= deadline {
            let removed = with_html_shack(|shack| shack.remove_byte_fetch(id));
            if removed {
                return Err(String::from("timed out"));
            }
            return Err(String::from("timed out waiting for result"));
        }
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
}

async fn store_ready_html(html: Html) -> usize {
    let (ready_len, _) = enqueue_ready_html_for_browser(html).await;
    ready_len
}

pub async fn enqueue_ready_html_for_browser(html: Html) -> (usize, bool) {
    let ready_len = with_html_shack(|shack| shack.put_ready_html(html.clone()));
    let handed_off = if HTML_SHACK_BROWSER_HANDOFF_ENABLE {
        handoff_html_to_truesurfer(html).await
    } else {
        crate::log!("html_shack: browser_handoff disabled url={}\n", html.url);
        false
    };
    (ready_len, handed_off)
}

pub async fn handoff_html_to_truesurfer(html: Html) -> bool {
    let url = html.url.clone();
    let Some(ticket) = crate::surfer::queue_html_parse(html.html, Some(url.clone())).await else {
        crate::log!(
            "html_shack: browser_handoff skipped url={} reason=parse_pool_unavailable\n",
            url
        );
        return false;
    };

    crate::log!(
        "html_shack: browser_handoff url={} browser={} ok={}\n",
        url,
        ticket.browser_instance_id,
        if ticket.queued { 1 } else { 0 }
    );
    ticket.queued
}

fn normalize_file_reference(path: &str) -> String {
    let trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix('/') {
        return String::from(rest);
    }
    String::from(trimmed)
}

fn strip_url_prefix_ignore_ascii_case<'a>(url: &'a str, prefix: &str) -> Option<&'a str> {
    url.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &url[prefix.len()..])
}

fn normalize_http_url_scheme(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if let Some(rest) = strip_url_prefix_ignore_ascii_case(trimmed, "https://") {
        return Some(alloc::format!("https://{}", rest));
    }
    if let Some(rest) = strip_url_prefix_ignore_ascii_case(trimmed, "http://") {
        return Some(alloc::format!("http://{}", rest));
    }
    None
}

fn resolve_request_url(request: &HtmlRequest) -> String {
    let trimmed = request.url.trim();
    if let Some(url) = normalize_http_url_scheme(trimmed) {
        return url;
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

struct RequestBody {
    data: Option<Bytes>,
}

impl RequestBody {
    fn empty() -> Self {
        Self { data: None }
    }

    fn from_vec(bytes: Vec<u8>) -> Self {
        if bytes.is_empty() {
            Self::empty()
        } else {
            Self {
                data: Some(Bytes::from(bytes)),
            }
        }
    }
}

impl Body for RequestBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(self.data.take().map(|data| Ok(Frame::data(data))))
    }

    fn is_end_stream(&self) -> bool {
        self.data.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        let len = self.data.as_ref().map(Bytes::len).unwrap_or(0) as u64;
        SizeHint::with_exact(len)
    }
}

struct HyperVnetIo {
    net: crate::r::net::VNet,
    handle: api::NetHandle,
    rx: VecDeque<Vec<u8>>,
    closed: bool,
}

impl HyperVnetIo {
    fn new(net: crate::r::net::VNet, handle: api::NetHandle) -> Self {
        Self {
            net,
            handle,
            rx: VecDeque::new(),
            closed: false,
        }
    }

    fn pop_into(&mut self, mut buf: ReadBufCursor<'_>) -> bool {
        let Some(chunk) = self.rx.pop_front() else {
            return false;
        };

        let copied = chunk.len().min(buf.remaining());
        buf.put_slice(&chunk[..copied]);
        if copied < chunk.len() {
            self.rx.push_front(Vec::from(&chunk[copied..]));
        }
        true
    }
}

impl Read for HyperVnetIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: ReadBufCursor<'_>,
    ) -> Poll<Result<(), io::Error>> {
        if self.pop_into(buf) {
            return Poll::Ready(Ok(()));
        }
        if self.closed {
            return Poll::Ready(Ok(()));
        }

        while let Some(ev) = self.net.pop_event() {
            match ev {
                api::Event::TcpData { handle, data } if handle == self.handle => {
                    self.rx.push_back(Vec::from(data.as_slice()));
                    return Poll::Pending;
                }
                api::Event::Closed { handle } if handle == self.handle => {
                    self.closed = true;
                    return Poll::Ready(Ok(()));
                }
                api::Event::Error { .. } => return Poll::Ready(Err(io::other("vnet error"))),
                _ => {}
            }
        }

        Poll::Pending
    }
}

impl Write for HyperVnetIo {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if self.closed {
            return Poll::Ready(Err(io::not_connected("vnet tcp closed")));
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        match self.net.send_tcp_all(self.handle, buf) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(()) => Poll::Ready(Err(io::other("vnet tcp send"))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let _ = self.net.submit(api::Command::Close {
            handle: self.handle,
        });
        self.closed = true;
        Poll::Ready(Ok(()))
    }
}

#[embassy_executor::task(pool_size = 10)]
async fn html_hyper_connection_task(
    connection: hyper::client::conn::http1::Connection<HyperVnetIo, RequestBody>,
) {
    let mut connection = core::pin::pin!(connection);
    loop {
        let result = with_timeout_or_none(
            core::future::poll_fn(|cx| core::future::Future::poll(connection.as_mut(), cx)),
            1,
        )
        .await;
        let Some(result) = result else {
            continue;
        };
        if let Err(err) = result {
            crate::log!("html_shack: hyper connection ended err={:?}\n", err);
        }
        break;
    }
}

#[derive(Clone)]
struct HttpTarget {
    url: String,
    host: String,
    port: u16,
    path_and_query: String,
}

struct HttpsTarget {
    url: String,
    host: String,
    port: u16,
    path_and_query: String,
}

enum HttpFetchError {
    BadUrl,
    UnsupportedScheme,
    Https,
    NoDns,
    Dns,
    Connect,
    Hyper,
    Body,
    TooLarge,
    Redirect(String),
}

impl HttpFetchError {
    fn reason(&self) -> &'static str {
        match self {
            Self::BadUrl => "bad-url",
            Self::UnsupportedScheme => "unsupported-scheme",
            Self::Https => "https",
            Self::NoDns => "no-dns",
            Self::Dns => "dns",
            Self::Connect => "connect",
            Self::Hyper => "hyper",
            Self::Body => "body",
            Self::TooLarge => "too-large",
            Self::Redirect(_) => "redirect",
        }
    }
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn parse_http_url(url: &str) -> Result<HttpTarget, HttpFetchError> {
    let trimmed = url.trim();
    if strip_url_prefix_ignore_ascii_case(trimmed, "https://").is_some() {
        return Err(HttpFetchError::UnsupportedScheme);
    }
    let Some(rest) = strip_url_prefix_ignore_ascii_case(trimmed, "http://") else {
        return Err(HttpFetchError::BadUrl);
    };

    let authority_end = rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(HttpFetchError::BadUrl);
    }

    let mut host = authority;
    let mut port = 80u16;
    if let Some(colon) = authority.rfind(':') {
        host = &authority[..colon];
        port = authority[colon + 1..]
            .parse::<u16>()
            .map_err(|_| HttpFetchError::BadUrl)?;
    }
    if host.is_empty() {
        return Err(HttpFetchError::BadUrl);
    }

    let mut path_and_query = if authority_end >= rest.len() {
        String::from("/")
    } else {
        let suffix = &rest[authority_end..];
        if suffix.starts_with('?') {
            alloc::format!("/{}", suffix)
        } else {
            String::from(suffix)
        }
    };
    if let Some(fragment) = path_and_query.find('#') {
        path_and_query.truncate(fragment);
    }
    if path_and_query.is_empty() {
        path_and_query.push('/');
    }

    Ok(HttpTarget {
        url: String::from(trimmed),
        host: String::from(host),
        port,
        path_and_query,
    })
}

fn parse_https_url(url: &str) -> Result<HttpsTarget, HttpFetchError> {
    let trimmed = url.trim();
    let Some(rest) = strip_url_prefix_ignore_ascii_case(trimmed, "https://") else {
        return Err(HttpFetchError::BadUrl);
    };

    let authority_end = rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(HttpFetchError::BadUrl);
    }

    let mut host = authority;
    let mut port = 443u16;
    if let Some(colon) = authority.rfind(':') {
        host = &authority[..colon];
        port = authority[colon + 1..]
            .parse::<u16>()
            .map_err(|_| HttpFetchError::BadUrl)?;
    }
    if host.is_empty() {
        return Err(HttpFetchError::BadUrl);
    }

    let mut path_and_query = if authority_end >= rest.len() {
        String::from("/")
    } else {
        let suffix = &rest[authority_end..];
        if suffix.starts_with('?') {
            alloc::format!("/{}", suffix)
        } else {
            String::from(suffix)
        }
    };
    if let Some(fragment) = path_and_query.find('#') {
        path_and_query.truncate(fragment);
    }
    if path_and_query.is_empty() {
        path_and_query.push('/');
    }

    Ok(HttpsTarget {
        url: String::from(trimmed),
        host: String::from(host),
        port,
        path_and_query,
    })
}

fn parse_ipv4_literal(host: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut count = 0usize;
    for part in host.split('.') {
        if count >= out.len() || part.is_empty() {
            return None;
        }
        let octet = part.parse::<u8>().ok()?;
        out[count] = octet;
        count += 1;
    }
    if count == out.len() { Some(out) } else { None }
}

fn build_dns_a_query(host: &str, id: u16) -> Result<Vec<u8>, HttpFetchError> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());

    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 || !label.is_ascii() {
            return Err(HttpFetchError::BadUrl);
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    Ok(out)
}

fn find_http_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

fn parse_http_status(bytes: &[u8]) -> Option<u16> {
    let line_end = bytes.windows(2).position(|w| w == b"\r\n")?;
    let line = core::str::from_utf8(&bytes[..line_end]).ok()?;
    let mut parts = line.split_whitespace();
    let _http = parts.next()?;
    parts.next()?.parse::<u16>().ok()
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn header_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    for line in headers.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let key = &line[..colon];
        if key.len() == name.len()
            && key
                .iter()
                .zip(name.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
        {
            return Some(trim_ascii(&line[colon + 1..]));
        }
    }
    None
}

fn header_value_has_token(value: &[u8], token: &[u8]) -> bool {
    value.split(|b| *b == b',' || *b == b';').any(|part| {
        let part = trim_ascii(part);
        part.len() == token.len()
            && part
                .iter()
                .zip(token.iter())
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    })
}

fn decode_chunked(body: &[u8], max_bytes: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    loop {
        let line_rel = body[offset..].windows(2).position(|w| w == b"\r\n")?;
        let line = &body[offset..offset + line_rel];
        let size_text = core::str::from_utf8(line.split(|b| *b == b';').next()?).ok()?;
        let size = usize::from_str_radix(size_text.trim(), 16).ok()?;
        offset = offset.checked_add(line_rel + 2)?;
        if size == 0 {
            return Some(out);
        }
        if offset.checked_add(size + 2)? > body.len() {
            return None;
        }
        if out.len().checked_add(size)? > max_bytes {
            return None;
        }
        out.extend_from_slice(&body[offset..offset + size]);
        offset += size + 2;
    }
}

fn https_body_from_response(response: &[u8], max_bytes: usize) -> Result<Vec<u8>, HttpFetchError> {
    let hdr_end = find_http_header_end(response).ok_or(HttpFetchError::Body)?;
    let status = parse_http_status(response).ok_or(HttpFetchError::Body)?;
    if !(200..300).contains(&status) {
        return Err(HttpFetchError::Body);
    }

    let headers = &response[..hdr_end];
    let body = &response[hdr_end..];
    if let Some(te) = header_value(headers, b"transfer-encoding")
        && header_value_has_token(te, b"chunked")
    {
        return decode_chunked(body, max_bytes).ok_or(HttpFetchError::Body);
    }

    if let Some(len_text) = header_value(headers, b"content-length")
        && let Ok(len) = core::str::from_utf8(trim_ascii(len_text))
            .unwrap_or("")
            .parse::<usize>()
    {
        if len > max_bytes {
            return Err(HttpFetchError::TooLarge);
        }
        return Ok(body.get(..len).unwrap_or(body).to_vec());
    }

    if body.len() > max_bytes {
        return Err(HttpFetchError::TooLarge);
    }
    Ok(body.to_vec())
}

fn dns_skip_name(buf: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        let len = *buf.get(offset)?;
        offset += 1;
        if len == 0 {
            return Some(offset);
        }
        if len & 0xC0 == 0xC0 {
            let _ = *buf.get(offset)?;
            return Some(offset + 1);
        }
        if len & 0xC0 != 0 {
            return None;
        }
        offset = offset.checked_add(len as usize)?;
        if offset > buf.len() {
            return None;
        }
    }
}

fn parse_dns_a_response(buf: &[u8], id: u16) -> Option<[u8; 4]> {
    if buf.len() < 12 || u16::from_be_bytes([buf[0], buf[1]]) != id {
        return None;
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut offset = 12usize;
    for _ in 0..qd {
        offset = dns_skip_name(buf, offset)?.checked_add(4)?;
        if offset > buf.len() {
            return None;
        }
    }
    for _ in 0..an {
        offset = dns_skip_name(buf, offset)?;
        if offset + 10 > buf.len() {
            return None;
        }
        let ty = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let class = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]);
        let rdlen = u16::from_be_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlen > buf.len() {
            return None;
        }
        if ty == 1 && class == 1 && rdlen == 4 {
            return Some([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
        }
        offset += rdlen;
    }
    None
}

async fn wait_for_vnet_event(
    net: &crate::r::net::VNet,
    timeout_ms: u64,
    mut f: impl FnMut(api::Event) -> Option<Result<api::NetHandle, HttpFetchError>>,
) -> Result<api::NetHandle, HttpFetchError> {
    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
    loop {
        while let Some(ev) = net.pop_event() {
            if let Some(result) = f(ev) {
                return result;
            }
        }
        if Instant::now() >= deadline {
            return Err(HttpFetchError::Connect);
        }
        Timer::after(EmbassyDuration::from_micros(100)).await;
    }
}

async fn resolve_ipv4(host: &str) -> Result<[u8; 4], HttpFetchError> {
    if let Some(ip) = parse_ipv4_literal(host) {
        return Ok(ip);
    }

    match crate::r::net::dns::resolve_ipv4_primary(
        host,
        crate::r::net::dns::DnsConfig::default().with_timeout_ms(HTML_FETCH_DNS_TIMEOUT_MS),
    )
    .await
    {
        Ok(ip) => Ok(ip),
        Err(crate::r::net::dns::DnsError::NoNic) => Err(HttpFetchError::NoDns),
        Err(err) => {
            crate::log!("html_shack: dns resolve failed host={} err={:?}\n", host, err);
            Err(HttpFetchError::Dns)
        }
    }
}

async fn connect_tcp(ip: [u8; 4], port: u16) -> Result<HyperVnetIo, HttpFetchError> {
    let net = crate::r::net::VNet::open_primary().ok_or(HttpFetchError::Connect)?;
    net.submit(api::Command::OpenTcpConnect {
        remote: api::EndpointV4::new(ip, port),
    })
    .map_err(|_| HttpFetchError::Connect)?;

    let mut opened = None;
    let handle = wait_for_vnet_event(&net, HTML_FETCH_CONNECT_TIMEOUT_MS, |ev| match ev {
        api::Event::Opened {
            handle,
            kind: api::SocketKind::Tcp,
        } => {
            opened = Some(handle);
            None
        }
        api::Event::TcpEstablished { handle, .. } if opened.is_none() || opened == Some(handle) => {
            Some(Ok(handle))
        }
        api::Event::Closed { handle } if opened == Some(handle) => {
            Some(Err(HttpFetchError::Connect))
        }
        api::Event::Error { .. } => Some(Err(HttpFetchError::Connect)),
        _ => None,
    })
    .await?;

    Ok(HyperVnetIo::new(net, handle))
}

async fn with_timeout_or_none<F: core::future::Future>(
    fut: F,
    timeout_ms: u64,
) -> Option<F::Output> {
    let mut fut = core::pin::pin!(fut);
    let mut timeout = core::pin::pin!(Timer::after(EmbassyDuration::from_millis(timeout_ms)));

    core::future::poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

async fn fetch_http_body_once(
    target: &HttpTarget,
    max_bytes: usize,
    method: &ByteFetchMethod,
) -> Result<Vec<u8>, HttpFetchError> {
    let ip = resolve_ipv4(target.host.as_str()).await?;
    crate::log!(
        "html_shack: hyper connect host={} remote={}.{}.{}.{}:{} path={}\n",
        target.host,
        ip[0],
        ip[1],
        ip[2],
        ip[3],
        target.port,
        target.path_and_query
    );

    let io = connect_tcp(ip, target.port).await?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake::<_, RequestBody>(io)
        .await
        .map_err(|_| HttpFetchError::Hyper)?;
    let spawner: Spawner = unsafe { Spawner::for_current_executor().await };
    let token = html_hyper_connection_task(connection).map_err(|_| HttpFetchError::Hyper)?;
    spawner.spawn(token);

    sender.ready().await.map_err(|_| HttpFetchError::Hyper)?;
    let (http_method, request_body, content_type, extra_headers, content_len) = match method {
        ByteFetchMethod::Get => (hyper::Method::GET, RequestBody::empty(), None, &[][..], 0usize),
        ByteFetchMethod::Post {
            content_type,
            headers,
            body,
        } => (
            hyper::Method::POST,
            RequestBody::from_vec(body.clone()),
            if content_type.is_empty() {
                None
            } else {
                Some(content_type.as_str())
            },
            headers.as_slice(),
            body.len(),
        ),
    };
    let content_len_header = alloc::format!("{}", content_len);
    let mut builder = hyper::Request::builder()
        .method(http_method)
        .uri(target.path_and_query.as_str())
        .header(hyper::header::HOST, target.host.as_str())
        .header(hyper::header::USER_AGENT, "TRUEOS/html_shack")
        .header(hyper::header::ACCEPT, "text/html,*/*;q=0.8")
        .header(hyper::header::CONNECTION, "close");
    if let Some(content_type) = content_type {
        builder = builder.header(hyper::header::CONTENT_TYPE, content_type);
    }
    if matches!(method, ByteFetchMethod::Post { .. }) {
        builder = builder.header(hyper::header::CONTENT_LENGTH, content_len_header.as_str());
    }
    for (name, value) in extra_headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let request = builder
        .body(request_body)
        .map_err(|_| HttpFetchError::Hyper)?;

    let mut response = sender
        .send_request(request)
        .await
        .map_err(|_| HttpFetchError::Hyper)?;
    let status = response.status();
    if status.is_redirection() {
        if let Some(location) = response
            .headers()
            .get(hyper::header::LOCATION)
            .and_then(|value| value.to_str().ok())
        {
            return Err(HttpFetchError::Redirect(String::from(location)));
        }
    }
    if !status.is_success() {
        return Err(HttpFetchError::Body);
    }

    let mut out = Vec::new();
    let body = response.body_mut();
    loop {
        let frame = core::future::poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx)).await;
        match frame {
            Some(Ok(frame)) => {
                if let Ok(data) = frame.into_data() {
                    if out.len().saturating_add(data.len()) > max_bytes {
                        return Err(HttpFetchError::TooLarge);
                    }
                    out.extend_from_slice(&data);
                }
            }
            Some(Err(_)) => return Err(HttpFetchError::Body),
            None => break,
        }
    }

    Ok(out)
}

fn resolve_redirect(current: &HttpTarget, location: &str) -> Result<String, HttpFetchError> {
    let location = location.trim();
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(String::from(location));
    }
    if location.starts_with('/') {
        return Ok(alloc::format!("http://{}:{}{}", current.host, current.port, location));
    }
    let base = current
        .path_and_query
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("");
    Ok(alloc::format!(
        "http://{}:{}/{}{}",
        current.host,
        current.port,
        base.trim_start_matches('/'),
        location
    ))
}

async fn fetch_http_body_hyper(
    url: &str,
    max_bytes: usize,
    method: &ByteFetchMethod,
    timeout_ms: u64,
) -> Result<(String, Vec<u8>), HttpFetchError> {
    let mut current = String::from(url);
    let mut seen = BTreeSet::new();

    for hop in 0..=HTML_FETCH_MAX_REDIRECTS {
        if !seen.insert(current.clone()) {
            return Err(HttpFetchError::Body);
        }
        if let Some(https_url) = normalize_http_url_scheme(current.as_str())
            && strip_url_prefix_ignore_ascii_case(https_url.as_str(), "https://").is_some()
        {
            match method {
                ByteFetchMethod::Get => {}
                ByteFetchMethod::Post { .. } => return Err(HttpFetchError::UnsupportedScheme),
            }

            let timeout_ms = timeout_ms.min(u32::MAX as u64).max(1) as u32;
            let bytes =
                crate::r::net::https::get_bytes_shared(https_url.as_str(), timeout_ms, max_bytes)
                    .await
                    .map_err(|err| {
                        crate::log!(
                            "html_shack: https fetch failed url={} err={}\n",
                            https_url,
                            err
                        );
                        HttpFetchError::Https
                    })?;
            return Ok((https_url, bytes));
        }
        let target = parse_http_url(current.as_str())?;
        match fetch_http_body_once(&target, max_bytes, method).await {
            Ok(body) => return Ok((target.url, body)),
            Err(HttpFetchError::Redirect(next)) if hop < HTML_FETCH_MAX_REDIRECTS => {
                let next = resolve_redirect(&target, next.as_str())?;
                crate::log!("html_shack: redirect hop={} {} -> {}\n", hop + 1, target.url, next);
                current = next;
            }
            Err(err) => return Err(err),
        }
    }
    Err(HttpFetchError::Body)
}

async fn fetch_https_body_once(
    target: &HttpsTarget,
    max_bytes: usize,
    timeout_ms: u64,
) -> Result<Vec<u8>, HttpFetchError> {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
    )
    .await;

    let device_index = NetProfile::default()
        .resolve_device_index()
        .ok_or(HttpFetchError::NoDns)?;
    let ip = crate::r::net::dns::resolve_ipv4_for_device(
        device_index,
        target.host.as_str(),
        crate::r::net::dns::DnsConfig::default().with_timeout_ms(HTML_FETCH_DNS_TIMEOUT_MS),
    )
    .await
    .map_err(|err| {
        crate::log!("html_shack: https dns resolve failed host={} err={:?}\n", target.host, err);
        HttpFetchError::Dns
    })?;

    let seq = HTML_HTTPS_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let owner = leak_str(alloc::format!("html-https-fetch-{}@{}", seq, device_index));
    let cmds = Queue::new_leaked(leak_str(alloc::format!("{}-cmd", owner)), 128);
    let events = Queue::new_leaked(leak_str(alloc::format!("{}-evt", owner)), 1024);
    register_tls_app_queues(owner, cmds, events);

    cmds.push(TlsCommand::OpenTcpConnect {
        remote: api::EndpointV4::new(ip, target.port),
        server_name: leak_str(target.host.clone()),
        cfg: TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]),
        roots: TlsRoots::mozilla(),
        timeouts: TlsTimeouts {
            connect_ms: 20_000,
            tls_ms: 30_000,
            idle_ms: timeout_ms.min(u32::MAX as u64).max(1) as u32,
        },
    })
    .map_err(|_| HttpFetchError::Connect)?;

    crate::log!(
        "html_shack: https connect host={} device={} remote={}.{}.{}.{}:{} path={}\n",
        target.host,
        device_index,
        ip[0],
        ip[1],
        ip[2],
        ip[3],
        target.port,
        target.path_and_query
    );

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
    let mut tls_handle = None;
    let mut sent_request = false;
    let mut response = Vec::new();

    loop {
        for ev in events.drain(64) {
            match ev {
                TlsEvent::Opened { handle } => tls_handle = Some(handle),
                TlsEvent::Connected { handle } => {
                    if tls_handle != Some(handle) || sent_request {
                        continue;
                    }
                    let req = alloc::format!(
                        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS/html_shack\r\nAccept: text/html,*/*;q=0.8\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
                        target.path_and_query,
                        target.host
                    );
                    cmds.push(TlsCommand::Send {
                        handle,
                        data: req.into_bytes(),
                    })
                    .map_err(|_| HttpFetchError::Connect)?;
                    sent_request = true;
                }
                TlsEvent::Data { handle, data } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if response.len().saturating_add(data.len()) > max_bytes.saturating_add(4096) {
                        let _ = cmds.push(TlsCommand::Close { handle });
                        return Err(HttpFetchError::TooLarge);
                    }
                    response.extend_from_slice(data.as_slice());
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle == Some(handle) {
                        return https_body_from_response(response.as_slice(), max_bytes);
                    }
                }
                TlsEvent::Error { msg } => {
                    crate::log!(
                        "html_shack: https tls-socket error host={} msg={}\n",
                        target.host,
                        msg
                    );
                    return Err(HttpFetchError::Https);
                }
                TlsEvent::TlsError { err } => {
                    crate::log!("html_shack: https tls error host={} err={:?}\n", target.host, err);
                    return Err(HttpFetchError::Https);
                }
            }
        }

        if Instant::now() >= deadline {
            if let Some(handle) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle });
            }
            return Err(HttpFetchError::Connect);
        }
        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}

async fn fetch_html_body_for_navigation(
    url: &str,
    max_bytes: usize,
    timeout_ms: u64,
) -> Result<(String, Vec<u8>), HttpFetchError> {
    let mut current = String::from(url);
    let mut seen = BTreeSet::new();

    for hop in 0..=HTML_FETCH_MAX_REDIRECTS {
        if !seen.insert(current.clone()) {
            return Err(HttpFetchError::Body);
        }

        if strip_url_prefix_ignore_ascii_case(current.trim(), "https://").is_some() {
            let target = parse_https_url(current.as_str())?;
            let body = fetch_https_body_once(&target, max_bytes, timeout_ms).await?;
            return Ok((target.url, body));
        }

        let target = parse_http_url(current.as_str())?;
        match fetch_http_body_once(&target, max_bytes, &ByteFetchMethod::Get).await {
            Ok(body) => return Ok((target.url, body)),
            Err(HttpFetchError::Redirect(next)) if hop < HTML_FETCH_MAX_REDIRECTS => {
                let next = resolve_redirect(&target, next.as_str())?;
                crate::log!("html_shack: redirect hop={} {} -> {}\n", hop + 1, target.url, next);
                current = next;
            }
            Err(err) => return Err(err),
        }
    }
    Err(HttpFetchError::Body)
}

async fn handle_byte_fetch_request(worker_id: u32, request: ByteFetchRequest) {
    let result = with_timeout_or_none(
        fetch_http_body_hyper(
            request.url.as_str(),
            request.max_bytes,
            &request.method,
            request.timeout_ms,
        ),
        request.timeout_ms,
    )
    .await;

    let result = match result {
        Some(Ok((url, bytes))) => {
            crate::log!(
                "html_shack: byte_fetch ready worker={} id={} url={} bytes={} transport=hyper-vnet\n",
                worker_id,
                request.id,
                url,
                bytes.len()
            );
            Ok(ByteFetch { url, bytes })
        }
        Some(Err(err)) => {
            crate::log!(
                "html_shack: byte_fetch failed worker={} id={} url={} reason={}\n",
                worker_id,
                request.id,
                request.url,
                err.reason()
            );
            Err(String::from(err.reason()))
        }
        None => {
            crate::log!(
                "html_shack: byte_fetch failed worker={} id={} url={} reason=timeout\n",
                worker_id,
                request.id,
                request.url
            );
            Err(String::from("timed out"))
        }
    };

    put_byte_fetch_result(request.id, result);
}

async fn handle_html_fetch_request(
    worker_id: u32,
    mut request: HtmlRequest,
    dropped_requests: usize,
) {
    let fetch_url = resolve_request_url(&request);
    if dropped_requests > 0 {
        crate::log!(
            "html_shack: dropped stale navigation requests worker={} count={} newest={}\n",
            worker_id,
            dropped_requests,
            fetch_url
        );
    }

    if fetch_url.len() > 256 {
        crate::log!(
            "html_shack: drop worker={} url={} reason=url too long max=256\n",
            worker_id,
            fetch_url
        );
        return;
    }

    let result = with_timeout_or_none(
        fetch_html_body_for_navigation(
            fetch_url.as_str(),
            HTML_FETCH_MAX_BYTES,
            HTML_FETCH_BODY_TIMEOUT_MS,
        ),
        HTML_FETCH_BODY_TIMEOUT_MS,
    )
    .await;

    let Some(result) = result else {
        crate::log!(
            "html_shack: fetch failed worker={} url={} reason=timeout\n",
            worker_id,
            fetch_url
        );
        return;
    };

    let (source_url, bytes) = match result {
        Ok(ok) => ok,
        Err(err) => {
            crate::log!(
                "html_shack: fetch failed worker={} url={} reason={}\n",
                worker_id,
                fetch_url,
                err.reason()
            );
            return;
        }
    };

    let html_text = String::from_utf8_lossy(bytes.as_slice()).into_owned();
    if HTML_SHACK_PREVIEW_ENABLE {
        log_html_preview(source_url.as_str(), html_text.as_str());
    }

    let html = Html::new(source_url.clone(), html_text);
    if let Some(callback) = request.auto_handoff_callback.as_mut() {
        callback(html.clone());
    }
    let ready_len = store_ready_html(html).await;
    crate::log!(
        "html_shack: fetched worker={} url={} bytes={} ready={} transport=hyper-vnet\n",
        worker_id,
        source_url,
        bytes.len(),
        ready_len
    );
}

#[embassy_executor::task(pool_size = 10)]
pub async fn html_fetch_worker_task() {
    let worker_id = HTML_FETCH_WORKER_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    crate::log!(
        "html_shack: fetch worker started worker={} max_parallel={} executor=local transport=hyper-vnet latest_wins=1\n",
        worker_id,
        HTML_FETCH_WORKERS
    );

    loop {
        if let Some(request) = pop_byte_fetch_request() {
            handle_byte_fetch_request(worker_id, request).await;
            continue;
        }

        let Some((request, dropped_requests)) = pop_latest_request() else {
            if crate::logflag::HTML_SHACK_IDLE_LOGS {
                let n = HTML_FETCH_IDLE_LOGS.fetch_add(1, Ordering::Relaxed);
                if n.is_multiple_of(256) {
                    crate::log!("html_shack: fetch service idle polls={}\n", n + 1);
                }
            }
            Timer::after(EmbassyDuration::from_millis(HTML_FETCH_IDLE_MS)).await;
            continue;
        };

        handle_html_fetch_request(worker_id, request, dropped_requests).await;
    }
}
