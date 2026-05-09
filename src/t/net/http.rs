extern crate alloc;
extern crate std;

use crate::r::net::NetProfile;
use crate::r::net::VNet;
use crate::t::net::dns::{self, DnsConfig};
use crate::t::net::hyper_io::{HyperBytesBody, HyperTokioIo};
use alloc::string::{String, ToString};
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

fn header_get_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
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

pub(super) fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
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

pub(super) async fn connect_hyper_tcp_stream(
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

pub(super) fn hyper_redirect_url_from_location(
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

pub async fn post_http_body_hyper(
    url: &str,
    content_type: &str,
    body_bytes: &[u8],
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    post_http_body_hyper_with_headers(url, content_type, &[], body_bytes, timeout_ms, max_rx).await
}

pub async fn post_http_body_hyper_with_headers(
    url: &str,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    body_bytes: &[u8],
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    request_http_body_hyper(
        hyper::Method::POST,
        url,
        content_type,
        extra_headers,
        body_bytes,
        timeout_ms,
        max_rx,
    )
    .await
}

pub async fn fetch_http_body_hyper(
    url: &str,
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    request_http_body_hyper(hyper::Method::GET, url, "", &[], &[], timeout_ms, max_rx).await
}

async fn request_http_body_hyper(
    method: hyper::Method,
    url: &str,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    body_bytes: &[u8],
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpFetchError> {
    let parsed = parse_http_url(url).map_err(|_| HttpFetchError::BadUrl)?;
    let stream = connect_hyper_tcp_stream(&parsed, timeout_ms).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake::<_, HyperBytesBody>(HyperTokioIo::new(stream))
            .await
            .map_err(|_| HttpFetchError::TimedOut)?;
    let connection = tokio::spawn(async move { connection.await });

    sender.ready().await.map_err(|_| HttpFetchError::TimedOut)?;
    crate::log!(
        "http-hyper: request method={} host={} path={}\n",
        method.as_str(),
        parsed.host,
        parsed.path
    );
    let mut builder = hyper::Request::builder()
        .method(method)
        .uri(parsed.path.as_str())
        .header(hyper::header::HOST, parsed.host.as_str())
        .header(hyper::header::USER_AGENT, "TRUEOS hyper")
        .header(hyper::header::ACCEPT, "*/*")
        .header(hyper::header::ACCEPT_ENCODING, "identity")
        .header(hyper::header::CONNECTION, "close");
    if !body_bytes.is_empty() {
        builder = builder
            .header(hyper::header::CONTENT_TYPE, content_type)
            .header(hyper::header::CONTENT_LENGTH, body_bytes.len().to_string());
    }
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    let request = builder
        .body(HyperBytesBody::new(body_bytes))
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
