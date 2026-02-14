use alloc::vec::Vec;

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;

use trueos_v::vnet as api;

use crate::v::net::dns::{self, DnsConfig};
use crate::v::net::VNet;

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
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
}

/// Minimal plaintext HTTP GET downloader.
///
/// Downloads into the matrix slot blob so users can save it later.
#[task]
pub async fn http_get_matrix_job(slot_id: u8, url: HString<256>) {
    crate::matrix::push_line(slot_id, "get: starting");

    // Permanent FSM gating: do not run until the network is actually usable.
    crate::v::readiness::wait_for(crate::v::readiness::NET_GATEWAY_REACHABLE).await;

    let parsed = match parse_http_url(url.as_str()) {
        Ok(p) => p,
        Err(e) => {
            crate::matrix::push_line(slot_id, "get: bad url");
            crate::matrix::push_line(slot_id, e);
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            return;
        }
    };

    let Ok(ip) = dns::resolve_ipv4_primary(parsed.host.as_str(), DnsConfig::default()).await else {
        crate::matrix::push_line(slot_id, "get: dns failed");
        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
        return;
    };

    {
        let line = alloc::format!(
            "get: nslookup {} => {}.{}.{}.{}",
            parsed.host.as_str(),
            ip[0],
            ip[1],
            ip[2],
            ip[3]
        );
        crate::matrix::push_line(slot_id, line.as_str());
    }

    let net = loop {
        if let Some(v) = VNet::open_primary() {
            break v;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    };

    let _ = net.submit(api::Command::OpenTcpConnect {
        remote: api::EndpointV4 { addr: ip, port: parsed.port },
    });
    crate::matrix::push_line(slot_id, "get: opening tcp");

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
                        crate::matrix::push_line(slot_id, "get: tcp opened");
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
                    crate::matrix::push_line(slot_id, "get: tcp established");
                    if !sent_get {
                        let mut req: Vec<u8> = Vec::new();
                        req.extend_from_slice(b"GET ");
                        req.extend_from_slice(parsed.path.as_str().as_bytes());
                        req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
                        req.extend_from_slice(parsed.host.as_str().as_bytes());
                        req.extend_from_slice(
                            b"\r\nUser-Agent: TRUEOS get\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                        );
                        if let Some(h) = tcp_handle {
                            let _ = net.submit(api::Command::SendTcp {
                                handle: h,
                                data: api::ByteBuf::from_slice_trunc(req.as_slice()),
                            });
                            sent_get = true;
                            crate::matrix::push_line(slot_id, "get: sent http request");
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
                        // We expect the server to close (Connection: close). Finish now.
                        let hdr_end = find_http_header_end(&rx);
                        let body_off = hdr_end.unwrap_or(0);
                        let status = parse_http_status(&rx);
                        if let Some(code) = status {
                            let line = alloc::format!("get: http status={}", code);
                            crate::matrix::push_line(slot_id, line.as_str());
                        } else {
                            crate::matrix::push_line(slot_id, "get: http status unknown");
                        }

                        let body = if body_off <= rx.len() {
                            rx.split_off(body_off)
                        } else {
                            Vec::new()
                        };

                        let line = alloc::format!(
                            "get: body bytes={}{}",
                            body.len(),
                            if truncated { " (truncated)" } else { "" }
                        );
                        crate::matrix::push_line(slot_id, line.as_str());

                        let _ = crate::matrix::set_blob_owned_with_preview(slot_id, body);
                        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
                        return;
                    }
                }
                api::Event::Error { msg } => {
                    let _ = msg;
                }
            }
        }

        if Instant::now() >= deadline {
            crate::matrix::push_line(slot_id, "get: timed out");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            if let Some(h) = tcp_handle {
                let _ = net.submit(api::Command::Close { handle: h });
            }
            return;
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}
