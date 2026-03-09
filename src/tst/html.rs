use alloc::string::String;
use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;

use trueos_v::vnet as api;

use crate::v::net::VNet;
use crate::v::net::dns::{self, DnsConfig};
use crate::v::net::https;

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

async fn fetch_http_plain_body(url: &str) -> Result<Vec<u8>, &'static str> {
    let parsed = parse_http_url(url)?;

    let ready = crate::v::readiness::wait_for_timeout(
        crate::v::readiness::NET_CONFIGURED,
        EmbassyDuration::from_secs(3),
    )
    .await;
    if !ready {
        // Best-effort: continue and let DNS/TCP timeouts decide final outcome.
    }

    let Ok(ip) = dns::resolve_ipv4_primary(parsed.host.as_str(), DnsConfig::default()).await else {
        return Err("dns failed");
    };

    let net = loop {
        if let Some(v) = VNet::open_primary() {
            break v;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    };

    let _ = net.submit(api::Command::OpenTcpConnect {
        remote: api::EndpointV4 {
            addr: ip,
            port: parsed.port,
        },
    });

    let mut tcp_handle: Option<api::NetHandle> = None;
    let mut sent_get = false;

    // Cap to avoid unbounded kernel heap growth.
    const MAX_RX: usize = 2 * 1024 * 1024;
    let mut rx: Vec<u8> = Vec::new();
    let mut truncated = false;

    let deadline = Instant::now() + EmbassyDuration::from_secs(12);

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
                            let _ = net.submit(api::Command::SendTcp {
                                handle: h,
                                data: api::ByteBuf::from_slice_trunc(req.as_slice()),
                            });
                            sent_get = true;
                        }
                    }
                }
                api::Event::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    let data = data.as_slice();
                    if rx.len() < MAX_RX {
                        let room = MAX_RX - rx.len();
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
                        if status >= 400 {
                            return Err("http status error");
                        }

                        let body = if body_off <= rx.len() {
                            rx.split_off(body_off)
                        } else {
                            Vec::new()
                        };
                        if truncated {
                            return Err("response too large");
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
            return Err("timed out");
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

pub async fn fetch_html_best_effort(url: HString<256>) -> Result<String, &'static str> {
    let attempts = build_best_effort_attempts(url.as_str());
    if attempts.is_empty() {
        return Err("bad url");
    }

    let mut saw_timeout = false;

    for attempt in attempts.iter() {
        if attempt.starts_with("https://") {
            match https::fetch_https_body_async(attempt.as_str(), 12_000, 2 * 1024 * 1024).await {
                Ok(body) => return Ok(bytes_to_string_lossy(body)),
                Err(crate::v::net::https::FetchError::ConnectTimeout)
                | Err(crate::v::net::https::FetchError::DnsTimeout)
                | Err(crate::v::net::https::FetchError::TlsTimeout)
                | Err(crate::v::net::https::FetchError::BodyTimeout) => {
                    saw_timeout = true;
                }
                Err(_) => {}
            }
            continue;
        }

        match fetch_http_plain_body(attempt.as_str()).await {
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
