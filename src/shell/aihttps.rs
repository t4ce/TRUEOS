extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use trueos_v::vnet;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent, TlsTimeouts};
use crate::v::net::dns::{self, DnsConfig};
use crate::v::net::Queue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiHttpsError {
    NoNic,
    BadUrl,
    DnsTimeout,
    DnsFailed,
    ConnectTimeout,
    TlsTimeout,
    BodyTimeout,
    ResponseTooLarge,
    Tls,
    Http(u16),
    ClosedBeforeHeaders,
}

pub trait SseHandler {
    fn on_data(&mut self, data: &str);
}

#[derive(Clone, Debug)]
struct ParsedHttpsUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_https_url(url: &str) -> Option<ParsedHttpsUrl> {
    let url = url.trim();
    let rest = url.strip_prefix("https://")?;

    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };

    let (host, port) = match hostport.rfind(':') {
        Some(colon) => {
            let h = &hostport[..colon];
            let p = &hostport[colon + 1..];
            if h.is_empty() || p.is_empty() {
                return None;
            }
            let port: u16 = p.parse().ok()?;
            (h.to_string(), port)
        }
        None => (hostport.to_string(), 443),
    };

    if host.is_empty() {
        return None;
    }

    let path = if path.is_empty() { "/" } else { path };

    Some(ParsedHttpsUrl {
        host,
        port,
        path: path.to_string(),
    })
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
        return Some(i + 4);
    }
    if let Some(i) = buf.windows(2).position(|w| w == b"\n\n") {
        return Some(i + 2);
    }
    None
}

fn parse_http_status(head: &[u8]) -> Option<u16> {
    let s = core::str::from_utf8(head).ok()?;
    let first_line = s.lines().next()?;
    let mut it = first_line.split_whitespace();
    let proto = it.next()?;
    if !proto.starts_with("HTTP/") {
        return None;
    }
    let code = it.next()?;
    code.parse::<u16>().ok()
}

fn trim_ascii_space(mut s: &[u8]) -> &[u8] {
    while !s.is_empty() && (s[0] == b' ' || s[0] == b'\t') {
        s = &s[1..];
    }
    while !s.is_empty() && (s[s.len() - 1] == b' ' || s[s.len() - 1] == b'\t') {
        s = &s[..s.len() - 1];
    }
    s
}

fn eq_ascii_case_insensitive(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

fn find_ascii_case_insensitive(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if hay.len() < needle.len() {
        return None;
    }
    for i in 0..=(hay.len() - needle.len()) {
        let mut ok = true;
        for j in 0..needle.len() {
            if hay[i + j].to_ascii_lowercase() != needle[j].to_ascii_lowercase() {
                ok = false;
                break;
            }
        }
        if ok {
            return Some(i);
        }
    }
    None
}

fn header_contains_token(headers: &[u8], name: &[u8], token: &[u8]) -> bool {
    let mut i = 0;
    while i < headers.len() {
        let line_end = headers[i..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .map(|p| i + p)
            .or_else(|| headers[i..].iter().position(|b| *b == b'\n').map(|p| i + p));
        let Some(end) = line_end else { break };
        let line = &headers[i..end];
        i = end + 1;
        if line.is_empty() {
            continue;
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let (n, v) = (&line[..colon], &line[colon + 1..]);
        if !eq_ascii_case_insensitive(n, name) {
            continue;
        }
        let v = trim_ascii_space(v);
        if find_ascii_case_insensitive(v, token).is_some() {
            return true;
        }
    }
    false
}

fn header_content_length(headers: &[u8]) -> Option<usize> {
    let name = b"content-length";
    let mut i = 0;
    while i < headers.len() {
        let line_end = headers[i..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .map(|p| i + p)
            .or_else(|| headers[i..].iter().position(|b| *b == b'\n').map(|p| i + p));
        let Some(end) = line_end else { break };
        let line = &headers[i..end];
        i = end + 1;
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let (n, v) = (&line[..colon], &line[colon + 1..]);
        if !eq_ascii_case_insensitive(n, name) {
            continue;
        }
        let v = trim_ascii_space(v);
        let s = core::str::from_utf8(v).ok()?;
        return s.trim().parse::<usize>().ok();
    }
    None
}

// Stable TLS queues for ai so we don't leak a per-request registration.
static AIHTTPS_INIT: AtomicBool = AtomicBool::new(false);
static mut AIHTTPS_CMDS: *const Queue<TlsCommand> = core::ptr::null();
static mut AIHTTPS_EVENTS: *const Queue<TlsEvent> = core::ptr::null();

fn ai_tls_queues() -> (&'static Queue<TlsCommand>, &'static Queue<TlsEvent>) {
    if !AIHTTPS_INIT.load(Ordering::Acquire) {
        // Best-effort single init.
        let owner: &'static str = "aihttps@primary";
        let cmds = Queue::new_leaked("aihttps-tls-cmd", 256);
        let events = Queue::new_leaked("aihttps-tls-evt", 4096);
        register_tls_app_queues(owner, cmds, events);
        unsafe {
            AIHTTPS_CMDS = cmds as *const _;
            AIHTTPS_EVENTS = events as *const _;
        }
        AIHTTPS_INIT.store(true, Ordering::Release);
    }
    unsafe { (&*AIHTTPS_CMDS, &*AIHTTPS_EVENTS) }
}

pub async fn post_json_async(
    url: &str,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, AiHttpsError> {
    let dev_count = crate::net::device_count();
    if dev_count == 0 {
        return Err(AiHttpsError::NoNic);
    }

    let parsed = parse_https_url(url).ok_or(AiHttpsError::BadUrl)?;

    let primary = crate::net::primary_device_index();
    let mut order: Vec<usize> = Vec::with_capacity(dev_count);
    order.push(primary);
    for i in 0..dev_count {
        if i != primary {
            order.push(i);
        }
    }

    let mut last_err = AiHttpsError::DnsFailed;
    for dev_idx in order {
        match post_on_device_json(
            &parsed, dev_idx, &body_json, auth_token, timeout_ms, max_bytes,
        )
        .await
        {
            Ok(v) => return Ok(v),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

pub async fn post_sse_async(
    url: &str,
    body_json: String,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
    handler: &mut dyn SseHandler,
) -> Result<(), AiHttpsError> {
    let dev_count = crate::net::device_count();
    if dev_count == 0 {
        return Err(AiHttpsError::NoNic);
    }

    let parsed = parse_https_url(url).ok_or(AiHttpsError::BadUrl)?;

    let primary = crate::net::primary_device_index();
    let mut order: Vec<usize> = Vec::with_capacity(dev_count);
    order.push(primary);
    for i in 0..dev_count {
        if i != primary {
            order.push(i);
        }
    }

    let mut last_err = AiHttpsError::DnsFailed;
    for dev_idx in order {
        match post_on_device_sse(
            &parsed, dev_idx, &body_json, auth_token, timeout_ms, max_bytes, handler,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

async fn post_on_device_json(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    body_json: &str,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, AiHttpsError> {
    let ip = match dns::resolve_ipv4_for_device(dev_idx, parsed.host.as_str(), DnsConfig::default())
        .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => return Err(AiHttpsError::DnsTimeout),
        Err(_) => return Err(AiHttpsError::DnsFailed),
    };

    let (cmds, events) = ai_tls_queues();
    let _ = events.drain(4096);

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

    let capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut plaintext: Vec<u8> = Vec::new();
    let mut hdr_end_cached: Option<usize> = None;
    let mut status: u16 = 0;
    let mut body_is_chunked = false;
    let mut expected_len: Option<usize> = None;

    let mut raw_body_consumed: usize = 0;
    let mut decoded: Vec<u8> = Vec::new();
    let mut chunked_done = false;

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
                        let req = format!(
                            "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS aihttps\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: application/json\r\nAccept-Encoding: identity\r\n{}Content-Length: {}\r\n\r\n{}",
                            parsed.path,
                            parsed.host,
                            auth,
                            body_json.len(),
                            body_json
                        );
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
                    if data.is_empty() {
                        continue;
                    }

                    let room = capture_cap.saturating_sub(plaintext.len());
                    if room == 0 {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(AiHttpsError::ResponseTooLarge);
                    }
                    let take = data.len().min(room);
                    plaintext.extend_from_slice(&data[..take]);
                    if take < data.len() {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(AiHttpsError::ResponseTooLarge);
                    }

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
                    if status == 0 {
                        status = parse_http_status(headers).unwrap_or(0);
                        body_is_chunked =
                            header_contains_token(headers, b"transfer-encoding", b"chunked");
                        expected_len = header_content_length(headers);
                        if status != 200 {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            return Err(AiHttpsError::Http(status));
                        }
                    }

                    let body_raw = &plaintext[hdr_end..];
                    if body_is_chunked {
                        while !chunked_done {
                            let rem = &body_raw[raw_body_consumed..];
                            let Some(line_end) = rem.windows(2).position(|w| w == b"\r\n") else {
                                break;
                            };
                            let line = &rem[..line_end];
                            let line = line.split(|b| *b == b';').next().unwrap_or(line);
                            let Ok(line_str) = core::str::from_utf8(line) else {
                                return Err(AiHttpsError::Http(0));
                            };
                            let Ok(size) = usize::from_str_radix(line_str.trim(), 16) else {
                                return Err(AiHttpsError::Http(0));
                            };
                            let after_line = raw_body_consumed + line_end + 2;
                            if size == 0 {
                                chunked_done = true;
                                break;
                            }
                            if after_line + size + 2 > body_raw.len() {
                                break;
                            }
                            let chunk = &body_raw[after_line..after_line + size];
                            decoded.extend_from_slice(chunk);
                            if decoded.len() > max_bytes {
                                return Err(AiHttpsError::ResponseTooLarge);
                            }
                            raw_body_consumed = after_line + size + 2;
                        }
                    } else {
                        if decoded.is_empty() {
                            decoded.extend_from_slice(body_raw);
                        } else {
                            let already = decoded.len();
                            if body_raw.len() > already {
                                decoded.extend_from_slice(&body_raw[already..]);
                            }
                        }
                        if decoded.len() > max_bytes {
                            return Err(AiHttpsError::ResponseTooLarge);
                        }
                    }

                    if let Some(n) = expected_len {
                        if !body_is_chunked && decoded.len() >= n {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            decoded.truncate(n);
                            return Ok(decoded);
                        }
                    }
                    if body_is_chunked && chunked_done {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Ok(decoded);
                    }
                }
                TlsEvent::Closed { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    if hdr_end_cached.is_none() {
                        return Err(AiHttpsError::ClosedBeforeHeaders);
                    }
                    return Ok(decoded);
                }
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return Err(AiHttpsError::Tls);
                }
                TlsEvent::Error { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return Err(AiHttpsError::Tls);
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
                timeouts: TlsTimeouts {
                    connect_ms: (timeout_ms / 4).max(5_000),
                    tls_ms: (timeout_ms / 4).max(5_000),
                    idle_ms: timeout_ms,
                },
            });
            sent_connect = true;
        }

        let now = Instant::now();
        if tls_handle.is_none() && now >= connect_deadline {
            return Err(AiHttpsError::ConnectTimeout);
        }
        if tls_handle.is_some() && !http_sent && now >= tls_deadline {
            return Err(AiHttpsError::TlsTimeout);
        }
        if http_sent {
            let idle_deadline = last_activity + EmbassyDuration::from_millis(timeout_ms as u64);
            if now >= idle_deadline {
                return Err(AiHttpsError::BodyTimeout);
            }
        }
        if now >= total_deadline {
            return Err(if tls_handle.is_none() {
                AiHttpsError::ConnectTimeout
            } else if !http_sent {
                AiHttpsError::TlsTimeout
            } else {
                AiHttpsError::BodyTimeout
            });
        }

        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}

async fn post_on_device_sse(
    parsed: &ParsedHttpsUrl,
    dev_idx: usize,
    body_json: &str,
    auth_token: Option<&str>,
    timeout_ms: u32,
    max_bytes: usize,
    handler: &mut dyn SseHandler,
) -> Result<(), AiHttpsError> {
    let ip = match dns::resolve_ipv4_for_device(dev_idx, parsed.host.as_str(), DnsConfig::default())
        .await
    {
        Ok(ip) => ip,
        Err(dns::DnsError::Timeout) => return Err(AiHttpsError::DnsTimeout),
        Err(_) => return Err(AiHttpsError::DnsFailed),
    };

    let (cmds, events) = ai_tls_queues();
    let _ = events.drain(4096);

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

    let capture_cap = max_bytes.saturating_add(64 * 1024);
    let mut plaintext: Vec<u8> = Vec::new();
    let mut hdr_end_cached: Option<usize> = None;
    let mut http_status: u16 = 0;
    let mut body_is_chunked = false;

    let mut raw_body_consumed: usize = 0;
    let mut decoded_body_len: usize = 0;
    let mut sse_buf: Vec<u8> = Vec::new();
    let mut chunked_done = false;
    let mut saw_terminal_event = false;

    fn responses_terminal_event(json: &str) -> bool {
        json.contains("\"type\":\"response.completed\"")
            || json.contains("\"type\":\"response.error\"")
    }

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
                        let req = format!(
                            "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS aihttps\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nAccept-Encoding: identity\r\n{}Content-Length: {}\r\n\r\n{}",
                            parsed.path,
                            parsed.host,
                            auth,
                            body_json.len(),
                            body_json
                        );
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
                    if data.is_empty() {
                        continue;
                    }

                    let room = capture_cap.saturating_sub(plaintext.len());
                    if room == 0 {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(AiHttpsError::ResponseTooLarge);
                    }
                    let take = data.len().min(room);
                    plaintext.extend_from_slice(&data[..take]);
                    if take < data.len() {
                        if let Some(h) = tls_handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return Err(AiHttpsError::ResponseTooLarge);
                    }

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
                    if http_status == 0 {
                        http_status = parse_http_status(headers).unwrap_or(0);
                        body_is_chunked =
                            header_contains_token(headers, b"transfer-encoding", b"chunked");
                        if http_status != 200 {
                            if let Some(h) = tls_handle {
                                let _ = cmds.push(TlsCommand::Close { handle: h });
                            }
                            return Err(AiHttpsError::Http(http_status));
                        }
                    }

                    let body_raw = &plaintext[hdr_end..];
                    if body_is_chunked {
                        while !chunked_done {
                            let rem = &body_raw[raw_body_consumed..];
                            let Some(line_end) = rem.windows(2).position(|w| w == b"\r\n") else {
                                break;
                            };
                            let line = &rem[..line_end];
                            let line = line.split(|b| *b == b';').next().unwrap_or(line);
                            let Ok(line_str) = core::str::from_utf8(line) else {
                                return Err(AiHttpsError::Http(0));
                            };
                            let Ok(size) = usize::from_str_radix(line_str.trim(), 16) else {
                                return Err(AiHttpsError::Http(0));
                            };
                            let after_line = raw_body_consumed + line_end + 2;
                            if size == 0 {
                                chunked_done = true;
                                break;
                            }
                            if after_line + size + 2 > body_raw.len() {
                                break;
                            }
                            let chunk = &body_raw[after_line..after_line + size];
                            decoded_body_len = decoded_body_len.saturating_add(chunk.len());
                            if decoded_body_len > max_bytes {
                                return Err(AiHttpsError::ResponseTooLarge);
                            }
                            sse_buf.extend_from_slice(chunk);
                            raw_body_consumed = after_line + size + 2;
                        }
                    } else {
                        let new = &body_raw[raw_body_consumed..];
                        if !new.is_empty() {
                            decoded_body_len = decoded_body_len.saturating_add(new.len());
                            if decoded_body_len > max_bytes {
                                return Err(AiHttpsError::ResponseTooLarge);
                            }
                            sse_buf.extend_from_slice(new);
                            raw_body_consumed = body_raw.len();
                        }
                    }

                    loop {
                        let delim = if let Some(p) = sse_buf.windows(2).position(|w| w == b"\n\n") {
                            Some((p, 2))
                        } else if let Some(p) = sse_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            Some((p, 4))
                        } else {
                            None
                        };
                        let Some((pos, dlen)) = delim else { break };

                        let mut block = sse_buf.drain(..pos + dlen).collect::<Vec<u8>>();
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
                            saw_terminal_event = true;
                            break;
                        }
                        if !data_out.is_empty() {
                            if responses_terminal_event(data_out.as_str()) {
                                saw_terminal_event = true;
                            }
                            handler.on_data(data_out.as_str());
                            if saw_terminal_event {
                                break;
                            }
                        }
                    }

                    if saw_terminal_event {
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
                    if hdr_end_cached.is_none() {
                        return Err(AiHttpsError::ClosedBeforeHeaders);
                    }
                    if saw_terminal_event {
                        return Ok(());
                    }
                    return Err(AiHttpsError::BodyTimeout);
                }
                TlsEvent::TlsError { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return Err(AiHttpsError::Tls);
                }
                TlsEvent::Error { .. } => {
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return Err(AiHttpsError::Tls);
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
                timeouts: TlsTimeouts {
                    connect_ms: (timeout_ms / 4).max(5_000),
                    tls_ms: (timeout_ms / 4).max(5_000),
                    idle_ms: timeout_ms,
                },
            });
            sent_connect = true;
        }

        let now = Instant::now();
        if tls_handle.is_none() && now >= connect_deadline {
            return Err(AiHttpsError::ConnectTimeout);
        }
        if tls_handle.is_some() && !http_sent && now >= tls_deadline {
            return Err(AiHttpsError::TlsTimeout);
        }
        if http_sent {
            let idle_deadline = last_activity + EmbassyDuration::from_millis(timeout_ms as u64);
            if now >= idle_deadline {
                return Err(AiHttpsError::BodyTimeout);
            }
        }
        if now >= total_deadline {
            return Err(if tls_handle.is_none() {
                AiHttpsError::ConnectTimeout
            } else if !http_sent {
                AiHttpsError::TlsTimeout
            } else {
                AiHttpsError::BodyTimeout
            });
        }

        Timer::after(EmbassyDuration::from_millis(2)).await;
    }
}
