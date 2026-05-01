extern crate alloc;
extern crate std;

use crate::r::net::NetProfile;
use crate::r::net::VNet;
use crate::t::net::dns::{self, DnsConfig};
use crate::t::net::hyper_io::{HyperEmptyBody, HyperTokioIo};
use alloc::string::String;
use alloc::vec::Vec;
use core::pin::Pin;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;
use hyper::body::Body;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use v::vnet as api;

pub(super) fn parse_ipv4_literal(host: &str) -> Option<[u8; 4]> {
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
    NoSpace,
    Truncated,
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
    UntilClose,
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
    if let Some(len) = header_parse_content_length(headers) {
        return Some(HttpHead {
            status,
            body: HttpBodyKind::ContentLength(len),
        });
    }
    Some(HttpHead {
        status,
        body: HttpBodyKind::UntilClose,
    })
}

pub(super) fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

pub(super) fn redirect_url_from_location(
    current: &ParsedHttpUrl,
    headers: &[u8],
) -> Option<String> {
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

async fn request_http_body(
    method: &[u8],
    url: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    let parsed = parse_http_url(url).map_err(|_| HttpFetchError::BadUrl)?;

    let _ = crate::r::readiness::wait_for_timeout(
        crate::r::readiness::NET_ANY_CONFIGURED,
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
            if crate::logflag::VHTTPS_VERBOSE {
                crate::log!(
                    "http: open-submitted host={} ip={}.{}.{}.{} port={}\n",
                    parsed.host,
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3],
                    parsed.port,
                );
            }
            break;
        }
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
    if !open_sent {
        crate::log!("http: open failed host={} port={}\n", parsed.host, parsed.port);
        return Err(HttpFetchError::TimedOut);
    }

    let mut tcp_handle: Option<api::NetHandle> = None;
    let mut saw_opened = false;
    let mut saw_established = false;
    let mut sent_request = false;
    let mut send_submit_failures = 0u32;
    let mut last_error: Option<&'static str> = None;
    let mut rx: Vec<u8> = Vec::new();
    let mut truncated = false;
    let timeout_window = EmbassyDuration::from_millis(timeout_ms as u64);
    let mut last_progress = Instant::now();

    async fn send_tcp_all(net: &VNet, handle: api::NetHandle, data: &[u8]) -> Result<(), ()> {
        for chunk in data.chunks(api::MAX_MSG) {
            let mut sent = false;
            for _ in 0..64 {
                if net
                    .submit(api::Command::SendTcp {
                        handle,
                        data: api::ByteBuf::from_slice_trunc(chunk),
                    })
                    .is_ok()
                {
                    sent = true;
                    break;
                }
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            if !sent {
                return Err(());
            }
        }
        Ok(())
    }

    loop {
        for _ in 0..256 {
            let Some(ev) = net.pop_event() else { break };
            match ev {
                api::Event::Opened { handle, kind } => {
                    if matches!(kind, api::SocketKind::Tcp) {
                        tcp_handle = Some(handle);
                        saw_opened = true;
                        last_progress = Instant::now();
                        if crate::logflag::VHTTPS_VERBOSE {
                            crate::log!(
                                "http: opened host={} port={} handle={}\n",
                                parsed.host,
                                parsed.port,
                                handle.0,
                            );
                        }
                    }
                }
                api::Event::TcpEstablished { handle } => {
                    if tcp_handle.is_none() {
                        tcp_handle = Some(handle);
                    }
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    saw_established = true;
                    if crate::logflag::VHTTPS_VERBOSE {
                        crate::log!(
                            "http: established host={} port={} handle={}\n",
                            parsed.host,
                            parsed.port,
                            handle.0,
                        );
                    }
                    if !sent_request {
                        let mut req: Vec<u8> = Vec::new();
                        req.extend_from_slice(method);
                        req.extend_from_slice(b" ");
                        req.extend_from_slice(parsed.path.as_str().as_bytes());
                        req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
                        req.extend_from_slice(parsed.host.as_str().as_bytes());
                        req.extend_from_slice(
                            b"\r\nUser-Agent: TRUEOS\r\nAccept: */*\r\nConnection: close\r\n",
                        );
                        if !body.is_empty() {
                            req.extend_from_slice(b"Content-Length: ");
                            req.extend_from_slice(alloc::format!("{}", body.len()).as_bytes());
                            req.extend_from_slice(b"\r\n");
                        }
                        for (name, value) in extra_headers.iter().copied() {
                            req.extend_from_slice(name.as_bytes());
                            req.extend_from_slice(b": ");
                            req.extend_from_slice(value.as_bytes());
                            req.extend_from_slice(b"\r\n");
                        }
                        req.extend_from_slice(b"\r\n");
                        req.extend_from_slice(body);

                        if let Some(h) = tcp_handle {
                            if send_tcp_all(&net, h, req.as_slice()).await.is_ok() {
                                sent_request = true;
                                last_progress = Instant::now();
                                if crate::logflag::VHTTPS_VERBOSE {
                                    crate::log!(
                                        "http: request-sent host={} ip={}.{}.{}.{} port={} handle={} method={} path={} req_len={} body_len={} extra_headers={}\n",
                                        parsed.host,
                                        ip[0],
                                        ip[1],
                                        ip[2],
                                        ip[3],
                                        parsed.port,
                                        h.0,
                                        if method == b"POST" { "POST" } else { "GET" },
                                        parsed.path,
                                        req.len(),
                                        body.len(),
                                        extra_headers.len(),
                                    );
                                }
                            } else {
                                send_submit_failures = send_submit_failures.saturating_add(1);
                                last_error = Some("request submit failed");
                                crate::log!(
                                    "http: request submit failed host={} handle={}\n",
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
                    if !data.is_empty() {
                        if crate::logflag::VHTTPS_VERBOSE && rx.is_empty() {
                            crate::log!(
                                "http: first-data host={} port={} handle={} bytes={}\n",
                                parsed.host,
                                parsed.port,
                                handle.0,
                                data.len(),
                            );
                        }
                        last_progress = Instant::now();
                    }
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

                    if let Some(hdr_end) = find_http_header_end(&rx) {
                        let headers = &rx[..hdr_end];
                        let status = parse_http_status(headers).unwrap_or(0);
                        if crate::logflag::VHTTPS_VERBOSE {
                            let content_length = header_parse_content_length(headers).unwrap_or(0);
                            let chunked =
                                header_contains_token(headers, b"transfer-encoding", b"chunked");
                            crate::log!(
                                "http: headers host={} port={} handle={} status={} hdr_bytes={} body_bytes={} chunked={} content_length={}\n",
                                parsed.host,
                                parsed.port,
                                handle.0,
                                status,
                                hdr_end,
                                rx.len().saturating_sub(hdr_end),
                                chunked as u8,
                                content_length,
                            );
                        }
                        if is_redirect_status(status) {
                            if let Some(next) = redirect_url_from_location(&parsed, headers) {
                                if let Some(h) = tcp_handle {
                                    let _ = net.submit(api::Command::Close { handle: h });
                                }
                                return Err(HttpFetchError::Redirect(next));
                            }
                        }
                        if status >= 400 {
                            if let Some(h) = tcp_handle {
                                let _ = net.submit(api::Command::Close { handle: h });
                            }
                            return Err(HttpFetchError::HttpStatus(status));
                        }
                        if let Some(head) = parse_http_head(headers) {
                            match head.body {
                                HttpBodyKind::ContentLength(len) => {
                                    let body_len = rx.len().saturating_sub(hdr_end);
                                    if body_len >= len {
                                        if let Some(h) = tcp_handle {
                                            let _ = net.submit(api::Command::Close { handle: h });
                                        }
                                        if truncated {
                                            return Err(HttpFetchError::ResponseTooLarge);
                                        }
                                        return Ok(rx[hdr_end..hdr_end + len].to_vec());
                                    }
                                }
                                HttpBodyKind::Chunked => {
                                    if let Some(body) = decode_http_chunked(&rx[hdr_end..]) {
                                        if let Some(h) = tcp_handle {
                                            let _ = net.submit(api::Command::Close { handle: h });
                                        }
                                        if truncated {
                                            return Err(HttpFetchError::ResponseTooLarge);
                                        }
                                        return Ok(body);
                                    }
                                }
                                HttpBodyKind::UntilClose => {}
                            }
                        }
                    }
                }
                api::Event::Closed { handle } => {
                    if tcp_handle == Some(handle) {
                        last_progress = Instant::now();
                        if crate::logflag::VHTTPS_VERBOSE {
                            crate::log!(
                                "http: closed host={} port={} handle={} rx_bytes={} hdr_end={}\n",
                                parsed.host,
                                parsed.port,
                                handle.0,
                                rx.len(),
                                find_http_header_end(&rx).is_some() as u8,
                            );
                        }
                        let Some(hdr_end) = find_http_header_end(&rx) else {
                            crate::log!(
                                "http: closed before complete headers host={} port={} rx_bytes={} last_error={}\n",
                                parsed.host,
                                parsed.port,
                                rx.len(),
                                last_error.unwrap_or("none"),
                            );
                            return Err(HttpFetchError::HttpStatus(0));
                        };
                        let Some(status) = parse_http_status(&rx) else {
                            crate::log!(
                                "http: closed with invalid status line host={} port={} rx_bytes={} last_error={}\n",
                                parsed.host,
                                parsed.port,
                                rx.len(),
                                last_error.unwrap_or("none"),
                            );
                            return Err(HttpFetchError::HttpStatus(0));
                        };
                        if is_redirect_status(status) {
                            if let Some(next) = redirect_url_from_location(&parsed, &rx[..hdr_end])
                            {
                                return Err(HttpFetchError::Redirect(next));
                            }
                        }
                        if status >= 400 {
                            return Err(HttpFetchError::HttpStatus(status));
                        }
                        let body = rx.split_off(hdr_end);
                        if truncated {
                            return Err(HttpFetchError::ResponseTooLarge);
                        }
                        return Ok(body);
                    }
                }
                api::Event::Error { msg } => {
                    last_error = Some(msg);
                    if crate::logflag::VHTTPS_VERBOSE {
                        crate::log!(
                            "http: event-error host={} port={} msg={}\n",
                            parsed.host,
                            parsed.port,
                            msg,
                        );
                    }
                }
                _ => {}
            }
        }

        if Instant::now().saturating_duration_since(last_progress) >= timeout_window {
            if let Some(h) = tcp_handle {
                let _ = net.submit(api::Command::Close { handle: h });
            }
            let phase = if !saw_opened {
                "waiting-open"
            } else if !saw_established {
                "waiting-establish"
            } else if !sent_request {
                "waiting-send-submit"
            } else if rx.is_empty() {
                "waiting-response"
            } else {
                "receiving-response"
            };
            crate::log!(
                "http: timeout host={} ip={}.{}.{}.{} port={} handle={} phase={} sent_request={} rx_bytes={} hdr_end={} idle_ms={} send_submit_failures={} last_error={}\n",
                parsed.host,
                ip[0],
                ip[1],
                ip[2],
                ip[3],
                parsed.port,
                tcp_handle.map(|h| h.0).unwrap_or(0),
                phase,
                sent_request as u8,
                rx.len(),
                find_http_header_end(&rx).is_some() as u8,
                timeout_ms,
                send_submit_failures,
                last_error.unwrap_or("none"),
            );
            return Err(HttpFetchError::TimedOut);
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

pub async fn fetch_http_body(
    url: &str,
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    request_http_body(b"GET", url, &[], &[], timeout_ms, max_rx).await
}

async fn send_tcp_all_hyper_bridge(
    net: &VNet,
    handle: api::NetHandle,
    data: &[u8],
) -> Result<(), HttpFetchError> {
    for chunk in data.chunks(api::MAX_MSG) {
        let mut sent = false;
        for _ in 0..64 {
            if net
                .submit(api::Command::SendTcp {
                    handle,
                    data: api::ByteBuf::from_slice_trunc(chunk),
                })
                .is_ok()
            {
                sent = true;
                break;
            }
            tokio::time::sleep(core::time::Duration::from_millis(1)).await;
        }
        if !sent {
            return Err(HttpFetchError::TimedOut);
        }
    }
    Ok(())
}

async fn tcp_duplex_bridge(
    net: VNet,
    handle: api::NetHandle,
    mut io: DuplexStream,
) -> Result<(), HttpFetchError> {
    let mut outbound = [0u8; 4096];
    loop {
        tokio::select! {
            read = io.read(&mut outbound) => {
                let n = read.map_err(|_| HttpFetchError::TimedOut)?;
                if n == 0 {
                    break;
                }
                send_tcp_all_hyper_bridge(&net, handle, &outbound[..n]).await?;
            }
            _ = tokio::time::sleep(core::time::Duration::from_millis(1)) => {
                for _ in 0..256 {
                    let Some(ev) = net.pop_event() else { break };
                    match ev {
                        api::Event::TcpData { handle: h, data } if h == handle => {
                            io.write_all(data.as_slice())
                                .await
                                .map_err(|_| HttpFetchError::TimedOut)?;
                        }
                        api::Event::Closed { handle: h } if h == handle => {
                            let _ = io.shutdown().await;
                            return Ok(());
                        }
                        api::Event::Error { .. } => {
                            let _ = io.shutdown().await;
                            return Err(HttpFetchError::TimedOut);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = net.submit(api::Command::Close { handle });
    let _ = io.shutdown().await;
    Ok(())
}

async fn connect_hyper_tcp_stream(
    parsed: &ParsedHttpUrl,
    timeout_ms: u32,
) -> Result<DuplexStream, HttpFetchError> {
    let _ = crate::r::readiness::wait_for_timeout(
        crate::r::readiness::NET_ANY_CONFIGURED,
        EmbassyDuration::from_secs(3),
    )
    .await;

    let profile = NetProfile::default();
    let ip = if let Some(ip) = parse_ipv4_literal(parsed.host.as_str()) {
        ip
    } else {
        dns::resolve_ipv4_with_profile(
            parsed.host.as_str(),
            profile,
            DnsConfig::for_profile(profile),
        )
        .await
        .map_err(|_| HttpFetchError::DnsFailed)?
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
        crate::log!("http-hyper: open failed host={} port={}\n", parsed.host, parsed.port);
        return Err(HttpFetchError::TimedOut);
    }

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);
    let mut opened = None;
    let handle = 'connect_wait: loop {
        for _ in 0..256 {
            let Some(ev) = net.pop_event() else { break };
            match ev {
                api::Event::Opened { handle, kind } if matches!(kind, api::SocketKind::Tcp) => {
                    opened = Some(handle);
                    if crate::logflag::VHTTPS_VERBOSE {
                        crate::log!(
                            "http-hyper: opened host={} ip={}.{}.{}.{} port={} handle={}\n",
                            parsed.host,
                            ip[0],
                            ip[1],
                            ip[2],
                            ip[3],
                            parsed.port,
                            handle.0
                        );
                    }
                }
                api::Event::TcpEstablished { handle } => {
                    if opened.is_none() {
                        opened = Some(handle);
                    }
                    if opened == Some(handle) {
                        if crate::logflag::VHTTPS_VERBOSE {
                            crate::log!(
                                "http-hyper: established host={} port={} handle={}\n",
                                parsed.host,
                                parsed.port,
                                handle.0
                            );
                        }
                        break 'connect_wait handle;
                    }
                }
                api::Event::Closed { handle } if opened == Some(handle) => {
                    return Err(HttpFetchError::TimedOut);
                }
                api::Event::Error { .. } => return Err(HttpFetchError::TimedOut),
                _ => {}
            }
        }
        if Instant::now() >= deadline {
            return Err(HttpFetchError::TimedOut);
        }
        Timer::after(EmbassyDuration::from_millis(2)).await;
    };

    let (client_io, bridge_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Err(err) = tcp_duplex_bridge(net, handle, bridge_io).await {
            crate::log!("http-hyper: bridge ended err={:?}\n", err);
        }
    });

    Ok(client_io)
}

fn hyper_redirect_url_from_location(
    current: &ParsedHttpUrl,
    headers: &hyper::HeaderMap,
) -> Option<String> {
    let loc = headers.get(hyper::header::LOCATION)?.to_str().ok()?.trim();
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

pub async fn fetch_http_body_hyper(
    url: &str,
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    let parsed = parse_http_url(url).map_err(|_| HttpFetchError::BadUrl)?;
    let stream = connect_hyper_tcp_stream(&parsed, timeout_ms).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, HyperEmptyBody>(HyperTokioIo::new(stream))
            .await
            .map_err(|_| HttpFetchError::TimedOut)?;
    let connection = tokio::spawn(async move { connection.await });

    sender.ready().await.map_err(|_| HttpFetchError::TimedOut)?;
    crate::log!("http-hyper: request host={} path={}\n", parsed.host, parsed.path);
    let request = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(parsed.path.as_str())
        .header(hyper::header::HOST, parsed.host.as_str())
        .header(hyper::header::USER_AGENT, "TRUEOS hyper")
        .header(hyper::header::ACCEPT, "*/*")
        .header(hyper::header::ACCEPT_ENCODING, "identity")
        .header(hyper::header::CONNECTION, "close")
        .body(HyperEmptyBody)
        .map_err(|_| HttpFetchError::BadUrl)?;
    let response = tokio::time::timeout(
        core::time::Duration::from_millis(timeout_ms as u64),
        sender.send_request(request),
    )
    .await
    .map_err(|_| HttpFetchError::TimedOut)?
    .map_err(|_| HttpFetchError::TimedOut)?;

    let status = response.status().as_u16();
    if is_redirect_status(status) {
        if let Some(url) = hyper_redirect_url_from_location(&parsed, response.headers()) {
            return Err(HttpFetchError::Redirect(url));
        }
    }
    if status >= 400 {
        return Err(HttpFetchError::HttpStatus(status));
    }

    let mut body = response.into_body();
    let mut out = Vec::new();
    loop {
        let next = tokio::time::timeout(
            core::time::Duration::from_millis(timeout_ms as u64),
            core::future::poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)),
        )
        .await
        .map_err(|_| HttpFetchError::TimedOut)?;
        let Some(frame) = next else {
            break;
        };
        let frame = frame.map_err(|_| HttpFetchError::TimedOut)?;
        if let Ok(data) = frame.into_data() {
            if out.len().saturating_add(data.len()) > max_rx {
                return Err(HttpFetchError::ResponseTooLarge);
            }
            out.extend_from_slice(&data);
        }
    }

    drop(sender);
    let _ = tokio::time::timeout(core::time::Duration::from_millis(250), connection).await;
    crate::log!("http-hyper: body-complete host={} bytes={}\n", parsed.host, out.len());
    Ok(out)
}

pub async fn post_http_body(
    url: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    request_http_body(b"POST", url, extra_headers, body, timeout_ms, max_rx).await
}
