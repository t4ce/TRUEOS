use alloc::string::String;
use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;

use trueos_v::vnet as api;

use crate::v::net::VNet;
use crate::v::net::dns::{self, DnsConfig};
use crate::v::net::https;

const SURF_TIMEOUT_MS: u32 = 35_000;
const SURF_MAX_BYTES: usize = 4 * 1024 * 1024;
// vhttps currently derives connect/tls budgets from timeout_ms/4.
// Scale surf's timeout so connect/tls do not fail far earlier than requested.
const SURF_HTTPS_TIMEOUT_MS: u32 = SURF_TIMEOUT_MS * 4;

fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
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

fn parse_http_status(buf: &[u8]) -> Option<u16> {
    // Expect: HTTP/1.1 200 ...\r\n
    if !buf.starts_with(b"HTTP/") {
        return None;
    }
    let mut i = 0;
    while i < buf.len() && buf[i] != b' ' {
        i += 1;
    }
    if i + 4 >= buf.len() {
        return None;
    }
    if buf[i] != b' ' {
        return None;
    }
    let d1 = buf.get(i + 1)?.wrapping_sub(b'0');
    let d2 = buf.get(i + 2)?.wrapping_sub(b'0');
    let d3 = buf.get(i + 3)?.wrapping_sub(b'0');
    if d1 > 9 || d2 > 9 || d3 > 9 {
        return None;
    }
    Some((d1 as u16) * 100 + (d2 as u16) * 10 + (d3 as u16))
}

#[derive(Clone, Debug)]
struct ParsedHttpUrl {
    host: HString<96>,
    port: u16,
    path: HString<160>,
}

#[derive(Clone, Debug)]
enum HttpPlainFetchError {
    BadUrl,
    TimedOut,
    DnsFailed,
    HttpStatus,
    Redirect(String),
    ResponseTooLarge,
}

fn parse_http_url(url: &str) -> Result<ParsedHttpUrl, &'static str> {
    // Accept:
    // - http://host[:port][/path]
    // - host[:port][/path]
    // (no IPv6 bracket support here yet)
    let mut u = url.trim();
    if u.is_empty() {
        return Err("empty url");
    }
    if let Some(rest) = u.strip_prefix("http://") {
        u = rest;
    } else if u.strip_prefix("https://").is_some() {
        return Err("https:// not supported here (use plaintext http://)");
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
    if path.is_empty() {
        let _ = out_path.push('/');
    } else {
        let _ = out_path.push('/');
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

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn redirect_url_from_location_http(current: &ParsedHttpUrl, headers: &[u8]) -> Option<String> {
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
        return Some(alloc::format!(
            "http://{}:{}{}",
            current.host,
            current.port,
            loc
        ));
    }

    None
}

fn build_best_effort_attempts(url: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return out;
    }

    if let Some(rest) = trimmed.strip_prefix("https://") {
        out.push(String::from(trimmed));
        out.push(alloc::format!("http://{}", rest));
        return out;
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        out.push(String::from(trimmed));
        out.push(alloc::format!("https://{}", rest));
        return out;
    }

    out.push(alloc::format!("https://{}", trimmed));
    out.push(alloc::format!("http://{}", trimmed));
    out
}

fn bytes_to_string_lossy(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(bytes.as_slice()).into_owned()
}

async fn fetch_http_plain_body(
    url: &str,
    timeout_ms: u32,
    max_rx: usize,
) -> Result<Vec<u8>, HttpPlainFetchError> {
    let parsed = parse_http_url(url).map_err(|_| HttpPlainFetchError::BadUrl)?;

    let ready = crate::v::readiness::wait_for_timeout(
        crate::v::readiness::NET_CONFIGURED,
        EmbassyDuration::from_secs(3),
    )
    .await;
    if !ready {
        // Best-effort: continue and let DNS/TCP timeouts decide final outcome.
    }

    let Ok(ip) = dns::resolve_ipv4_primary(parsed.host.as_str(), DnsConfig::default()).await else {
        return Err(HttpPlainFetchError::DnsFailed);
    };

    let net = loop {
        if let Some(v) = VNet::open_primary() {
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
        crate::log!(
            "surf/http: open submit failed host={} port={}\n",
            parsed.host,
            parsed.port
        );
        return Err(HttpPlainFetchError::TimedOut);
    }

    let mut tcp_handle: Option<api::NetHandle> = None;
    let mut sent_get = false;

    // Cap to avoid unbounded kernel heap growth.
    let mut rx: Vec<u8> = Vec::new();
    let mut truncated = false;

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms as u64);

    loop {
        for _ in 0..32 {
            let Some(ev) = net.pop_event() else { break };
            match ev {
                api::Event::Opened { handle, kind } => match kind {
                    api::SocketKind::Tcp => {
                        tcp_handle = Some(handle);
                    }
                    api::SocketKind::Udp => {}
                },
                api::Event::UdpPacket { .. } => {}
                api::Event::UdpPacketV6 { .. } => {}
                api::Event::TcpSent { .. } => {}
                api::Event::IcmpReply { .. } => {}
                api::Event::IcmpReplyV6 { .. } => {}
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
                            b"\r\nUser-Agent: TRUEOS get\r\nAccept: text/html,application/xhtml+xml,*/*;q=0.8\r\nConnection: close\r\n\r\n",
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
                                    "surf/http: get submit failed host={} handle={}\n",
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
                        let body_off = hdr_end.unwrap_or(0);
                        let status = parse_http_status(&rx).unwrap_or(0);
                        if is_redirect_status(status)
                            && let Some(hdr_end) = hdr_end
                            && let Some(next) =
                                redirect_url_from_location_http(&parsed, &rx[..hdr_end])
                        {
                            return Err(HttpPlainFetchError::Redirect(next));
                        }
                        if status >= 400 {
                            return Err(HttpPlainFetchError::HttpStatus);
                        }

                        let body = if body_off <= rx.len() {
                            rx.split_off(body_off)
                        } else {
                            Vec::new()
                        };
                        if truncated {
                            return Err(HttpPlainFetchError::ResponseTooLarge);
                        }

                        return Ok(body);
                    }
                }
                api::Event::Error { msg } => {
                    let _ = msg;
                }
            }
        }

        if Instant::now() >= deadline {
            if let Some(h) = tcp_handle {
                let _ = net.submit(api::Command::Close { handle: h });
            }
            return Err(HttpPlainFetchError::TimedOut);
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

async fn fetch_html_attempt_with_redirects(url: &str) -> Result<Vec<u8>, &'static str> {
    const MAX_REDIRECTS: usize = 5;
    let mut current_url = String::from(url);
    let mut saw_timeout = false;

    for hop in 0..=MAX_REDIRECTS {
        if current_url.starts_with("https://") {
            match https::fetch_https_body_async(
                current_url.as_str(),
                SURF_HTTPS_TIMEOUT_MS,
                SURF_MAX_BYTES,
            )
            .await
            {
                Ok(body) => return Ok(body),
                Err(https::FetchError::Redirect { url, .. }) => {
                    if hop >= MAX_REDIRECTS {
                        return Err("too many redirects");
                    }
                    current_url = url;
                    continue;
                }
                Err(https::FetchError::ConnectTimeout)
                | Err(https::FetchError::DnsTimeout)
                | Err(https::FetchError::TlsTimeout)
                | Err(https::FetchError::BodyTimeout) => {
                    saw_timeout = true;
                    break;
                }
                Err(_) => break,
            }
        } else if current_url.starts_with("http://") {
            match fetch_http_plain_body(current_url.as_str(), SURF_TIMEOUT_MS, SURF_MAX_BYTES).await
            {
                Ok(body) => return Ok(body),
                Err(HttpPlainFetchError::Redirect(url)) => {
                    if hop >= MAX_REDIRECTS {
                        return Err("too many redirects");
                    }
                    current_url = url;
                    continue;
                }
                Err(HttpPlainFetchError::TimedOut) => {
                    saw_timeout = true;
                    break;
                }
                Err(_) => break,
            }
        } else {
            break;
        }
    }

    if saw_timeout {
        return Err("timed out");
    }

    Err("all attempts failed")
}

pub async fn fetch_html_best_effort(url: HString<256>) -> Result<String, &'static str> {
    let attempts = build_best_effort_attempts(url.as_str());
    if attempts.is_empty() {
        return Err("bad url");
    }

    let mut saw_timeout = false;

    for attempt in attempts.iter() {
        match fetch_html_attempt_with_redirects(attempt.as_str()).await {
            Ok(body) => return Ok(bytes_to_string_lossy(body)),
            Err("timed out") => {
                saw_timeout = true;
            }
            Err(_) => {}
        }
    }

    if saw_timeout {
        return Err("timed out");
    }

    Err("all attempts failed")
}
