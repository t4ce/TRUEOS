use alloc::{boxed::Box, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;

use crate::net::adapter::{
    register_app_queues, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue, SocketKind,
};

// Reuse the QEMU slirp defaults from `net::adapter`.
const SLIRP_DNS_IP: [u8; 4] = [10, 0, 2, 3];
static HTTP_GET_JOB_SEQ: AtomicU32 = AtomicU32::new(1);

fn dns_query(id: u16, host: &str, qtype: u16) -> Vec<u8> {
    // Minimal DNS query for <qtype>/IN <host>.
    // Header: flags=0x0100 (RD), qdcount=1
    let mut q = Vec::new();
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&0x0100u16.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
    q.extend_from_slice(&0u16.to_be_bytes());
    q.extend_from_slice(&0u16.to_be_bytes());
    q.extend_from_slice(&0u16.to_be_bytes());

    // QNAME
    for label in host.split('.') {
        let bytes = label.as_bytes();
        let len = bytes.len().min(63);
        q.push(len as u8);
        q.extend_from_slice(&bytes[..len]);
    }
    q.push(0);

    // QTYPE, QCLASS=IN (1)
    q.extend_from_slice(&qtype.to_be_bytes());
    q.extend_from_slice(&1u16.to_be_bytes());
    q
}

fn dns_skip_name(pkt: &[u8], idx: &mut usize) -> bool {
    if *idx >= pkt.len() {
        return false;
    }

    let mut steps: u8 = 0;
    loop {
        if *idx >= pkt.len() {
            return false;
        }

        let b = pkt[*idx];
        if b == 0 {
            *idx += 1;
            return true;
        }

        if (b & 0xC0) == 0xC0 {
            if *idx + 1 >= pkt.len() {
                return false;
            }
            *idx += 2;
            return true;
        }

        let len = b as usize;
        *idx += 1;
        if *idx + len > pkt.len() {
            return false;
        }
        *idx += len;

        steps = steps.wrapping_add(1);
        if steps > 64 {
            return false;
        }
    }
}

fn dns_parse_first_a(pkt: &[u8], want_id: u16) -> Result<Option<[u8; 4]>, &'static str> {
    if pkt.len() < 12 {
        return Err("dns too short");
    }

    let id = u16::from_be_bytes([pkt[0], pkt[1]]);
    if id != want_id {
        return Ok(None);
    }

    let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
    let rcode = (flags & 0x000F) as u8;
    if rcode != 0 {
        return Err("dns rcode != 0");
    }

    let qd = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
    let an = u16::from_be_bytes([pkt[6], pkt[7]]) as usize;

    let mut idx: usize = 12;
    for _ in 0..qd {
        if !dns_skip_name(pkt, &mut idx) {
            return Err("dns bad qname");
        }
        if idx + 4 > pkt.len() {
            return Err("dns qtype/qclass truncated");
        }
        idx += 4;
    }

    for _ in 0..an {
        if !dns_skip_name(pkt, &mut idx) {
            return Err("dns bad aname");
        }
        if idx + 10 > pkt.len() {
            return Err("dns answer hdr truncated");
        }
        let typ = u16::from_be_bytes([pkt[idx], pkt[idx + 1]]);
        let class = u16::from_be_bytes([pkt[idx + 2], pkt[idx + 3]]);
        let rdlen = u16::from_be_bytes([pkt[idx + 8], pkt[idx + 9]]) as usize;
        idx += 10;
        if idx + rdlen > pkt.len() {
            return Err("dns rdata truncated");
        }

        if typ == 1 && class == 1 && rdlen == 4 {
            return Ok(Some([pkt[idx], pkt[idx + 1], pkt[idx + 2], pkt[idx + 3]]));
        }

        idx += rdlen;
    }

    Ok(None)
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

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
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
/// Downloads into the matrix slot blob so users can save it via `io` later.
#[task]
pub async fn http_get_matrix_job(slot_id: u8, url: HString<256>) {
    crate::matrix::push_line(slot_id, "get: starting");

    if crate::net::mac_address().is_none() {
        crate::matrix::push_line(slot_id, "get: disabled (no NIC)");
        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
        return;
    }

    let parsed = match parse_http_url(url.as_str()) {
        Ok(p) => p,
        Err(e) => {
            crate::matrix::push_line(slot_id, "get: bad url");
            crate::matrix::push_line(slot_id, e);
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            return;
        }
    };

    let seq = HTTP_GET_JOB_SEQ.fetch_add(1, Ordering::Relaxed);
    let owner = leak_str(alloc::format!("net-httpget-{}-{}", slot_id + 1, seq));
    let cmds_name = leak_str(alloc::format!("{}-cmd", owner));
    let evts_name = leak_str(alloc::format!("{}-evt", owner));
    let cmds = NetQueue::new_leaked(cmds_name, 128);
    let events = NetQueue::new_leaked(evts_name, 128);
    register_app_queues(owner, cmds, events);

    let dns_id: u16 = 0xA100u16.wrapping_add(slot_id as u16);
    let udp_port: u16 = 4400u16.wrapping_add(slot_id as u16);
    let _ = cmds.push(NetCommand::OpenUdp { port: udp_port });
    crate::matrix::push_line(slot_id, "get: opening udp");

    let mut udp_handle: Option<NetHandle> = None;
    let mut tcp_handle: Option<NetHandle> = None;
    let mut resolved: Option<[u8; 4]> = None;
    let mut sent_dns = false;
    let mut sent_connect = false;
    let mut sent_get = false;

    // Cap to avoid unbounded kernel heap growth.
    const MAX_RX: usize = 2 * 1024 * 1024;
    let mut rx: Vec<u8> = Vec::new();
    let mut truncated = false;

    let deadline = Instant::now() + EmbassyDuration::from_secs(12);

    loop {
        for ev in events.drain(32) {
            match ev {
                NetEvent::Opened { handle, kind } => match kind {
                    SocketKind::Udp => {
                        udp_handle = Some(handle);
                        crate::matrix::push_line(slot_id, "get: udp opened");
                    }
                    SocketKind::Tcp => {
                        tcp_handle = Some(handle);
                        crate::matrix::push_line(slot_id, "get: tcp opened");
                    }
                },
                NetEvent::UdpPacket { handle, data, .. } => {
                    if udp_handle != Some(handle) {
                        continue;
                    }
                    match dns_parse_first_a(&data, dns_id) {
                        Ok(Some(ip)) => {
                            resolved = Some(ip);
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
                        Ok(None) => {}
                        Err(e) => {
                            crate::matrix::push_line(slot_id, "get: dns parse error");
                            crate::matrix::push_line(slot_id, e);
                            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                            return;
                        }
                    }
                }
                NetEvent::TcpEstablished { handle } => {
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
                        req.extend_from_slice(b"\r\nUser-Agent: TRUEOS get\r\nAccept: */*\r\nConnection: close\r\n\r\n");
                        if let Some(h) = tcp_handle {
                            let _ = cmds.push(NetCommand::SendTcp { handle: h, data: req });
                            sent_get = true;
                            crate::matrix::push_line(slot_id, "get: sent http request");
                        }
                    }
                }
                NetEvent::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }
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
                NetEvent::Closed { handle } => {
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

                        if let Some(h) = udp_handle {
                            let _ = cmds.push(NetCommand::Close { handle: h });
                        }
                        return;
                    }
                    if udp_handle == Some(handle) {
                        udp_handle = None;
                    }
                }
                NetEvent::Error { msg } => {
                    let _ = msg;
                }
                NetEvent::TcpSent { .. } => {}
            }
        }

        if !sent_dns {
            if let Some(h) = udp_handle {
                let dns = dns_query(dns_id, parsed.host.as_str(), 1);
                let _ = cmds.push(NetCommand::SendUdp {
                    handle: h,
                    remote: NetEndpoint {
                        addr: SLIRP_DNS_IP,
                        port: 53,
                    },
                    data: dns,
                });
                sent_dns = true;
                crate::matrix::push_line(slot_id, "get: sent dns query");
            }
        }

        if !sent_connect {
            if let Some(ip) = resolved {
                let _ = cmds.push(NetCommand::OpenTcpConnect {
                    remote: NetEndpoint {
                        addr: ip,
                        port: parsed.port,
                    },
                });
                sent_connect = true;
                crate::matrix::push_line(slot_id, "get: opening tcp");
            }
        }

        if Instant::now() >= deadline {
            crate::matrix::push_line(slot_id, "get: timed out");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            if let Some(h) = tcp_handle {
                let _ = cmds.push(NetCommand::Close { handle: h });
            }
            if let Some(h) = udp_handle {
                let _ = cmds.push(NetCommand::Close { handle: h });
            }
            return;
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}
