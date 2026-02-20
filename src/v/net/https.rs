extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use trueos_v::vnet;

use super::Queue;
use super::dns::{self, DnsConfig};
use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, register_tls_app_queues};
use crate::surface::io::cabi::{
    FS_ERR_BAD_PARAM, FS_ERR_BAD_PATH, FS_ERR_IO, FS_ERR_NO_SPACE, FS_ERR_NOT_FOUND,
    FS_ERR_TIMEOUT, FS_ERR_TOO_LARGE, FS_ERR_USBMS_NOT_FOUND, NET_ERR_BAD_URL, NET_ERR_HTTP,
    NET_ERR_TIMEOUT, NET_ERR_TIMEOUT_BODY, NET_ERR_TIMEOUT_CONNECT, NET_ERR_TIMEOUT_DNS,
    NET_ERR_TIMEOUT_TLS, NET_ERR_TLS,
};
use crate::wait::WaitQueue;
use spin::Mutex;

static CABI_NET_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);
static CABI_NET_FETCH_RESULTS: Mutex<BTreeMap<u32, Option<i32>>> = Mutex::new(BTreeMap::new());
static CABI_NET_FETCH_WAIT: WaitQueue = WaitQueue::new();

// --- keep-alive pool (per host) ---

// NOTE: Keep-alive pooling is currently disabled.
// We observed persistent `NET_ERR_TIMEOUT_BODY` failures on some CDN (esm.sh) chunked
// responses when reusing connections; forcing fresh connections avoids the stall.
// TODO: Re-enable after the keep-alive fetch path is proven robust.
const VHTTPS_KEEPALIVE_ENABLE: bool = false;
const VHTTPS_KEEPALIVE_IDLE_CLOSE_MS: u64 = 10_000;

static VHTTPS_KEEPALIVE_SEQ: AtomicU32 = AtomicU32::new(1);

struct KeepAliveConn {
    cmds: &'static Queue<TlsCommand>,
    events: &'static Queue<TlsEvent>,
    in_use: AtomicBool,
    state: Mutex<KeepAliveState>,
}

#[derive(Clone, Copy)]
struct KeepAliveState {
    handle: Option<vnet::NetHandle>,
    connected: bool,
    last_used: Instant,
}

impl Default for KeepAliveState {
    fn default() -> Self {
        Self {
            handle: None,
            connected: false,
            last_used: Instant::from_ticks(0),
        }
    }
}

static VHTTPS_KEEPALIVE_POOL: Mutex<BTreeMap<String, &'static KeepAliveConn>> =
    Mutex::new(BTreeMap::new());

fn keepalive_pool_key(dev_idx: usize, host: &str, port: u16) -> String {
    // host is already a DNS name in our URL parser; no IPv6 here.
    alloc::format!("{}|{}|{}", dev_idx, host, port)
}

fn ensure_keepalive_conn(dev_idx: usize, host: &str, port: u16) -> &'static KeepAliveConn {
    let key = keepalive_pool_key(dev_idx, host, port);
    let mut pool = VHTTPS_KEEPALIVE_POOL.lock();
    if let Some(c) = pool.get(&key) {
        return c;
    }

    let seq = VHTTPS_KEEPALIVE_SEQ.fetch_add(1, Ordering::Relaxed);
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        alloc::format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        alloc::format!("{:04x}:{:04x}", vid, pid)
    } else {
        alloc::format!("{}", dev_idx)
    };

    // Owner suffix pins tls-socket's VNet selection to the chosen NIC.
    let owner = leak_str(alloc::format!("vhttps-ka-{}@{}", seq, selector));
    let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
    let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 4096);
    register_tls_app_queues(owner, cmds, events);

    let conn = KeepAliveConn {
        cmds,
        events,
        in_use: AtomicBool::new(false),
        state: Mutex::new(KeepAliveState::default()),
    };
    let leaked: &'static KeepAliveConn = Box::leak(Box::new(conn));
    pool.insert(key, leaked);
    leaked
}

async fn keepalive_acquire(conn: &'static KeepAliveConn) {
    loop {
        if conn
            .in_use
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return;
        }
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

fn keepalive_release(conn: &'static KeepAliveConn) {
    conn.in_use.store(false, Ordering::Release);
}

// Net-fetch scheduler (used by QJS URL module cache):
// - coalesces concurrent requests for the same cache key
// - caps concurrency to avoid TLS-handshake storms starving the executor
const NET_FETCH_MAX_CONCURRENCY: usize = 4;
static NET_FETCH_ACTIVE: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Default)]
struct InflightFetch {
    followers: Vec<u32>,
}

static CABI_NET_FETCH_INFLIGHT: Mutex<BTreeMap<String, InflightFetch>> =
    Mutex::new(BTreeMap::new());

async fn net_fetch_acquire_slot() {
    loop {
        let cur = NET_FETCH_ACTIVE.load(Ordering::Relaxed);
        if cur < NET_FETCH_MAX_CONCURRENCY
            && NET_FETCH_ACTIVE
                .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return;
        }
        // Cooperative backoff.
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

fn net_fetch_release_slot() {
    NET_FETCH_ACTIVE.fetch_sub(1, Ordering::AcqRel);
}

/// Errors returned by [`fetch_https_body_async`].
#[derive(Clone, Debug)]
pub enum FetchError {
    NoNic,
    BadUrl,
    DnsFailed,
    DnsTimeout,
    ConnectTimeout,
    TlsTimeout,
    BodyTimeout,
    Tls,
    Http(u16),
    Redirect { status: u16, url: String },
    ResponseTooLarge,
}

/// Progress callback for HTTPS body fetches.
///
/// `received` counts body bytes received so far (not including headers).
/// `total` is the Content-Length when known.
pub trait FetchProgress {
    fn on_progress(&mut self, received: usize, total: Option<usize>);
}

/// Callback sink for Server-Sent Events (SSE) streaming.
///
/// The handler receives the raw `data:` payload (already concatenated across
/// multiple `data:` lines for a single SSE event).
pub trait SseHandler {
    fn on_data(&mut self, data: &str);
}

#[inline]
fn block_error_to_code(err: crate::disc::block::Error) -> i32 {
    use crate::disc::block::Error;
    match err {
        Error::InvalidParam | Error::OutOfBounds => FS_ERR_BAD_PARAM,
        Error::NotReady => FS_ERR_USBMS_NOT_FOUND,
        Error::Corrupted
        | Error::Io
        | Error::Timeout
        | Error::NotSupported
        | Error::DmaUnavailable
        | Error::MmioMapFailed => FS_ERR_IO,
    }
}

#[inline]
fn fetch_error_to_code(err: FetchError) -> i32 {
    match err {
        FetchError::NoNic => NET_ERR_TIMEOUT,
        FetchError::BadUrl => NET_ERR_BAD_URL,
        FetchError::DnsFailed | FetchError::DnsTimeout => NET_ERR_TIMEOUT_DNS,
        FetchError::ConnectTimeout => NET_ERR_TIMEOUT_CONNECT,
        FetchError::TlsTimeout => NET_ERR_TIMEOUT_TLS,
        FetchError::BodyTimeout => NET_ERR_TIMEOUT_BODY,
        FetchError::Tls => NET_ERR_TLS,
        FetchError::Http(status) => {
            let _status = status;
            NET_ERR_HTTP
        }
        FetchError::Redirect { .. } => NET_ERR_HTTP,
        FetchError::ResponseTooLarge => FS_ERR_TOO_LARGE,
    }
}

#[inline]
fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn redirect_url_from_location(current: &ParsedHttpsUrl, headers: &[u8]) -> Option<String> {
    let loc = header_get_value(headers, b"location")?;
    let loc = core::str::from_utf8(loc).ok()?.trim();
    if loc.is_empty() {
        return None;
    }

    // Only follow HTTPS redirects.
    if loc.starts_with("https://") {
        return Some(String::from(loc));
    }
    if loc.starts_with("http://") {
        return None;
    }

    // Origin-relative redirect: "/path".
    if loc.starts_with('/') {
        if current.port == 443 {
            return Some(format!("https://{}{}", current.host, loc));
        }
        return Some(format!("https://{}:{}{}", current.host, current.port, loc));
    }

    None
}

fn normalize_rel(path: &str, allow_empty: bool) -> Result<String, i32> {
    let mut out = String::new();
    let t = path.trim();
    if t.is_empty() {
        return if allow_empty {
            Ok(out)
        } else {
            Err(FS_ERR_BAD_PATH)
        };
    }

    for part in t.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(FS_ERR_BAD_PATH);
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }

    if out.is_empty() && !allow_empty {
        return Err(FS_ERR_BAD_PATH);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
struct ParsedHttpsUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_https_url(url: &str) -> Option<ParsedHttpsUrl> {
    let url = url.strip_prefix("https://")?;

    // Split authority and path.
    let (authority, path) = match url.split_once('/') {
        Some((a, p)) => (a, format!("/{}", p)),
        None => (url, String::from("/")),
    };

    if authority.is_empty() {
        return None;
    }

    // Parse optional ":port" in authority.
    let (host, port) = if let Some((h, p)) = authority.rsplit_once(':') {
        // Only treat as port if digits.
        if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            let port = p.parse::<u16>().ok()?;
            (String::from(h), port)
        } else {
            (String::from(authority), 443)
        }
    } else {
        (String::from(authority), 443)
    };

    if host.is_empty() {
        return None;
    }

    Some(ParsedHttpsUrl { host, port, path })
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn parse_http_status(buf: &[u8]) -> Option<u16> {
    // Expect: HTTP/1.1 200 ...\r\n
    if !buf.starts_with(b"HTTP/") {
        return None;
    }
    let mut i = 0;
    while i < buf.len() && buf[i] != b' ' {
        i += 1;
    }
    while i < buf.len() && buf[i] == b' ' {
        i += 1;
    }
    if i + 3 > buf.len() {
        return None;
    }
    let a = *buf.get(i)?;
    let b = *buf.get(i + 1)?;
    let c = *buf.get(i + 2)?;
    if !a.is_ascii_digit() || !b.is_ascii_digit() || !c.is_ascii_digit() {
        return None;
    }
    Some(((a - b'0') as u16) * 100 + ((b - b'0') as u16) * 10 + ((c - b'0') as u16))
}

fn header_get_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    // Case-insensitive header match. Returns trimmed value bytes.
    let mut i = 0;
    while i < headers.len() {
        let line_start = i;
        while i < headers.len() && headers[i] != b'\n' {
            i += 1;
        }
        let mut line = &headers[line_start..i];
        if i < headers.len() && headers[i] == b'\n' {
            i += 1;
        }
        if let Some((&b'\r', rest)) = line.split_last() {
            line = rest;
        }
        if line.is_empty() {
            continue;
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let (k, mut v) = line.split_at(colon);
        // Skip ':'
        v = v.get(1..).unwrap_or(&[]);
        if k.len() != name.len() {
            continue;
        }
        if !k
            .iter()
            .zip(name.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            continue;
        }
        while !v.is_empty() && (v[0] == b' ' || v[0] == b'\t') {
            v = &v[1..];
        }
        return Some(v);
    }
    None
}

fn header_value_contains_token(value: &[u8], token: &[u8]) -> bool {
    let v = value
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect::<Vec<u8>>();
    let t = token
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect::<Vec<u8>>();

    v.split(|b| *b == b',' || *b == b' ' || *b == b'\t')
        .any(|part| part == t.as_slice())
}

fn header_contains_token(headers: &[u8], name: &[u8], token: &[u8]) -> bool {
    let Some(v) = header_get_value(headers, name) else {
        return false;
    };
    header_value_contains_token(v, token)
}

fn header_parse_content_length(headers: &[u8]) -> Option<usize> {
    let v = header_get_value(headers, b"content-length")?;
    let v = core::str::from_utf8(v).ok()?;
    v.trim().parse::<usize>().ok()
}

fn decode_http_chunked(body: &[u8]) -> Option<Vec<u8>> {
    // Minimal chunked decoder. Returns decoded bytes if fully present.
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;

    loop {
        // Read chunk size line.
        let line_end = body[i..].windows(2).position(|w| w == b"\r\n")?;
        let line = &body[i..i + line_end];
        i += line_end + 2;

        // Strip extensions.
        let line = line.split(|b| *b == b';').next().unwrap_or(line);
        let line_str = core::str::from_utf8(line).ok()?;
        let size = usize::from_str_radix(line_str.trim(), 16).ok()?;

        if size == 0 {
            // Ignore trailers; we're done.
            return Some(out);
        }

        if i + size > body.len() {
            return None;
        }
        out.extend_from_slice(&body[i..i + size]);
        i += size;

        // Expect CRLF after data.
        if i + 2 > body.len() || &body[i..i + 2] != b"\r\n" {
            return None;
        }
        i += 2;
    }
}

fn log_utf8_chunks(prefix: &str, s: &str) {
    // Avoid log-line truncation by splitting into multiple lines.
    // UTF-8 safe: ensure chunk boundaries are on char boundaries.
    const CHUNK: usize = 768;
    let mut i = 0usize;
    while i < s.len() {
        let mut end = (i + CHUNK).min(s.len());
        while end < s.len() && !s.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        if end == i {
            // Avoid infinite loop on unexpected boundary issues.
            end = (i + 1).min(s.len());
            while end < s.len() && !s.is_char_boundary(end) {
                end += 1;
            }
        }
        crate::log!("{}{}\n", prefix, &s[i..end]);
        i = end;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HttpBodyKind {
    ContentLength(usize),
    Chunked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HttpHead {
    status: u16,
    body: HttpBodyKind,
}

fn parse_http_head(headers: &[u8]) -> Option<HttpHead> {
    let status = parse_http_status(headers)?;
    let chunked = header_contains_token(headers, b"transfer-encoding", b"chunked");
    if chunked {
        return Some(HttpHead {
            status,
            body: HttpBodyKind::Chunked,
        });
    }
    let len = header_parse_content_length(headers)?;
    Some(HttpHead {
        status,
        body: HttpBodyKind::ContentLength(len),
    })
}

fn log_http_head(prefix: &str, host: &str, head: HttpHead) {
    match head.body {
        HttpBodyKind::ContentLength(len) => {
            crate::log!(
                "{} host={} status={} body=content-length len={}\n",
                prefix,
                host,
                head.status,
                len
            );
        }
        HttpBodyKind::Chunked => {
            crate::log!(
                "{} host={} status={} body=chunked\n",
                prefix,
                host,
                head.status
            );
        }
    }
}

async fn write_body_to_tmp_file(
    disk: crate::disc::block::DeviceHandle,
    tmp_path: &str,
    body: &[u8],
) -> Result<(), i32> {
    let Some(sh) =
        crate::v::fs::trueosfs::file_write_begin_async(disk, tmp_path, body.len() as u64)
            .await
            .map_err(block_error_to_code)?
    else {
        return Err(FS_ERR_NO_SPACE);
    };
    if !body.is_empty() {
        crate::v::fs::trueosfs::file_write_chunk_async(sh, body)
            .await
            .map_err(block_error_to_code)?;
    }
    crate::v::fs::trueosfs::file_write_finish_async(sh)
        .await
        .map_err(block_error_to_code)?;
    Ok(())
}

static VHTTPS_SEQ: AtomicU32 = AtomicU32::new(1);

// Keep vhttps logging minimal by default; verbose prints are useful for debugging
// but can flood globalog during downloads.
const VHTTPS_VERBOSE: bool = false;

async fn fetch_on_device(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    body_json: Option<&str>,
    auth_token: Option<&str>,
    mut progress: Option<&mut dyn FetchProgress>,
) -> Result<Vec<u8>, FetchError> {
    // If the caller asked for progress updates, this is likely a large transfer.
    // Avoid per-chunk logging (which floods globalog); emit a single completion line instead.
    let want_done_log = progress.is_some();

    let ip = match dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_device(dev_idx),
    )
    .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => return Err(FetchError::DnsTimeout),
        Err(_) => return Err(FetchError::DnsFailed),
    };

    let seq = VHTTPS_SEQ.fetch_add(1, Ordering::Relaxed);
    // Suffix with a stable selector so tls-socket can pin the underlying TCP socket to the chosen NIC.
    // Prefer PCI BDF (unique), otherwise fall back to VID:PID.
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        format!("{}", dev_idx)
    };
    let owner = leak_str(format!("vhttps-{}@{}", seq, selector));
    let cmds_name = leak_str(format!("{}-tls-cmd", owner));
    let evts_name = leak_str(format!("{}-tls-evt", owner));

    // These queues can see a burst of TCP segments (small `TlsEvent::Data` packets).
    // If the consumer drains too slowly, events may be dropped and large downloads can stall.
    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 4096);
    register_tls_app_queues(owner, cmds, events);

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let server_name = leak_str(parsed.host.clone());

    let start = Instant::now();
    let total_deadline = start + EmbassyDuration::from_millis(timeout_ms as u64);
    let connect_ms: u64 = (timeout_ms as u64 / 4).max(5_000);
    let tls_ms: u64 = (timeout_ms as u64 / 4).max(5_000);
    let connect_deadline = start + EmbassyDuration::from_millis(connect_ms);
    // TLS can only start after TCP open; this is a best-effort wall clock deadline.
    let tls_deadline = start + EmbassyDuration::from_millis(connect_ms.saturating_add(tls_ms));

    let mut tls_handle: Option<vnet::NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;

    // Capture plaintext up to (headers + body cap). We parse after close.
    // Keep this bounded even if a server misbehaves.
    let capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut plaintext: Vec<u8> = Vec::new();

    // Once we've seen complete headers, try to finish early (without waiting for TCP/TLS close)
    // when the response body is complete (Content-Length or chunked terminator).
    let mut hdr_end_cached: Option<usize> = None;
    let mut content_len_cached: Option<Option<usize>> = None;

    // Rate-limit progress callbacks.
    let mut last_progress: Instant = Instant::now();

    let mut last_activity = Instant::now();

    loop {
        for ev in events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    last_activity = Instant::now();
                    tls_handle = Some(handle);
                }
                TlsEvent::Connected { handle } => {
                    last_activity = Instant::now();
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !http_sent {
                        let auth = if let Some(token) = auth_token {
                            format!("Authorization: Bearer {}\r\n", token)
                        } else {
                            String::new()
                        };
                        let req = if let Some(body) = body_json {
                            let len = body.len();
                            format!(
                                "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS vhttps\r\nConnection: close\r\nContent-Type: application/json\r\n{}Content-Length: {}\r\nAccept: */*\r\n\r\n{}",
                                parsed.path, parsed.host, auth, len, body
                            )
                        } else {
                            format!(
                                "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS vhttps\r\n{}Accept: */*\r\nConnection: close\r\n\r\n",
                                parsed.path, parsed.host, auth
                            )
                        };
                        let _ = cmds.push(TlsCommand::Send {
                            handle,
                            data: req.into_bytes(),
                        });
                        http_sent = true;
                    }
                }
                TlsEvent::Data { handle, data } => {
                    last_activity = Instant::now();
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !data.is_empty() {
                        let room = capture_cap.saturating_sub(plaintext.len());
                        if room == 0 {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            return Err(FetchError::ResponseTooLarge);
                        }
                        let take = data.len().min(room);
                        plaintext.extend_from_slice(&data[..take]);
                        if take < data.len() {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            return Err(FetchError::ResponseTooLarge);
                        }

                        // If we have enough data to fully satisfy the response, finish now.
                        let hdr_end = match hdr_end_cached {
                            Some(v) => v,
                            None => {
                                let v = find_http_header_end(&plaintext);
                                if let Some(v) = v {
                                    hdr_end_cached = Some(v);
                                }
                                v.unwrap_or(0)
                            }
                        };

                        // Progress reporting: once headers are known, report body byte count.
                        if let Some(hdr_end) = hdr_end_cached
                            && hdr_end != 0
                        {
                            if content_len_cached.is_none() {
                                let headers = &plaintext[..hdr_end];
                                content_len_cached = Some(header_parse_content_length(headers));
                            }

                            if let Some(ref mut p) = progress {
                                // Avoid spamming UI: update at most ~10Hz.
                                let now = Instant::now();
                                if now.saturating_duration_since(last_progress)
                                    >= EmbassyDuration::from_millis(100)
                                {
                                    let body_len = plaintext.len().saturating_sub(hdr_end);
                                    p.on_progress(body_len, content_len_cached.unwrap_or(None));
                                    last_progress = now;
                                }
                            }
                        }
                        if hdr_end != 0 {
                            let headers = &plaintext[..hdr_end];
                            let body = &plaintext[hdr_end..];

                            let status = parse_http_status(&plaintext).unwrap_or(0);
                            if status != 200 {
                                if is_redirect_status(status)
                                    && let Some(next) = redirect_url_from_location(parsed, headers)
                                {
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    return Err(FetchError::Redirect { status, url: next });
                                }

                                // Log error bodies (often JSON) to aid debugging.
                                let is_chunked = header_contains_token(
                                    headers,
                                    b"transfer-encoding",
                                    b"chunked",
                                );
                                let decoded_body = if is_chunked {
                                    decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
                                } else if let Some(len) = header_parse_content_length(headers) {
                                    body.get(..len).unwrap_or(body).to_vec()
                                } else {
                                    body.to_vec()
                                };
                                crate::log!(
                                    "vhttps: http_error status={} body_len={}\n",
                                    status,
                                    decoded_body.len()
                                );
                                if let Ok(s) = core::str::from_utf8(decoded_body.as_slice()) {
                                    log_utf8_chunks("vhttps: http_error_body: ", s);
                                } else {
                                    crate::log!("vhttps: http_error_body: [non-utf8]\n");
                                }

                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                return Err(FetchError::Http(status));
                            }
                            // 204 No Content
                            if status == 204 {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                return Ok(Vec::new());
                            }

                            let is_chunked =
                                header_contains_token(headers, b"transfer-encoding", b"chunked");
                            if is_chunked {
                                if let Some(decoded) = decode_http_chunked(body) {
                                    if decoded.len() > max_bytes {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        return Err(FetchError::ResponseTooLarge);
                                    }
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    if let Some(ref mut p) = progress {
                                        p.on_progress(decoded.len(), Some(decoded.len()));
                                    }
                                    if want_done_log {
                                        crate::log!(
                                            "vhttps: done host={} dev={} status={} bytes={}\n",
                                            parsed.host,
                                            dev_idx,
                                            status,
                                            decoded.len(),
                                        );
                                    }
                                    return Ok(decoded);
                                }
                            } else if let Some(len) = header_parse_content_length(headers) {
                                if body.len() >= len {
                                    let out = body[..len].to_vec();
                                    if out.len() > max_bytes {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        return Err(FetchError::ResponseTooLarge);
                                    }
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    if let Some(ref mut p) = progress {
                                        p.on_progress(out.len(), Some(out.len()));
                                    }
                                    if want_done_log {
                                        crate::log!(
                                            "vhttps: done host={} dev={} status={} bytes={}\n",
                                            parsed.host,
                                            dev_idx,
                                            status,
                                            out.len(),
                                        );
                                    }
                                    return Ok(out);
                                }
                            } else {
                                // No chunked, no content-length. If Connection: close, we wait for close.
                                // If status implies no body (HEAD request, 1xx, 204, 304), handled above or implicitly.
                            }
                        }
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }

                    let Some(hdr_end) = find_http_header_end(&plaintext) else {
                        return Err(FetchError::Http(0));
                    };
                    let headers = &plaintext[..hdr_end];
                    let body = &plaintext[hdr_end..];

                    let status = parse_http_status(&plaintext).unwrap_or(0);
                    if status != 200 {
                        if is_redirect_status(status)
                            && let Some(next) = redirect_url_from_location(parsed, headers)
                        {
                            return Err(FetchError::Redirect { status, url: next });
                        }

                        // Log error bodies (often JSON) to aid debugging.
                        let is_chunked =
                            header_contains_token(headers, b"transfer-encoding", b"chunked");
                        let decoded_body = if is_chunked {
                            decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
                        } else if let Some(len) = header_parse_content_length(headers) {
                            body.get(..len).unwrap_or(body).to_vec()
                        } else {
                            body.to_vec()
                        };
                        crate::log!(
                            "vhttps: http_error status={} body_len={}\n",
                            status,
                            decoded_body.len()
                        );
                        if let Ok(s) = core::str::from_utf8(decoded_body.as_slice()) {
                            log_utf8_chunks("vhttps: http_error_body: ", s);
                        } else {
                            crate::log!("vhttps: http_error_body: [non-utf8]\n");
                        }

                        return Err(FetchError::Http(status));
                    }

                    let is_chunked =
                        header_contains_token(headers, b"transfer-encoding", b"chunked");
                    let decoded_body = if is_chunked {
                        decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
                    } else if let Some(len) = header_parse_content_length(headers) {
                        body.get(..len).unwrap_or(body).to_vec()
                    } else {
                        body.to_vec()
                    };

                    if decoded_body.len() > max_bytes {
                        return Err(FetchError::ResponseTooLarge);
                    }

                    if let Some(ref mut p) = progress {
                        p.on_progress(decoded_body.len(), Some(decoded_body.len()));
                    }

                    if want_done_log {
                        crate::log!(
                            "vhttps: done host={} dev={} status={} bytes={}\n",
                            parsed.host,
                            dev_idx,
                            status,
                            decoded_body.len(),
                        );
                    }

                    // Trim any accidental leading/trailing whitespace? No: callers want exact bytes.
                    return Ok(decoded_body);
                }
                TlsEvent::Error { .. } => {
                    // Keep waiting; underlying net can emit transient errors.
                }
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return Err(FetchError::Tls);
                }
            }
        }

        if !sent_connect {
            let t = crate::net::tls_socket::TlsTimeouts {
                connect_ms: (timeout_ms / 4).max(5_000),
                tls_ms: (timeout_ms / 4).max(5_000),
                idle_ms: timeout_ms,
            };
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
                timeouts: t,
            });
            sent_connect = true;
        }

        let now = Instant::now();
        if tls_handle.is_none() && now >= connect_deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            return Err(FetchError::ConnectTimeout);
        }
        if tls_handle.is_some() && !http_sent && now >= tls_deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            return Err(FetchError::TlsTimeout);
        }
        if http_sent {
            let idle_deadline = last_activity + EmbassyDuration::from_millis(timeout_ms as u64);
            if now >= idle_deadline {
                if let Some(h) = tls_handle {
                    let _ = cmds.push(TlsCommand::Close { handle: h });
                }
                return Err(FetchError::BodyTimeout);
            }
        }
        if now >= total_deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            // Fallback classification: we hit the total wall clock deadline.
            return Err(if tls_handle.is_none() {
                FetchError::ConnectTimeout
            } else if !http_sent {
                FetchError::TlsTimeout
            } else {
                FetchError::BodyTimeout
            });
        }

        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

async fn fetch_on_device_sse(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    body_json: &str,
    auth_token: Option<&str>,
    handler: &mut dyn SseHandler,
) -> Result<(), FetchError> {
    fn sse_json_type_hint(s: &str) -> Option<&str> {
        let needle = "\"type\":\"";
        let i = s.find(needle)?;
        let start = i + needle.len();
        let end_rel = s[start..].find('"')?;
        Some(&s[start..start + end_rel])
    }

    fn set_preview(dst: &mut String, src: &str, max_chars: usize) {
        dst.clear();
        for ch in src.chars().take(max_chars) {
            if ch.is_control() {
                dst.push(' ');
            } else {
                dst.push(ch);
            }
        }
    }

    let ip = match dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_device(dev_idx),
    )
    .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => return Err(FetchError::DnsTimeout),
        Err(_) => return Err(FetchError::DnsFailed),
    };

    let seq = VHTTPS_SEQ.fetch_add(1, Ordering::Relaxed);
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        format!("{}", dev_idx)
    };
    let owner = leak_str(format!("vhttpssse-{}@{}", seq, selector));
    let cmds_name = leak_str(format!("{}-tls-cmd", owner));
    let evts_name = leak_str(format!("{}-tls-evt", owner));
    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 4096);
    register_tls_app_queues(owner, cmds, events);

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let server_name = leak_str(parsed.host.clone());

    let start = Instant::now();
    let total_deadline = start + EmbassyDuration::from_millis(timeout_ms as u64);
    let connect_ms: u64 = (timeout_ms as u64 / 4).max(5_000);
    let tls_ms: u64 = (timeout_ms as u64 / 4).max(5_000);
    let connect_deadline = start + EmbassyDuration::from_millis(connect_ms);
    let tls_deadline = start + EmbassyDuration::from_millis(connect_ms.saturating_add(tls_ms));

    let mut tls_handle: Option<vnet::NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;
    let mut last_activity = Instant::now();

    // Capture enough plaintext for headers + some body. Keep bounded.
    let capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut plaintext: Vec<u8> = Vec::new();
    let mut hdr_end_cached: Option<usize> = None;

    // Streaming decode state.
    let mut raw_body_consumed: usize = 0;
    let mut decoded_body_len: usize = 0;
    let mut sse_buf: Vec<u8> = Vec::new();
    let mut chunked_done = false;
    let mut saw_done_event = false;
    let mut last_http_status: u16 = 0;
    let mut body_is_chunked = false;
    let mut sse_event_count: usize = 0;
    let mut last_sse_type: String = String::new();
    let mut last_sse_preview: String = String::new();
    let mut logged_http_sent = false;
    let mut logged_hdr_parsed = false;
    let mut logged_first_body = false;
    let mut logged_first_event = false;

    loop {
        for ev in events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    last_activity = Instant::now();
                    tls_handle = Some(handle);
                }
                TlsEvent::Connected { handle } => {
                    last_activity = Instant::now();
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !http_sent {
                        let auth = if let Some(token) = auth_token {
                            format!("Authorization: Bearer {}\r\n", token)
                        } else {
                            String::new()
                        };
                        let len = body_json.len();
                        let req = format!(
                            "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS vhttps\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nAccept-Encoding: identity\r\n{}Content-Length: {}\r\n\r\n{}",
                            parsed.path, parsed.host, auth, len, body_json
                        );
                        let _ = cmds.push(TlsCommand::Send {
                            handle,
                            data: req.into_bytes(),
                        });
                        http_sent = true;
                        if !logged_http_sent {
                            crate::log!(
                                "vhttps-sse: request-sent host={} dev={} timeout_ms={} body_len={}\n",
                                parsed.host,
                                dev_idx,
                                timeout_ms,
                                body_json.len(),
                            );
                            logged_http_sent = true;
                        }
                    }
                }
                TlsEvent::Data { handle, data } => {
                    last_activity = Instant::now();
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if data.is_empty() {
                        continue;
                    }

                    let room = capture_cap.saturating_sub(plaintext.len());
                    if room == 0 {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(FetchError::ResponseTooLarge);
                    }
                    let take = data.len().min(room);
                    plaintext.extend_from_slice(&data[..take]);
                    if take < data.len() {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(FetchError::ResponseTooLarge);
                    }

                    // Find headers once.
                    let hdr_end = match hdr_end_cached {
                        Some(v) => v,
                        None => {
                            let v = find_http_header_end(&plaintext);
                            if let Some(v) = v {
                                hdr_end_cached = Some(v);
                            }
                            v.unwrap_or(0)
                        }
                    };
                    if hdr_end == 0 {
                        continue;
                    }

                    let headers = &plaintext[..hdr_end];
                    let status = parse_http_status(&plaintext).unwrap_or(0);
                    last_http_status = status;
                    if status != 200 {
                        // Let existing error-body logging handle details.
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(FetchError::Http(status));
                    }

                    let is_chunked =
                        header_contains_token(headers, b"transfer-encoding", b"chunked");
                    body_is_chunked = is_chunked;
                    if !logged_hdr_parsed {
                        crate::log!(
                            "vhttps-sse: headers host={} dev={} status={} chunked={} hdr_bytes={} plain_bytes={}\n",
                            parsed.host,
                            dev_idx,
                            status,
                            is_chunked,
                            hdr_end,
                            plaintext.len(),
                        );
                        logged_hdr_parsed = true;
                    }
                    let body_raw = &plaintext[hdr_end..];

                    if is_chunked {
                        // Incremental chunked decode.
                        while !chunked_done {
                            // Need size line.
                            let rem = &body_raw[raw_body_consumed..];
                            let Some(line_end) = rem.windows(2).position(|w| w == b"\r\n") else {
                                break;
                            };
                            let line = &rem[..line_end];
                            // Strip extensions.
                            let line = line.split(|b| *b == b';').next().unwrap_or(line);
                            let Ok(line_str) = core::str::from_utf8(line) else {
                                return Err(FetchError::Http(0));
                            };
                            let Ok(size) = usize::from_str_radix(line_str.trim(), 16) else {
                                return Err(FetchError::Http(0));
                            };
                            let after_line = raw_body_consumed + line_end + 2;
                            if size == 0 {
                                // Need terminating CRLF after 0 size and possible trailers; best-effort done.
                                chunked_done = true;
                                break;
                            }
                            if after_line + size + 2 > body_raw.len() {
                                break;
                            }
                            let chunk = &body_raw[after_line..after_line + size];
                            decoded_body_len = decoded_body_len.saturating_add(chunk.len());
                            if decoded_body_len > max_bytes {
                                return Err(FetchError::ResponseTooLarge);
                            }
                            sse_buf.extend_from_slice(chunk);
                            if !logged_first_body {
                                crate::log!(
                                    "vhttps-sse: first-body host={} dev={} decoded={} raw_consumed={}\n",
                                    parsed.host,
                                    dev_idx,
                                    decoded_body_len,
                                    raw_body_consumed,
                                );
                                logged_first_body = true;
                            }
                            raw_body_consumed = after_line + size + 2;
                        }
                    } else {
                        // Non-chunked: treat raw body bytes as decoded.
                        let new = &body_raw[raw_body_consumed..];
                        if !new.is_empty() {
                            decoded_body_len = decoded_body_len.saturating_add(new.len());
                            if decoded_body_len > max_bytes {
                                return Err(FetchError::ResponseTooLarge);
                            }
                            sse_buf.extend_from_slice(new);
                            if !logged_first_body {
                                crate::log!(
                                    "vhttps-sse: first-body host={} dev={} decoded={} raw_consumed={}\n",
                                    parsed.host,
                                    dev_idx,
                                    decoded_body_len,
                                    raw_body_consumed,
                                );
                                logged_first_body = true;
                            }
                            raw_body_consumed = body_raw.len();
                        }
                    }

                    // SSE parse: emit complete events as they arrive.
                    loop {
                        let delim = if let Some(p) = sse_buf.windows(2).position(|w| w == b"\n\n") {
                            Some((p, 2))
                        } else {
                            sse_buf
                                .windows(4)
                                .position(|w| w == b"\r\n\r\n")
                                .map(|p| (p, 4))
                        };
                        let Some((pos, dlen)) = delim else { break };
                        let block = sse_buf.drain(..pos + dlen).collect::<Vec<u8>>();
                        // Strip delimiter
                        let mut block = block;
                        if block.len() >= dlen {
                            block.truncate(block.len() - dlen);
                        }
                        if block.is_empty() {
                            continue;
                        }
                        let Ok(text) = core::str::from_utf8(block.as_slice()) else {
                            continue;
                        };
                        let mut data_out = String::new();
                        for line in text.lines() {
                            let line = line.trim_end_matches('\r');
                            if let Some(rest) = line.strip_prefix("data:") {
                                let mut rest = rest;
                                if rest.starts_with(' ') {
                                    rest = &rest[1..];
                                }
                                if !data_out.is_empty() {
                                    data_out.push('\n');
                                }
                                data_out.push_str(rest);
                            }
                        }
                        if data_out == "[DONE]" {
                            saw_done_event = true;
                            break;
                        }
                        if !data_out.is_empty() {
                            sse_event_count = sse_event_count.saturating_add(1);
                            last_sse_type.clear();
                            if let Some(t) = sse_json_type_hint(data_out.as_str()) {
                                last_sse_type.push_str(t);
                            }
                            set_preview(&mut last_sse_preview, data_out.as_str(), 96);
                            if !logged_first_event {
                                crate::log!(
                                    "vhttps-sse: first-event host={} dev={} type={} preview={}\n",
                                    parsed.host,
                                    dev_idx,
                                    if last_sse_type.is_empty() {
                                        "-"
                                    } else {
                                        last_sse_type.as_str()
                                    },
                                    if last_sse_preview.is_empty() {
                                        "-"
                                    } else {
                                        last_sse_preview.as_str()
                                    },
                                );
                                logged_first_event = true;
                            }
                            handler.on_data(data_out.as_str());
                        }
                    }

                    if saw_done_event {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Ok(());
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    crate::log!(
                        "vhttps-sse: closed host={} dev={} status={} hdr={} chunked={} raw={} decoded={} sse_buf={} events={} done={} last_type={} last_preview={}\n",
                        parsed.host,
                        dev_idx,
                        last_http_status,
                        hdr_end_cached.is_some(),
                        body_is_chunked,
                        raw_body_consumed,
                        decoded_body_len,
                        sse_buf.len(),
                        sse_event_count,
                        saw_done_event,
                        if last_sse_type.is_empty() {
                            "-"
                        } else {
                            last_sse_type.as_str()
                        },
                        if last_sse_preview.is_empty() {
                            "-"
                        } else {
                            last_sse_preview.as_str()
                        },
                    );
                    // Connection closed; treat as end of stream.
                    return Ok(());
                }
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return Err(FetchError::Tls);
                }
                _ => {}
            }
        }

        if !sent_connect {
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
                timeouts: crate::net::tls_socket::TlsTimeouts {
                    connect_ms: (timeout_ms / 4).max(5_000),
                    tls_ms: (timeout_ms / 4).max(5_000),
                    idle_ms: timeout_ms,
                },
            });
            sent_connect = true;
        }

        let now = Instant::now();
        if tls_handle.is_none() && now >= connect_deadline {
            return Err(FetchError::ConnectTimeout);
        }
        if tls_handle.is_some() && !http_sent && now >= tls_deadline {
            return Err(FetchError::TlsTimeout);
        }
        if http_sent {
            let idle_deadline = last_activity + EmbassyDuration::from_millis(timeout_ms as u64);
            if now >= idle_deadline {
                crate::log!(
                    "vhttps-sse: body-timeout host={} dev={} status={} hdr={} chunked={} raw={} decoded={} sse_buf={} events={} done={} idle_ms={} last_type={} last_preview={}\n",
                    parsed.host,
                    dev_idx,
                    last_http_status,
                    hdr_end_cached.is_some(),
                    body_is_chunked,
                    raw_body_consumed,
                    decoded_body_len,
                    sse_buf.len(),
                    sse_event_count,
                    saw_done_event,
                    timeout_ms,
                    if last_sse_type.is_empty() {
                        "-"
                    } else {
                        last_sse_type.as_str()
                    },
                    if last_sse_preview.is_empty() {
                        "-"
                    } else {
                        last_sse_preview.as_str()
                    },
                );
                return Err(FetchError::BodyTimeout);
            }
        }
        if now >= total_deadline {
            let err = if tls_handle.is_none() {
                FetchError::ConnectTimeout
            } else if !http_sent {
                FetchError::TlsTimeout
            } else {
                FetchError::BodyTimeout
            };
            if matches!(err, FetchError::BodyTimeout) {
                crate::log!(
                    "vhttps-sse: total-timeout(body) host={} dev={} status={} hdr={} chunked={} raw={} decoded={} sse_buf={} events={} done={} total_ms={} last_type={} last_preview={}\n",
                    parsed.host,
                    dev_idx,
                    last_http_status,
                    hdr_end_cached.is_some(),
                    body_is_chunked,
                    raw_body_consumed,
                    decoded_body_len,
                    sse_buf.len(),
                    sse_event_count,
                    saw_done_event,
                    timeout_ms,
                    if last_sse_type.is_empty() {
                        "-"
                    } else {
                        last_sse_type.as_str()
                    },
                    if last_sse_preview.is_empty() {
                        "-"
                    } else {
                        last_sse_preview.as_str()
                    },
                );
            }
            return Err(err);
        }

        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

#[derive(Debug)]
enum FetchToFileError {
    Code(i32),
    Redirect { status: u16, url: String },
}

async fn fetch_on_device_to_file_keepalive(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    disk: crate::disc::block::DeviceHandle,
    tmp_path: &str,
) -> Result<(), FetchToFileError> {
    let conn = ensure_keepalive_conn(dev_idx, parsed.host.as_str(), parsed.port);
    keepalive_acquire(conn).await;

    // Drain any stale events (best-effort) before starting a new request.
    let _ = conn.events.drain(4096);

    // Close idle keep-alive connections (server may have timed out anyway).
    {
        let mut st = conn.state.lock();
        let idle_ms = Instant::now()
            .saturating_duration_since(st.last_used)
            .as_millis() as u64;
        if st.handle.is_some() && idle_ms > VHTTPS_KEEPALIVE_IDLE_CLOSE_MS {
            if let Some(h) = st.handle.take() {
                let _ = conn.cmds.push(TlsCommand::Close { handle: h });
            }
            st.connected = false;
        }
    }

    // Resolve DNS only when we need to (re)connect.
    let mut ip: Option<[u8; 4]> = None;
    {
        let st = conn.state.lock();
        if st.handle.is_none() || !st.connected {
            drop(st);
            match dns::resolve_ipv4_for_device(
                dev_idx,
                parsed.host.as_str(),
                DnsConfig::for_device(dev_idx),
            )
            .await
            {
                Ok(v) => ip = Some(v),
                Err(dns::DnsError::Timeout) => {
                    keepalive_release(conn);
                    return Err(FetchToFileError::Code(fetch_error_to_code(
                        FetchError::DnsTimeout,
                    )));
                }
                Err(_) => {
                    keepalive_release(conn);
                    return Err(FetchToFileError::Code(fetch_error_to_code(
                        FetchError::DnsFailed,
                    )));
                }
            }
        }
    }

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let server_name = leak_str(parsed.host.clone());

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);

    // Avoid flooding tls-socket with repeated connect requests while waiting for events.
    let mut connect_in_flight = false;

    // Ensure connected.
    loop {
        let (handle, connected) = {
            let st = conn.state.lock();
            (st.handle, st.connected)
        };

        if handle.is_some() && connected {
            break;
        }

        // If no handle, initiate a connect.
        if handle.is_none() && !connect_in_flight {
            let Some(ip) = ip else {
                keepalive_release(conn);
                return Err(FetchToFileError::Code(fetch_error_to_code(
                    FetchError::ConnectTimeout,
                )));
            };
            let _ = conn.cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
                timeouts: crate::net::tls_socket::TlsTimeouts {
                    connect_ms: (timeout_ms / 4).max(5_000),
                    tls_ms: (timeout_ms / 4).max(5_000),
                    idle_ms: timeout_ms,
                },
            });
            connect_in_flight = true;
        }

        // Wait for Opened/Connected.
        for ev in conn.events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    let mut st = conn.state.lock();
                    st.handle = Some(handle);
                }
                TlsEvent::Connected { handle } => {
                    let mut st = conn.state.lock();
                    if st.handle == Some(handle) {
                        st.connected = true;
                    }
                }
                TlsEvent::Closed { handle } => {
                    let mut st = conn.state.lock();
                    if st.handle == Some(handle) {
                        st.handle = None;
                        st.connected = false;
                        connect_in_flight = false;
                    }
                }
                TlsEvent::TlsError { .. } => {
                    let mut st = conn.state.lock();
                    st.handle = None;
                    st.connected = false;
                    connect_in_flight = false;
                }
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            keepalive_release(conn);
            return Err(FetchToFileError::Code(fetch_error_to_code(
                FetchError::TlsTimeout,
            )));
        }
        Timer::after(EmbassyDuration::from_millis(2)).await;
    }

    let handle = conn.state.lock().handle.expect("connected handle");

    // Send HTTP GET request. Use keep-alive; we will stop reading once body complete.
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS vhttps\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: keep-alive\r\n\r\n",
        parsed.path, parsed.host
    );
    let _ = conn.cmds.push(TlsCommand::Send {
        handle,
        data: req.into_bytes(),
    });

    // Response parsing/writing (mostly identical to the non-keepalive path).
    let mut header_buf: Vec<u8> = Vec::new();
    let mut header_done = false;
    let mut body_is_chunked = false;
    let mut chunked_raw_body: Vec<u8> = Vec::new();
    let chunked_capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut body_expected = 0usize;
    let mut body_written = 0usize;
    let mut stream_handle: Option<u32> = None;

    loop {
        for ev in conn.events.drain(1024) {
            match ev {
                TlsEvent::Data { handle: h, data } => {
                    if h != handle || data.is_empty() {
                        continue;
                    }

                    if !header_done {
                        header_buf.extend_from_slice(&data);
                        if header_buf.len() > (64 * 1024) {
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            keepalive_release(conn);
                            return Err(FetchToFileError::Code(fetch_error_to_code(
                                FetchError::Http(0),
                            )));
                        }

                        if let Some(hdr_end) = find_http_header_end(&header_buf) {
                            let headers = &header_buf[..hdr_end];
                            let status = parse_http_status(headers).unwrap_or(0);
                            if status != 200 {
                                if is_redirect_status(status)
                                    && let Some(next) = redirect_url_from_location(parsed, headers)
                                {
                                    keepalive_release(conn);
                                    return Err(FetchToFileError::Redirect { status, url: next });
                                }
                                keepalive_release(conn);
                                return Err(FetchToFileError::Code(fetch_error_to_code(
                                    FetchError::Http(status),
                                )));
                            }

                            let Some(head) = parse_http_head(headers) else {
                                keepalive_release(conn);
                                return Err(FetchToFileError::Code(fetch_error_to_code(
                                    FetchError::Http(0),
                                )));
                            };

                            body_expected = match head.body {
                                HttpBodyKind::ContentLength(v) => v,
                                HttpBodyKind::Chunked => {
                                    body_is_chunked = true;
                                    0
                                }
                            };

                            if !body_is_chunked && body_expected > max_bytes {
                                keepalive_release(conn);
                                return Err(FetchToFileError::Code(fetch_error_to_code(
                                    FetchError::ResponseTooLarge,
                                )));
                            }

                            if !body_is_chunked {
                                let sh = match crate::v::fs::trueosfs::file_write_begin_async(
                                    disk,
                                    tmp_path,
                                    body_expected as u64,
                                )
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?
                                {
                                    Some(h) => h,
                                    None => {
                                        keepalive_release(conn);
                                        return Err(FetchToFileError::Code(FS_ERR_NO_SPACE));
                                    }
                                };
                                stream_handle = Some(sh);
                            }

                            header_done = true;

                            let body_start = hdr_end;
                            if header_buf.len() > body_start {
                                let part = &header_buf[body_start..];
                                if body_is_chunked {
                                    let room =
                                        chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                                    let take = part.len().min(room);
                                    chunked_raw_body.extend_from_slice(&part[..take]);
                                } else if let Some(sh) = stream_handle {
                                    let rem = body_expected.saturating_sub(body_written);
                                    let take = part.len().min(rem);
                                    if take > 0 {
                                        crate::v::fs::trueosfs::file_write_chunk_async(
                                            sh,
                                            &part[..take],
                                        )
                                        .await
                                        .map_err(block_error_to_code)
                                        .map_err(FetchToFileError::Code)?;
                                        body_written = body_written.saturating_add(take);
                                    }
                                }
                            }
                            header_buf.clear();

                            if !body_is_chunked && body_written >= body_expected {
                                if let Some(sh) = stream_handle.take() {
                                    crate::v::fs::trueosfs::file_write_finish_async(sh)
                                        .await
                                        .map_err(block_error_to_code)
                                        .map_err(FetchToFileError::Code)?;
                                }
                                let mut st = conn.state.lock();
                                st.last_used = Instant::now();
                                keepalive_release(conn);
                                return Ok(());
                            }
                        }
                    } else if body_is_chunked {
                        let room = chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                        let take = data.len().min(room);
                        chunked_raw_body.extend_from_slice(&data[..take]);
                        if let Some(decoded) = decode_http_chunked(chunked_raw_body.as_slice()) {
                            if decoded.len() > max_bytes {
                                keepalive_release(conn);
                                return Err(FetchToFileError::Code(fetch_error_to_code(
                                    FetchError::ResponseTooLarge,
                                )));
                            }
                            write_body_to_tmp_file(disk, tmp_path, decoded.as_slice())
                                .await
                                .map_err(FetchToFileError::Code)?;
                            let mut st = conn.state.lock();
                            st.last_used = Instant::now();
                            keepalive_release(conn);
                            return Ok(());
                        }
                    } else if let Some(sh) = stream_handle {
                        let rem = body_expected.saturating_sub(body_written);
                        let take = data.len().min(rem);
                        if take > 0 {
                            crate::v::fs::trueosfs::file_write_chunk_async(sh, &data[..take])
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?;
                            body_written = body_written.saturating_add(take);
                        }
                        if body_written >= body_expected {
                            crate::v::fs::trueosfs::file_write_finish_async(sh)
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?;
                            let mut st = conn.state.lock();
                            st.last_used = Instant::now();
                            keepalive_release(conn);
                            return Ok(());
                        }
                    }
                }
                TlsEvent::Closed { handle: h } => {
                    if h == handle {
                        // Server closed; reset pool state and fail.
                        {
                            let mut st = conn.state.lock();
                            st.handle = None;
                            st.connected = false;
                        }
                        if let Some(sh) = stream_handle.take() {
                            let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                        }
                        keepalive_release(conn);
                        return Err(FetchToFileError::Code(fetch_error_to_code(
                            FetchError::BodyTimeout,
                        )));
                    }
                }
                TlsEvent::TlsError { .. } => {
                    {
                        let mut st = conn.state.lock();
                        st.handle = None;
                        st.connected = false;
                    }
                    if let Some(sh) = stream_handle.take() {
                        let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                    }
                    keepalive_release(conn);
                    return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::Tls)));
                }
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            if let Some(sh) = stream_handle.take() {
                let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
            }
            keepalive_release(conn);
            return Err(FetchToFileError::Code(fetch_error_to_code(
                FetchError::BodyTimeout,
            )));
        }
        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

async fn fetch_on_device_to_file(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    disk: crate::disc::block::DeviceHandle,
    tmp_path: &str,
) -> Result<(), FetchToFileError> {
    let t0 = Instant::now();

    let ip = match dns::resolve_ipv4_for_device(
        dev_idx,
        parsed.host.as_str(),
        DnsConfig::for_device(dev_idx),
    )
    .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => {
            return Err(FetchToFileError::Code(fetch_error_to_code(
                FetchError::DnsTimeout,
            )));
        }
        Err(_) => {
            return Err(FetchToFileError::Code(fetch_error_to_code(
                FetchError::DnsFailed,
            )));
        }
    };

    let t_dns = Instant::now();

    let seq = VHTTPS_SEQ.fetch_add(1, Ordering::Relaxed);
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        format!("{}", dev_idx)
    };
    let owner = leak_str(format!("vhttps-file-{}@{}", seq, selector));
    let cmds_name = leak_str(format!("{}-tls-cmd", owner));
    let evts_name = leak_str(format!("{}-tls-evt", owner));

    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 4096);
    register_tls_app_queues(owner, cmds, events);

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);
    let server_name = leak_str(parsed.host.clone());

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);
    let mut tls_handle: Option<vnet::NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;

    let mut t_open_sent: Option<Instant> = None;
    let mut t_tcp_opened: Option<Instant> = None;
    let mut t_tls_connected: Option<Instant> = None;
    let mut t_header_done: Option<Instant> = None;
    let mut t_write_begin: Option<Instant> = None;
    let mut t_write_done: Option<Instant> = None;

    let mut header_buf: Vec<u8> = Vec::new();
    let mut header_done = false;
    let mut body_is_chunked = false;
    let mut chunked_raw_body: Vec<u8> = Vec::new();
    let chunked_capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut body_expected = 0usize;
    let mut body_written = 0usize;
    let mut stream_handle: Option<u32> = None;

    let mut last_http_status: u16 = 0;

    #[inline]
    fn ms_since(a: Instant, b: Instant) -> u64 {
        b.saturating_duration_since(a).as_millis()
    }

    #[inline]
    fn log_vhttps_file_timing(
        host: &str,
        dev_idx: usize,
        status: u16,
        t0: Instant,
        t_dns: Instant,
        t_open_sent: Option<Instant>,
        t_tcp_opened: Option<Instant>,
        t_tls_connected: Option<Instant>,
        t_header_done: Option<Instant>,
        t_write_begin: Option<Instant>,
        t_write_done: Option<Instant>,
        rc: i32,
    ) {
        // Successful fetches are already summarized by the higher-level cache log.
        // Keep detailed timing only for failures (or when explicitly enabled).
        if rc == 0 && !VHTTPS_VERBOSE {
            return;
        }
        let t_end = Instant::now();
        let dns_ms = ms_since(t0, t_dns);
        let tcp_ms = match (t_open_sent, t_tcp_opened) {
            (Some(a), Some(b)) => ms_since(a, b),
            _ => 0,
        };
        let tls_ms = match (t_tcp_opened, t_tls_connected) {
            (Some(a), Some(b)) => ms_since(a, b),
            _ => 0,
        };
        let hdr_ms = match (t_tls_connected, t_header_done) {
            (Some(a), Some(b)) => ms_since(a, b),
            _ => 0,
        };
        let write_ms = match (t_write_begin, t_write_done) {
            (Some(a), Some(b)) => ms_since(a, b),
            _ => 0,
        };
        let total_ms = ms_since(t0, t_end);
        crate::log!(
            "vhttps-file: timing host={} dev={} status={} rc={} dns={}ms tcp={}ms tls={}ms hdr={}ms write={}ms total={}ms\n",
            host,
            dev_idx,
            status,
            rc,
            dns_ms,
            tcp_ms,
            tls_ms,
            hdr_ms,
            write_ms,
            total_ms,
        );
    }

    loop {
        for ev in events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    tls_handle = Some(handle);
                    if t_tcp_opened.is_none() {
                        t_tcp_opened = Some(Instant::now());
                    }
                }
                TlsEvent::Connected { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if t_tls_connected.is_none() {
                        t_tls_connected = Some(Instant::now());
                    }
                    if !http_sent {
                        let req = format!(
                            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS vhttps\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                            parsed.path, parsed.host
                        );
                        let _ = cmds.push(TlsCommand::Send {
                            handle,
                            data: req.into_bytes(),
                        });
                        http_sent = true;
                    }
                }
                TlsEvent::Data { handle, data } => {
                    if tls_handle != Some(handle) || data.is_empty() {
                        continue;
                    }

                    if !header_done {
                        header_buf.extend_from_slice(&data);
                        if header_buf.len() > (64 * 1024) {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            return Err(FetchToFileError::Code(fetch_error_to_code(
                                FetchError::Http(0),
                            )));
                        }

                        if let Some(hdr_end) = find_http_header_end(&header_buf) {
                            let headers = &header_buf[..hdr_end];
                            let status = parse_http_status(headers).unwrap_or(0);
                            last_http_status = status;
                            if status != 200 {
                                if is_redirect_status(status)
                                    && let Some(next) = redirect_url_from_location(parsed, headers)
                                {
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    if let Some(sh) = stream_handle.take() {
                                        let _ = crate::v::fs::trueosfs::file_write_abort_async(sh)
                                            .await;
                                    }
                                    log_vhttps_file_timing(
                                        parsed.host.as_str(),
                                        dev_idx,
                                        last_http_status,
                                        t0,
                                        t_dns,
                                        t_open_sent,
                                        t_tcp_opened,
                                        t_tls_connected,
                                        t_header_done,
                                        t_write_begin,
                                        t_write_done,
                                        fetch_error_to_code(FetchError::Http(status)),
                                    );
                                    return Err(FetchToFileError::Redirect { status, url: next });
                                }

                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                if let Some(sh) = stream_handle.take() {
                                    let _ =
                                        crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                let rc = fetch_error_to_code(FetchError::Http(status));
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    rc,
                                );
                                return Err(FetchToFileError::Code(rc));
                            }

                            let Some(head) = parse_http_head(headers) else {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                if let Some(sh) = stream_handle.take() {
                                    let _ =
                                        crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                crate::log!(
                                    "vhttps-file: invalid-http-head host={} hdr_bytes={}\n",
                                    parsed.host,
                                    header_buf.len()
                                );
                                let rc = fetch_error_to_code(FetchError::Http(0));
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    rc,
                                );
                                return Err(FetchToFileError::Code(rc));
                            };
                            if VHTTPS_VERBOSE {
                                log_http_head("vhttps-file: head", parsed.host.as_str(), head);
                            }

                            body_expected = match head.body {
                                HttpBodyKind::ContentLength(v) => v,
                                HttpBodyKind::Chunked => {
                                    body_is_chunked = true;
                                    0
                                }
                            };
                            if !body_is_chunked && body_expected > max_bytes {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                if let Some(sh) = stream_handle.take() {
                                    let _ =
                                        crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    rc,
                                );
                                return Err(FetchToFileError::Code(rc));
                            }

                            if !body_is_chunked {
                                if t_write_begin.is_none() {
                                    t_write_begin = Some(Instant::now());
                                }
                                let sh = match crate::v::fs::trueosfs::file_write_begin_async(
                                    disk,
                                    tmp_path,
                                    body_expected as u64,
                                )
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?
                                {
                                    Some(h) => h,
                                    None => {
                                        log_vhttps_file_timing(
                                            parsed.host.as_str(),
                                            dev_idx,
                                            last_http_status,
                                            t0,
                                            t_dns,
                                            t_open_sent,
                                            t_tcp_opened,
                                            t_tls_connected,
                                            t_header_done,
                                            t_write_begin,
                                            t_write_done,
                                            FS_ERR_NO_SPACE,
                                        );
                                        return Err(FetchToFileError::Code(FS_ERR_NO_SPACE));
                                    }
                                };
                                stream_handle = Some(sh);
                            }
                            header_done = true;
                            if t_header_done.is_none() {
                                t_header_done = Some(Instant::now());
                            }

                            let body_start = hdr_end;
                            if header_buf.len() > body_start {
                                let part = &header_buf[body_start..];
                                if body_is_chunked {
                                    let room =
                                        chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                                    if room == 0 {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                                        log_vhttps_file_timing(
                                            parsed.host.as_str(),
                                            dev_idx,
                                            last_http_status,
                                            t0,
                                            t_dns,
                                            t_open_sent,
                                            t_tcp_opened,
                                            t_tls_connected,
                                            t_header_done,
                                            t_write_begin,
                                            t_write_done,
                                            rc,
                                        );
                                        return Err(FetchToFileError::Code(rc));
                                    }
                                    let take = part.len().min(room);
                                    chunked_raw_body.extend_from_slice(&part[..take]);
                                    if take < part.len() {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                                        log_vhttps_file_timing(
                                            parsed.host.as_str(),
                                            dev_idx,
                                            last_http_status,
                                            t0,
                                            t_dns,
                                            t_open_sent,
                                            t_tcp_opened,
                                            t_tls_connected,
                                            t_header_done,
                                            t_write_begin,
                                            t_write_done,
                                            rc,
                                        );
                                        return Err(FetchToFileError::Code(rc));
                                    }
                                } else if let Some(sh) = stream_handle {
                                    let rem = body_expected.saturating_sub(body_written);
                                    let take = part.len().min(rem);
                                    if take > 0 {
                                        crate::v::fs::trueosfs::file_write_chunk_async(
                                            sh,
                                            &part[..take],
                                        )
                                        .await
                                        .map_err(block_error_to_code)
                                        .map_err(FetchToFileError::Code)?;
                                        body_written = body_written.saturating_add(take);
                                    }
                                }
                            }
                            header_buf.clear();

                            if !body_is_chunked && body_written >= body_expected {
                                if let Some(sh) = stream_handle.take() {
                                    crate::v::fs::trueosfs::file_write_finish_async(sh)
                                        .await
                                        .map_err(block_error_to_code)
                                        .map_err(FetchToFileError::Code)?;
                                    if t_write_done.is_none() {
                                        t_write_done = Some(Instant::now());
                                    }
                                }
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    0,
                                );
                                return Ok(());
                            }
                        }
                    } else if body_is_chunked {
                        let room = chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                        if room == 0 {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                rc,
                            );
                            return Err(FetchToFileError::Code(rc));
                        }
                        let take = data.len().min(room);
                        chunked_raw_body.extend_from_slice(&data[..take]);
                        if take < data.len() {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                rc,
                            );
                            return Err(FetchToFileError::Code(rc));
                        }
                        if let Some(decoded) = decode_http_chunked(chunked_raw_body.as_slice()) {
                            if decoded.len() > max_bytes {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                                log_vhttps_file_timing(
                                    parsed.host.as_str(),
                                    dev_idx,
                                    last_http_status,
                                    t0,
                                    t_dns,
                                    t_open_sent,
                                    t_tcp_opened,
                                    t_tls_connected,
                                    t_header_done,
                                    t_write_begin,
                                    t_write_done,
                                    rc,
                                );
                                return Err(FetchToFileError::Code(rc));
                            }
                            if t_write_begin.is_none() {
                                t_write_begin = Some(Instant::now());
                            }
                            write_body_to_tmp_file(disk, tmp_path, decoded.as_slice())
                                .await
                                .map_err(FetchToFileError::Code)?;
                            if t_write_done.is_none() {
                                t_write_done = Some(Instant::now());
                            }
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                0,
                            );
                            return Ok(());
                        }
                    } else if let Some(sh) = stream_handle {
                        let rem = body_expected.saturating_sub(body_written);
                        let take = data.len().min(rem);
                        if take > 0 {
                            crate::v::fs::trueosfs::file_write_chunk_async(sh, &data[..take])
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?;
                            body_written = body_written.saturating_add(take);
                        }
                        if body_written >= body_expected {
                            crate::v::fs::trueosfs::file_write_finish_async(sh)
                                .await
                                .map_err(block_error_to_code)
                                .map_err(FetchToFileError::Code)?;
                            if t_write_done.is_none() {
                                t_write_done = Some(Instant::now());
                            }
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                0,
                            );
                            return Ok(());
                        }
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if body_is_chunked && header_done {
                        let Some(decoded) = decode_http_chunked(chunked_raw_body.as_slice()) else {
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            let rc = fetch_error_to_code(FetchError::Http(0));
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                rc,
                            );
                            return Err(FetchToFileError::Code(rc));
                        };
                        if decoded.len() > max_bytes {
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            let rc = fetch_error_to_code(FetchError::ResponseTooLarge);
                            log_vhttps_file_timing(
                                parsed.host.as_str(),
                                dev_idx,
                                last_http_status,
                                t0,
                                t_dns,
                                t_open_sent,
                                t_tcp_opened,
                                t_tls_connected,
                                t_header_done,
                                t_write_begin,
                                t_write_done,
                                rc,
                            );
                            return Err(FetchToFileError::Code(rc));
                        }
                        if let Some(sh) = stream_handle.take() {
                            let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                        }
                        if t_write_begin.is_none() {
                            t_write_begin = Some(Instant::now());
                        }
                        write_body_to_tmp_file(disk, tmp_path, decoded.as_slice())
                            .await
                            .map_err(FetchToFileError::Code)?;
                        if t_write_done.is_none() {
                            t_write_done = Some(Instant::now());
                        }
                        log_vhttps_file_timing(
                            parsed.host.as_str(),
                            dev_idx,
                            last_http_status,
                            t0,
                            t_dns,
                            t_open_sent,
                            t_tcp_opened,
                            t_tls_connected,
                            t_header_done,
                            t_write_begin,
                            t_write_done,
                            0,
                        );
                        return Ok(());
                    }
                    if let Some(sh) = stream_handle.take() {
                        let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                    }
                    let rc = fetch_error_to_code(FetchError::BodyTimeout);
                    log_vhttps_file_timing(
                        parsed.host.as_str(),
                        dev_idx,
                        last_http_status,
                        t0,
                        t_dns,
                        t_open_sent,
                        t_tcp_opened,
                        t_tls_connected,
                        t_header_done,
                        t_write_begin,
                        t_write_done,
                        rc,
                    );
                    return Err(FetchToFileError::Code(rc));
                }
                TlsEvent::Error { .. } => {}
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    if let Some(sh) = stream_handle.take() {
                        let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                    }
                    let rc = fetch_error_to_code(FetchError::Tls);
                    log_vhttps_file_timing(
                        parsed.host.as_str(),
                        dev_idx,
                        last_http_status,
                        t0,
                        t_dns,
                        t_open_sent,
                        t_tcp_opened,
                        t_tls_connected,
                        t_header_done,
                        t_write_begin,
                        t_write_done,
                        rc,
                    );
                    return Err(FetchToFileError::Code(rc));
                }
            }
        }

        if !sent_connect {
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
                timeouts: crate::net::tls_socket::TlsTimeouts {
                    connect_ms: (timeout_ms / 4).max(5_000),
                    tls_ms: (timeout_ms / 4).max(5_000),
                    idle_ms: timeout_ms,
                },
            });
            if t_open_sent.is_none() {
                t_open_sent = Some(Instant::now());
            }
            sent_connect = true;
        }

        if Instant::now() >= deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            if let Some(sh) = stream_handle.take() {
                let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
            }
            let rc = if tls_handle.is_none() {
                fetch_error_to_code(FetchError::ConnectTimeout)
            } else if !http_sent {
                fetch_error_to_code(FetchError::TlsTimeout)
            } else {
                fetch_error_to_code(FetchError::BodyTimeout)
            };
            log_vhttps_file_timing(
                parsed.host.as_str(),
                dev_idx,
                last_http_status,
                t0,
                t_dns,
                t_open_sent,
                t_tcp_opened,
                t_tls_connected,
                t_header_done,
                t_write_begin,
                t_write_done,
                rc,
            );
            return Err(FetchToFileError::Code(rc));
        }

        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

/// Fetch an HTTPS URL and return the response body.
///
/// Notes:
/// - This is a minimal HTTP/1.1-over-TLS client intended for boot-time fetching.
/// - Tries each NIC index once (useful when multiple devices exist but only one is wired).
pub async fn fetch_https_body_async(
    url: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let dev_count = crate::net::device_count();
    if dev_count == 0 {
        return Err(FetchError::NoNic);
    }

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        let mut last_err: Option<FetchError> = None;
        let mut redirect: Option<(u16, String)> = None;

        for dev_idx in 0..dev_count {
            match fetch_on_device(&parsed, dev_idx, timeout_ms, max_bytes, None, None, None).await {
                Ok(v) => return Ok(v),
                Err(FetchError::Redirect { status, url }) => {
                    redirect = Some((status, url));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }

        if let Some((status, next_url)) = redirect {
            if hop >= MAX_REDIRECTS {
                return Err(FetchError::Http(status));
            }
            current_url = next_url;
            continue;
        }

        return Err(last_err.unwrap_or(FetchError::DnsFailed));
    }

    Err(FetchError::Http(0))
}

/// Fetch an HTTPS URL and return the response body, with progress updates.
///
/// Progress is based on received body bytes (after headers). If Content-Length
/// is present, `total` will be provided.
pub async fn fetch_https_body_progress_async(
    url: &str,
    timeout_ms: u32,
    max_bytes: usize,
    progress: &mut dyn FetchProgress,
) -> Result<Vec<u8>, FetchError> {
    let dev_count = crate::net::device_count();
    if dev_count == 0 {
        return Err(FetchError::NoNic);
    }

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        let mut last_err: Option<FetchError> = None;
        let mut redirect: Option<(u16, String)> = None;

        for dev_idx in 0..dev_count {
            match fetch_on_device(
                &parsed,
                dev_idx,
                timeout_ms,
                max_bytes,
                None,
                None,
                Some(progress),
            )
            .await
            {
                Ok(v) => return Ok(v),
                Err(FetchError::Redirect { status, url }) => {
                    redirect = Some((status, url));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }

        if let Some((status, next_url)) = redirect {
            if hop >= MAX_REDIRECTS {
                return Err(FetchError::Http(status));
            }
            current_url = next_url;
            continue;
        }

        return Err(last_err.unwrap_or(FetchError::DnsFailed));
    }

    Err(FetchError::Http(0))
}

pub async fn post_https_json_async(
    url: &str,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let dev_count = crate::net::device_count();
    if dev_count == 0 {
        return Err(FetchError::NoNic);
    }

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        let mut last_err: Option<FetchError> = None;
        let mut redirect: Option<(u16, String)> = None;

        for dev_idx in 0..dev_count {
            match fetch_on_device(
                &parsed,
                dev_idx,
                timeout_ms,
                max_bytes,
                Some(body_json.as_str()),
                auth_token,
                None,
            )
            .await
            {
                Ok(v) => return Ok(v),
                Err(FetchError::Redirect { status, url }) => {
                    redirect = Some((status, url));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }

        if let Some((status, next_url)) = redirect {
            if hop >= MAX_REDIRECTS {
                return Err(FetchError::Http(status));
            }
            current_url = next_url;
            continue;
        }

        return Err(last_err.unwrap_or(FetchError::DnsFailed));
    }

    Err(FetchError::Http(0))
}

/// POST JSON and stream response as SSE (`text/event-stream`).
///
/// This is intended for model streaming (`stream: true`). The handler will be
/// called for each parsed SSE `data:` payload.
pub async fn post_https_sse_async(
    url: &str,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
    handler: &mut dyn SseHandler,
) -> Result<(), FetchError> {
    let dev_count = crate::net::device_count();
    if dev_count == 0 {
        return Err(FetchError::NoNic);
    }

    const MAX_REDIRECTS: usize = 3;
    let mut current_url = String::from(url);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str()).ok_or(FetchError::BadUrl)?;

        let mut last_err: Option<FetchError> = None;
        let mut redirect: Option<(u16, String)> = None;

        for dev_idx in 0..dev_count {
            match fetch_on_device_sse(
                &parsed,
                dev_idx,
                timeout_ms,
                max_bytes,
                body_json.as_str(),
                auth_token,
                handler,
            )
            .await
            {
                Ok(()) => return Ok(()),
                Err(FetchError::Redirect { status, url }) => {
                    redirect = Some((status, url));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }

        if let Some((status, next_url)) = redirect {
            if hop >= MAX_REDIRECTS {
                return Err(FetchError::Http(status));
            }
            current_url = next_url;
            continue;
        }

        return Err(last_err.unwrap_or(FetchError::DnsFailed));
    }

    Err(FetchError::Http(0))
}

/// Fetch a URL into a TRUEOSFS key (cache file).
///
/// Behavior used by async net-fetch C-ABI:
/// - if `path` already exists: success
/// - otherwise: download body (capped), write `path.tmp`, then rename into place
pub async fn fetch_https_to_file_async(
    url: &str,
    path: &str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<(), i32> {
    let t0 = Instant::now();
    let Some(disk) = crate::v::fs::trueosfs::primary_root_handle() else {
        return Err(FS_ERR_USBMS_NOT_FOUND);
    };

    let key = normalize_rel(path, false)?;

    match crate::v::fs::trueosfs::file_exists_async(disk, key.as_str()).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => return Err(block_error_to_code(e)),
    }

    crate::log!("vhttps-cache: start key={} url={}\n", key, url);

    let t_exists = Instant::now();

    const MAX_REDIRECTS: usize = 3;

    let tmp = format!("{}.tmp", key);
    let dev_count = crate::net::device_count();
    if dev_count == 0 {
        return Err(fetch_error_to_code(FetchError::NoNic));
    }

    let mut current_url = String::from(url);
    let mut last_err = fetch_error_to_code(FetchError::DnsFailed);

    for hop in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(current_url.as_str())
            .ok_or(FetchError::BadUrl)
            .map_err(fetch_error_to_code)?;

        let mut redirect: Option<(u16, String)> = None;
        last_err = fetch_error_to_code(FetchError::DnsFailed);

        for dev_idx in 0..dev_count {
            let r = if VHTTPS_KEEPALIVE_ENABLE {
                fetch_on_device_to_file_keepalive(
                    &parsed,
                    dev_idx,
                    timeout_ms,
                    max_bytes,
                    disk,
                    tmp.as_str(),
                )
                .await
            } else {
                fetch_on_device_to_file(&parsed, dev_idx, timeout_ms, max_bytes, disk, tmp.as_str())
                    .await
            };

            match r {
                Ok(()) => {
                    last_err = 0;
                    break;
                }
                Err(FetchToFileError::Redirect { status, url }) => {
                    redirect = Some((status, url));
                    break;
                }
                Err(FetchToFileError::Code(rc)) => {
                    let _ = crate::v::fs::trueosfs::file_delete_async(disk, tmp.as_str()).await;
                    last_err = rc;
                }
            }
        }

        if last_err == 0 {
            break;
        }

        if let Some((status, next)) = redirect {
            let _ = crate::v::fs::trueosfs::file_delete_async(disk, tmp.as_str()).await;
            if hop >= MAX_REDIRECTS {
                return Err(fetch_error_to_code(FetchError::Http(status)));
            }
            current_url = next;
            continue;
        }

        return Err(last_err);
    }

    if last_err != 0 {
        return Err(last_err);
    }

    let t_dl = Instant::now();

    let rename_res =
        crate::v::fs::trueosfs::file_rename_async(disk, tmp.as_str(), key.as_str()).await;
    let t_ren = Instant::now();

    let total_ms = t_ren.saturating_duration_since(t0).as_millis();
    let exists_ms = t_exists.saturating_duration_since(t0).as_millis();
    let dl_ms = t_dl.saturating_duration_since(t_exists).as_millis();
    let ren_ms = t_ren.saturating_duration_since(t_dl).as_millis();
    crate::log!(
        "vhttps-cache: done key={} ms_total={} exists={} dl={} rename={}\n",
        key,
        total_ms,
        exists_ms,
        dl_ms,
        ren_ms
    );

    match rename_res {
        Ok(true) => Ok(()),
        Ok(false) => {
            let _ = crate::v::fs::trueosfs::file_delete_async(disk, tmp.as_str()).await;
            Err(FS_ERR_IO)
        }
        Err(e) => {
            let _ = crate::v::fs::trueosfs::file_delete_async(disk, tmp.as_str()).await;
            Err(block_error_to_code(e))
        }
    }
}

/// TRUEOS C ABI: start async HTTPS fetch to cache file.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_net_fetch_start(
    url_ptr: *const u8,
    url_len: usize,
    path_ptr: *const u8,
    path_len: usize,
) -> u32 {
    if url_ptr.is_null() || url_len == 0 || path_ptr.is_null() || path_len == 0 {
        return 0;
    }

    let url_bytes = core::slice::from_raw_parts(url_ptr, url_len);
    let path_bytes = core::slice::from_raw_parts(path_ptr, path_len);
    let Ok(url_s) = core::str::from_utf8(url_bytes) else {
        return 0;
    };
    let Ok(path_s) = core::str::from_utf8(path_bytes) else {
        return 0;
    };

    // Fixed fetch limits for loader cache path.
    //
    // This powers the QJS URL-module cache (esm.sh / CDN imports). Some responses are
    // large and/or slow enough that a ~2.5s global deadline causes spurious
    // `NET_ERR_TIMEOUT_BODY` failures even when connectivity is fine.
    const TIMEOUT_MS: u32 = 20_000;
    const MAX_BYTES: usize = 4 * 1024 * 1024;

    // Normalize the cache key so coalescing matches how fetch_https_to_file_async resolves paths.
    let key = match normalize_rel(path_s, false) {
        Ok(v) => v,
        Err(_) => return 0,
    };

    let url = String::from(url_s);
    let path = String::from(path_s);
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);

    // Coalesce duplicates: if the same cache key is already being fetched, register as follower.
    {
        let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
        if let Some(entry) = inflight.get_mut(&key) {
            entry.followers.push(op_id);
            return op_id;
        }
        inflight.insert(
            key.clone(),
            InflightFetch {
                followers: Vec::new(),
            },
        );
    }

    crate::wait::spawn_local_detached(async move {
        let t0 = Instant::now();
        net_fetch_acquire_slot().await;
        let rc = match fetch_https_to_file_async(url.as_str(), path.as_str(), TIMEOUT_MS, MAX_BYTES)
            .await
        {
            Ok(()) => 0,
            Err(code) => code,
        };
        net_fetch_release_slot();
        let elapsed_ms = t0.elapsed().as_millis();

        // Complete leader + all followers.
        let followers = {
            let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
            inflight
                .remove(&key)
                .map(|e| e.followers)
                .unwrap_or_default()
        };

        let mut map = CABI_NET_FETCH_RESULTS.lock();
        if let Some(slot) = map.get_mut(&op_id) {
            *slot = Some(rc);
        }
        for fid in &followers {
            if let Some(slot) = map.get_mut(fid) {
                *slot = Some(rc);
            }
        }

        crate::log!(
            "net-fetch: done key={} rc={} ms={} followers={}\n",
            key,
            rc,
            elapsed_ms,
            followers.len()
        );

        CABI_NET_FETCH_WAIT.notify_all();
    });
    op_id
}

/// TRUEOS C ABI: query async HTTPS fetch result.
///
/// Returns:
/// - `FS_ERR_NOT_FOUND` while operation is pending/unknown
/// - `0` on success
/// - negative error code on completion failure
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_result(op_id: u32) -> i32 {
    let map = CABI_NET_FETCH_RESULTS.lock();
    match map.get(&op_id) {
        Some(Some(rc)) => *rc,
        Some(None) => FS_ERR_NOT_FOUND,
        None => FS_ERR_NOT_FOUND,
    }
}

/// TRUEOS C ABI: discard async HTTPS fetch state.
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32 {
    let mut map = CABI_NET_FETCH_RESULTS.lock();
    map.remove(&op_id);

    // Best-effort: remove from any follower lists so coalescing maps don't retain dead ids.
    // (Leader tasks may still complete; they will simply skip removed result slots.)
    {
        let mut inflight = CABI_NET_FETCH_INFLIGHT.lock();
        for (_k, v) in inflight.iter_mut() {
            v.followers.retain(|&id| id != op_id);
        }
    }
    0
}

/// TRUEOS C ABI: wait for a net-fetch operation to complete.
///
/// Returns:
/// - `FS_ERR_NOT_FOUND` while pending (only when timeout_ms == 0)
/// - `FS_ERR_TIMEOUT` when deadline expires
/// - `0` on success
/// - negative error code on completion failure
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32 {
    if op_id == 0 {
        return FS_ERR_BAD_PARAM;
    }

    if timeout_ms == 0 {
        return trueos_cabi_net_fetch_result(op_id);
    }

    let start = embassy_time::Instant::now();
    let timeout = EmbassyDuration::from_millis(timeout_ms);
    loop {
        let rc = trueos_cabi_net_fetch_result(op_id);
        if rc != FS_ERR_NOT_FOUND {
            return rc;
        }

        let elapsed = embassy_time::Instant::now().saturating_duration_since(start);
        if elapsed >= timeout {
            return FS_ERR_TIMEOUT;
        }
        let remain = timeout - elapsed;
        let step = core::cmp::min(remain, EmbassyDuration::from_millis(100));
        let _ = CABI_NET_FETCH_WAIT.wait_for_event_blocking(step.as_millis() as u64);
    }
}
