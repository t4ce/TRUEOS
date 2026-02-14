extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use trueos_v::vnet as vnet;

use super::dns::{self, DnsConfig};
use super::Queue;
use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
use crate::surface::io::cabi::{
    FS_ERR_BAD_PARAM, FS_ERR_BAD_PATH, FS_ERR_IO, FS_ERR_NOT_FOUND, FS_ERR_TIMEOUT,
    FS_ERR_NO_SPACE, FS_ERR_TOO_LARGE, FS_ERR_USBMS_NOT_FOUND, NET_ERR_BAD_URL, NET_ERR_HTTP,
    NET_ERR_TIMEOUT, NET_ERR_TIMEOUT_BODY, NET_ERR_TIMEOUT_CONNECT, NET_ERR_TIMEOUT_DNS,
    NET_ERR_TIMEOUT_TLS, NET_ERR_TLS,
};
use spin::Mutex;
use crate::wait::WaitQueue;

static CABI_NET_FETCH_SEQ: AtomicU32 = AtomicU32::new(1);
static CABI_NET_FETCH_RESULTS: Mutex<BTreeMap<u32, Option<i32>>> = Mutex::new(BTreeMap::new());
static CABI_NET_FETCH_WAIT: WaitQueue = WaitQueue::new();

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
        FetchError::Http(_) => NET_ERR_HTTP,
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
        return if allow_empty { Ok(out) } else { Err(FS_ERR_BAD_PATH) };
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
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
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
        if !k.iter()
            .zip(name.iter())
            .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
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
            crate::log!("{} host={} status={} body=chunked\n", prefix, host, head.status);
        }
    }
}

async fn write_body_to_tmp_file(
    disk: crate::disc::block::DeviceHandle,
    tmp_path: &str,
    body: &[u8],
) -> Result<(), i32> {
    let Some(sh) = crate::v::fs::trueosfs::file_write_begin_async(disk, tmp_path, body.len() as u64)
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

async fn fetch_on_device(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    let ip = match dns::resolve_ipv4_for_device(dev_idx, parsed.host.as_str(), DnsConfig::default()).await {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => return Err(FetchError::DnsTimeout),
        Err(_) => return Err(FetchError::DnsFailed),
    };

    crate::log!(
        "vhttps: host={} dev={} ip={}.{}.{}.{}\n",
        parsed.host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );

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

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);

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

    loop {
        for ev in events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    tls_handle = Some(handle);
                }
                TlsEvent::Connected { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !http_sent {
                        let req = format!(
                            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS vhttps\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                            parsed.path,
                            parsed.host
                        );
                        let _ = cmds.push(TlsCommand::Send {
                            handle,
                            data: req.into_bytes(),
                        });
                        http_sent = true;
                    }
                }
                TlsEvent::Data { handle, data } => {
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
                        if hdr_end != 0 {
                            let headers = &plaintext[..hdr_end];
                            let body = &plaintext[hdr_end..];

                            let status = parse_http_status(&plaintext).unwrap_or(0);
                            if status != 200 {
                                if is_redirect_status(status) {
                                    if let Some(next) = redirect_url_from_location(parsed, headers) {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        return Err(FetchError::Redirect { status, url: next });
                                    }
                                }
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                return Err(FetchError::Http(status));
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
                                    return Ok(decoded);
                                }
                            } else if let Some(len) = header_parse_content_length(headers) {
                                if body.len() >= len {
                                    let  out = body[..len].to_vec();
                                    if out.len() > max_bytes {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        return Err(FetchError::ResponseTooLarge);
                                    }
                                    if let Some(h) = tls_handle {
                                        let _ = cmds.push(TlsCommand::Close { handle: h });
                                    }
                                    return Ok(out);
                                }
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
                        if is_redirect_status(status) {
                            if let Some(next) = redirect_url_from_location(parsed, headers) {
                                return Err(FetchError::Redirect { status, url: next });
                            }
                        }
                        return Err(FetchError::Http(status));
                    }

                    let is_chunked = header_contains_token(headers, b"transfer-encoding", b"chunked");
                    let  decoded_body = if is_chunked {
                        decode_http_chunked(body).unwrap_or_else(|| body.to_vec())
                    } else if let Some(len) = header_parse_content_length(headers) {
                        body.get(..len).unwrap_or(body).to_vec()
                    } else {
                        body.to_vec()
                    };

                    if decoded_body.len() > max_bytes {
                        return Err(FetchError::ResponseTooLarge);
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
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: vnet::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
                server_name,
                cfg: cfg.clone(),
                roots: roots.clone(),
            });
            sent_connect = true;
        }

        if Instant::now() >= deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            // Best-effort phase classification: we have a single overall deadline.
            // - No handle => TCP connect never opened
            // - Handle but no HTTP sent => TLS handshake didn't complete
            // - HTTP sent => body stalled
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

#[derive(Debug)]
enum FetchToFileError {
    Code(i32),
    Redirect { status: u16, url: String },
}

async fn fetch_on_device_to_file(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    timeout_ms: u32,
    max_bytes: usize,
    disk: crate::disc::block::DeviceHandle,
    tmp_path: &str,
) -> Result<(), FetchToFileError> {
    let ip = match dns::resolve_ipv4_for_device(dev_idx, parsed.host.as_str(), DnsConfig::default()).await {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::DnsTimeout))),
        Err(_) => return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::DnsFailed))),
    };

    crate::log!(
        "vhttps: host={} dev={} ip={}.{}.{}.{}\n",
        parsed.host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );

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

    let mut header_buf: Vec<u8> = Vec::new();
    let mut header_done = false;
    let mut body_is_chunked = false;
    let mut chunked_raw_body: Vec<u8> = Vec::new();
    let chunked_capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut body_expected = 0usize;
    let mut body_written = 0usize;
    let mut stream_handle: Option<u32> = None;

    loop {
        for ev in events.drain(1024) {
            match ev {
                TlsEvent::Opened { handle } => {
                    tls_handle = Some(handle);
                }
                TlsEvent::Connected { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if !http_sent {
                        let req = format!(
                            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS vhttps\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                            parsed.path,
                            parsed.host
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
                            return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::Http(0))));
                        }

                        if let Some(hdr_end) = find_http_header_end(&header_buf) {
                            let headers = &header_buf[..hdr_end];
                            let status = parse_http_status(headers).unwrap_or(0);
                            if status != 200 {
                                if is_redirect_status(status) {
                                    if let Some(next) = redirect_url_from_location(parsed, headers) {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        if let Some(sh) = stream_handle.take() {
                                            let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                                        }
                                        return Err(FetchToFileError::Redirect { status, url: next });
                                    }
                                }

                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                if let Some(sh) = stream_handle.take() {
                                    let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::Http(status))));
                            }

                            let Some(head) = parse_http_head(headers) else {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                if let Some(sh) = stream_handle.take() {
                                    let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                crate::log!(
                                    "vhttps-file: invalid-http-head host={} hdr_bytes={}\n",
                                    parsed.host,
                                    header_buf.len()
                                );
                                return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::Http(0))));
                            };
                            log_http_head("vhttps-file: head", parsed.host.as_str(), head);

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
                                    let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                                }
                                return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::ResponseTooLarge)));
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
                                    None => return Err(FetchToFileError::Code(FS_ERR_NO_SPACE)),
                                };
                                stream_handle = Some(sh);
                            }
                            header_done = true;

                            let body_start = hdr_end;
                            if header_buf.len() > body_start {
                                let part = &header_buf[body_start..];
                                if body_is_chunked {
                                    let room = chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                                    if room == 0 {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::ResponseTooLarge)));
                                    }
                                    let take = part.len().min(room);
                                    chunked_raw_body.extend_from_slice(&part[..take]);
                                    if take < part.len() {
                                        if let Some(h) = tls_handle {
                                            let _ = cmds.push(TlsCommand::Close { handle: h });
                                        }
                                        return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::ResponseTooLarge)));
                                    }
                                } else if let Some(sh) = stream_handle {
                                    let rem = body_expected.saturating_sub(body_written);
                                    let take = part.len().min(rem);
                                    if take > 0 {
                                        crate::v::fs::trueosfs::file_write_chunk_async(sh, &part[..take])
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
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                return Ok(());
                            }
                        }
                    } else if body_is_chunked {
                        let room = chunked_capture_cap.saturating_sub(chunked_raw_body.len());
                        if room == 0 {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::ResponseTooLarge)));
                        }
                        let take = data.len().min(room);
                        chunked_raw_body.extend_from_slice(&data[..take]);
                        if take < data.len() {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::ResponseTooLarge)));
                        }
                        if let Some(decoded) = decode_http_chunked(chunked_raw_body.as_slice()) {
                            if decoded.len() > max_bytes {
                                if let Some(h) = tls_handle {
                                    let _ = cmds.push(TlsCommand::Close { handle: h });
                                }
                                return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::ResponseTooLarge)));
                            }
                            write_body_to_tmp_file(disk, tmp_path, decoded.as_slice())
                                .await
                                .map_err(FetchToFileError::Code)?;
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
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
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
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
                            return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::Http(0))));
                        };
                        if decoded.len() > max_bytes {
                            if let Some(sh) = stream_handle.take() {
                                let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                            }
                            return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::ResponseTooLarge)));
                        }
                        if let Some(sh) = stream_handle.take() {
                            let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                        }
                        write_body_to_tmp_file(disk, tmp_path, decoded.as_slice())
                            .await
                            .map_err(FetchToFileError::Code)?;
                        return Ok(());
                    }
                    if let Some(sh) = stream_handle.take() {
                        let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                    }
                    return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::BodyTimeout)));
                }
                TlsEvent::Error { .. } => {}
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    if let Some(sh) = stream_handle.take() {
                        let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
                    }
                    return Err(FetchToFileError::Code(fetch_error_to_code(FetchError::Tls)));
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
            });
            sent_connect = true;
        }

        if Instant::now() >= deadline {
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            if let Some(sh) = stream_handle.take() {
                let _ = crate::v::fs::trueosfs::file_write_abort_async(sh).await;
            }
            return Err(FetchToFileError::Code(if tls_handle.is_none() {
                fetch_error_to_code(FetchError::ConnectTimeout)
            } else if !http_sent {
                fetch_error_to_code(FetchError::TlsTimeout)
            } else {
                fetch_error_to_code(FetchError::BodyTimeout)
            }));
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
            match fetch_on_device(&parsed, dev_idx, timeout_ms, max_bytes).await {
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
    let Some(disk) = crate::v::fs::trueosfs::primary_root_handle() else {
        return Err(FS_ERR_USBMS_NOT_FOUND);
    };

    let key = normalize_rel(path, false)?;

    match crate::v::fs::trueosfs::file_exists_async(disk, key.as_str()).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => return Err(block_error_to_code(e)),
    }

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
            match fetch_on_device_to_file(
                &parsed,
                dev_idx,
                timeout_ms,
                max_bytes,
                disk,
                tmp.as_str(),
            )
            .await
            {
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

    match crate::v::fs::trueosfs::file_rename_async(disk, tmp.as_str(), key.as_str()).await {
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
#[no_mangle]
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
    const TIMEOUT_MS: u32 = 2_500;
    const MAX_BYTES: usize = 4 * 1024 * 1024;

    let url = String::from(url_s);
    let path = String::from(path_s);
    let op_id = CABI_NET_FETCH_SEQ.fetch_add(1, Ordering::Relaxed);
    CABI_NET_FETCH_RESULTS.lock().insert(op_id, None);
    crate::wait::spawn_local_detached(async move {
        let rc = match fetch_https_to_file_async(url.as_str(), path.as_str(), TIMEOUT_MS, MAX_BYTES).await {
            Ok(()) => 0,
            Err(code) => code,
        };
        let mut map = CABI_NET_FETCH_RESULTS.lock();
        if let Some(slot) = map.get_mut(&op_id) {
            *slot = Some(rc);
        }
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
#[no_mangle]
pub extern "C" fn trueos_cabi_net_fetch_result(op_id: u32) -> i32 {
    let map = CABI_NET_FETCH_RESULTS.lock();
    match map.get(&op_id) {
        Some(Some(rc)) => *rc,
        Some(None) => FS_ERR_NOT_FOUND,
        None => FS_ERR_NOT_FOUND,
    }
}

/// TRUEOS C ABI: discard async HTTPS fetch state.
#[no_mangle]
pub extern "C" fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32 {
    let mut map = CABI_NET_FETCH_RESULTS.lock();
    map.remove(&op_id);
    0
}

/// TRUEOS C ABI: wait for a net-fetch operation to complete.
///
/// Returns:
/// - `FS_ERR_NOT_FOUND` while pending (only when timeout_ms == 0)
/// - `FS_ERR_TIMEOUT` when deadline expires
/// - `0` on success
/// - negative error code on completion failure
#[no_mangle]
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
