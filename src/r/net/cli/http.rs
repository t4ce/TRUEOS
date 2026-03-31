extern crate alloc;

use crate::r::net::NetProfile;
use crate::r::net::VNet;
use crate::r::net::dns::{self, DnsConfig};
use alloc::string::String;
use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;
use v::vnet as api;

fn parse_ipv4_literal(host: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = host.split('.');
    for octet in &mut out {
        let part = parts.next()?;
        *octet = part.parse::<u8>().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

#[derive(Clone, Debug)]
pub struct ParsedHttpUrl {
    pub host: HString<96>,
    pub port: u16,
    pub path: HString<160>,
}

#[derive(Clone, Debug)]
pub enum HttpFetchError {
    BadUrl,
    TimedOut,
    DnsFailed,
    HttpStatus(u16),
    Redirect(String),
    ResponseTooLarge,
}

pub fn parse_http_url(url: &str) -> Result<ParsedHttpUrl, &'static str> {
    let mut u = url.trim();
    if u.is_empty() {
        return Err("empty url");
    }
    if let Some(rest) = u.strip_prefix("http://") {
        u = rest;
    } else if u.strip_prefix("https://").is_some() {
        return Err("https:// not supported here");
    }

    let (hostport, path) = match u.split_once('/') {
        Some((a, b)) => (a, b),
        None => (u, ""),
    };

    let (host_str, port) = match hostport.split_once(':') {
        Some((h, p)) => {
            let p = p.trim();
            if p.is_empty() {
                return Err("empty port");
            }
            let port = p.parse::<u16>().map_err(|_| "bad port")?;
            (h, port)
        }
        None => (hostport, 80u16),
    };

    let host_str = host_str.trim();
    if host_str.is_empty() {
        return Err("empty host");
    }

    let mut host: HString<96> = HString::new();
    for ch in host_str.chars() {
        if host.push(ch).is_err() {
            break;
        }
    }

    let mut out_path: HString<160> = HString::new();
    let _ = out_path.push('/');
    if !path.is_empty() {
        for ch in path.chars() {
            if out_path.push(ch).is_err() {
                break;
            }
        }
    }

    Ok(ParsedHttpUrl {
        host,
        port,
        path: out_path,
    })
}

pub(super) fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

pub(super) fn parse_http_status(buf: &[u8]) -> Option<u16> {
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

pub(super) fn header_get_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
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

pub(super) fn header_contains_token(headers: &[u8], name: &[u8], token: &[u8]) -> bool {
    let Some(v) = header_get_value(headers, name) else {
        return false;
    };
    header_value_contains_token(v, token)
}

pub(super) fn header_parse_content_length(headers: &[u8]) -> Option<usize> {
    let v = header_get_value(headers, b"content-length")?;
    let v = core::str::from_utf8(v).ok()?;
    v.trim().parse::<usize>().ok()
}

pub(super) fn decode_http_chunked(body: &[u8]) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    loop {
        let line_end = body[i..].windows(2).position(|w| w == b"\r\n")?;
        let line = &body[i..i + line_end];
        i += line_end + 2;
        let line = line.split(|b| *b == b';').next().unwrap_or(line);
        let line_str = core::str::from_utf8(line).ok()?;
        let size = usize::from_str_radix(line_str.trim(), 16).ok()?;
        if size == 0 {
            return Some(out);
        }
        if i + size > body.len() {
            return None;
        }
        out.extend_from_slice(&body[i..i + size]);
        i += size;
        if i + 2 > body.len() || &body[i..i + 2] != b"\r\n" {
            return None;
        }
        i += 2;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HttpBodyKind {
    ContentLength(usize),
    Chunked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HttpHead {
    pub(super) status: u16,
    pub(super) body: HttpBodyKind,
}

pub(super) fn parse_http_head(headers: &[u8]) -> Option<HttpHead> {
    let status = parse_http_status(headers)?;
    if header_contains_token(headers, b"transfer-encoding", b"chunked") {
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

fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn redirect_url_from_location(current: &ParsedHttpUrl, headers: &[u8]) -> Option<String> {
    let loc = header_get_value(headers, b"location")?;
    let loc = core::str::from_utf8(loc).ok()?.trim();
    if loc.is_empty() {
        return None;
    }
    if loc.starts_with("http://") || loc.starts_with("https://") {
        return Some(String::from(loc));
    }
    if loc.starts_with('/') {
        if current.port == 80 {
            return Some(alloc::format!("http://{}{}", current.host, loc));
        }
        return Some(alloc::format!("http://{}:{}{}", current.host, current.port, loc));
    }
    None
}

pub async fn fetch_http_body(
    url: &str,
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    let parsed = parse_http_url(url).map_err(|_| HttpFetchError::BadUrl)?;

    let _ = crate::r::readiness::wait_for_timeout(
        crate::r::readiness::NET_CONFIGURED,
        EmbassyDuration::from_secs(3),
    )
    .await;

    let profile = NetProfile::default();
    let ip = if let Some(ip) = parse_ipv4_literal(parsed.host.as_str()) {
        ip
    } else {
        let Ok(ip) = dns::resolve_ipv4_with_profile(
            parsed.host.as_str(),
            profile,
            DnsConfig::for_profile(profile),
        )
        .await
        else {
            return Err(HttpFetchError::DnsFailed);
        };
        ip
    };

    let net = loop {
        if let Some(v) = VNet::open_with_profile(profile) {
            break v;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    };

    let mut open_sent = false;
    for _ in 0..64 {
        if net
            .submit(api::Command::OpenTcpConnect {
                remote: api::EndpointV4 {
                    addr: ip,
                    port: parsed.port,
                },
            })
            .is_ok()
        {
            open_sent = true;
            break;
        }
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
    if !open_sent {
        crate::log!("http: open failed host={} port={}\n", parsed.host, parsed.port);
        return Err(HttpFetchError::TimedOut);
    }

    let mut tcp_handle: Option<api::NetHandle> = None;
    let mut sent_get = false;
    let mut rx: Vec<u8> = Vec::new();
    let mut truncated = false;
    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);

    loop {
        for _ in 0..32 {
            let Some(ev) = net.pop_event() else { break };
            match ev {
                api::Event::Opened { handle, kind } => {
                    if matches!(kind, api::SocketKind::Tcp) {
                        tcp_handle = Some(handle);
                    }
                }
                api::Event::TcpEstablished { handle } => {
                    if tcp_handle.is_none() {
                        tcp_handle = Some(handle);
                    }
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    if !sent_get {
                        let mut req: Vec<u8> = Vec::new();
                        req.extend_from_slice(b"GET ");
                        req.extend_from_slice(parsed.path.as_str().as_bytes());
                        req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
                        req.extend_from_slice(parsed.host.as_str().as_bytes());
                        req.extend_from_slice(
                            b"\r\nUser-Agent: TRUEOS\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                        );
                        if let Some(h) = tcp_handle {
                            let mut send_ok = false;
                            for _ in 0..64 {
                                if net
                                    .submit(api::Command::SendTcp {
                                        handle: h,
                                        data: api::ByteBuf::from_slice_trunc(req.as_slice()),
                                    })
                                    .is_ok()
                                {
                                    send_ok = true;
                                    break;
                                }
                                Timer::after(EmbassyDuration::from_millis(1)).await;
                            }
                            if send_ok {
                                sent_get = true;
                            } else {
                                crate::log!(
                                    "http: get submit failed host={} handle={}\n",
                                    parsed.host,
                                    h.0
                                );
                            }
                        }
                    }
                }
                api::Event::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    let data = data.as_slice();
                    if rx.len() < max_rx {
                        let room = max_rx - rx.len();
                        let take = data.len().min(room);
                        rx.extend_from_slice(&data[..take]);
                        if take < data.len() {
                            truncated = true;
                        }
                    } else {
                        truncated = true;
                    }
                }
                api::Event::Closed { handle } => {
                    if tcp_handle == Some(handle) {
                        let hdr_end = find_http_header_end(&rx);
                        let status = parse_http_status(&rx).unwrap_or(0);
                        if is_redirect_status(status) {
                            if let Some(end) = hdr_end {
                                if let Some(next) = redirect_url_from_location(&parsed, &rx[..end])
                                {
                                    return Err(HttpFetchError::Redirect(next));
                                }
                            }
                        }
                        if status >= 400 {
                            return Err(HttpFetchError::HttpStatus(status));
                        }
                        let body_off = hdr_end.unwrap_or(0);
                        let body = if body_off <= rx.len() {
                            rx.split_off(body_off)
                        } else {
                            Vec::new()
                        };
                        if truncated {
                            return Err(HttpFetchError::ResponseTooLarge);
                        }
                        return Ok(body);
                    }
                }
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            if let Some(h) = tcp_handle {
                let _ = net.submit(api::Command::Close { handle: h });
            }
            return Err(HttpFetchError::TimedOut);
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}
