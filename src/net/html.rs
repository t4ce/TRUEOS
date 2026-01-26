use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::net::adapter::{
    net_debug_counters, register_app_queues, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue,
    SocketKind,
};

// Reuse the QEMU slirp defaults from `net::adapter`.
const SLIRP_DNS_IP: [u8; 4] = [10, 0, 2, 3];

const HTTP_SMOKE_UDP_PORT: u16 = 4244;

static NET_HTTP_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);

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

fn log_http_snippet(buf: &[u8]) {
    const MAX_DUMP_BYTES: usize = 512;
    const MAX_LINES: usize = 16;
    const MAX_LINE_CHARS: usize = 160;

    if buf.is_empty() {
        crate::log!("net-http-smoke: http rx: <empty>\n");
        return;
    }

    let dump_len = buf.len().min(MAX_DUMP_BYTES);
    let mut s = String::with_capacity(dump_len);
    for &b in &buf[..dump_len] {
        match b {
            b'\n' | b'\r' | b'\t' => s.push(b as char),
            0x20..=0x7E => s.push(b as char),
            _ => s.push('.'),
        }
    }

    crate::log!(
        "net-http-smoke: http rx dump bytes={} (shown={}{} )\n",
        buf.len(),
        dump_len,
        if buf.len() > dump_len { ", truncated" } else { "" }
    );

    // Avoid a single huge log line; split into bounded lines.
    let mut lines_emitted: usize = 0;
    for raw_line in s.split('\n') {
        if lines_emitted >= MAX_LINES {
            crate::log!("net-http-smoke: http rx: ... (more lines omitted)\n");
            break;
        }

        // Trim trailing CR that comes from \r\n.
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() {
            crate::log!("net-http-smoke: http rx: \n");
            lines_emitted += 1;
            continue;
        }

        if line.len() <= MAX_LINE_CHARS {
            crate::log!("net-http-smoke: http rx: {}\n", line);
            lines_emitted += 1;
            continue;
        }

        // Very long line (e.g. a minified page): print a prefix.
        let prefix = &line[..MAX_LINE_CHARS.min(line.len())];
        crate::log!("net-http-smoke: http rx: {}...\n", prefix);
        lines_emitted += 1;
    }
}

/// Simple HTTP (plaintext) reachability probe.
///
/// - Resolves `google.de` via the slirp DNS server (10.0.2.3).
/// - Connects to port 80 and issues a minimal HTTP/1.1 GET.
/// - Treats 200 and all 3xx redirects as success.
#[task]
pub async fn net_http_smoke_task() {
    if NET_HTTP_SMOKE_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    if crate::net::mac_address().is_none() {
        crate::log!("net-http-smoke: disabled (no NIC)\n");
        return;
    }

    const OWNER: &'static str = "net-http-smoke";
    const DNS_ID: u16 = 0x1300;
    const HOST: &'static str = "google.de";
    const PATH: &'static str = "/";

    let cmds = NetQueue::new_leaked("net-http-smoke-cmd", 64);
    let events = NetQueue::new_leaked("net-http-smoke-evt", 64);
    register_app_queues(OWNER, cmds, events);

    let _ = cmds.push(NetCommand::OpenUdp {
        port: HTTP_SMOKE_UDP_PORT,
    });

    crate::log!(
        "net-http-smoke: starting (dns={} tcp=80 udp_bind={})\n",
        "10.0.2.3",
        HTTP_SMOKE_UDP_PORT
    );

    let mut udp_handle: Option<NetHandle> = None;
    let mut tcp_handle: Option<NetHandle> = None;
    let mut resolved_ip: Option<[u8; 4]> = None;
    let mut dns_sent: bool = false;
    let mut tcp_connect_sent: bool = false;
    let mut tcp_established: bool = false;
    let mut http_req: Option<Vec<u8>> = None;
    let mut http_sent: bool = false;
    let mut rx_buf: Vec<u8> = Vec::new();

    let mut ticks: u32 = 0;
    let mut last_stats: Option<(u64, u64, u64, u32, u32)> = None;

    // 50ms tick, so 200 ticks ~ 10 seconds.
    const TIMEOUT_TICKS: u32 = 200;

    loop {
        for ev in events.drain(32) {
            match ev {
                NetEvent::Opened { handle, kind } => {
                    if kind == SocketKind::Udp {
                        udp_handle = Some(handle);
                        crate::log!("net-http-smoke: opened udp handle={}\n", handle.0);
                    }
                    if kind == SocketKind::Tcp {
                        tcp_handle = Some(handle);
                        crate::log!("net-http-smoke: opened tcp handle={}\n", handle.0);
                    }
                }
                NetEvent::UdpPacket { from, data, .. } => {
                    if from.port == 53 && from.addr == SLIRP_DNS_IP {
                        match dns_parse_first_a(&data, DNS_ID) {
                            Ok(Some(ip)) => {
                                resolved_ip = Some(ip);
                                crate::log!(
                                    "net-http-smoke: nslookup {} => A {}.{}.{}.{}\n",
                                    HOST,
                                    ip[0],
                                    ip[1],
                                    ip[2],
                                    ip[3]
                                );
                            }
                            Ok(None) => {}
                            Err(msg) => {
                                crate::log!("net-http-smoke: dns parse error ({})\n", msg);
                            }
                        }
                    }
                }
                NetEvent::TcpEstablished { handle } => {
                    if tcp_handle == Some(handle) {
                        crate::log!("net-http-smoke: tcp established handle={}\n", handle.0);
                        tcp_established = true;
                    }
                }
                NetEvent::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }

                    let remaining = 1024usize.saturating_sub(rx_buf.len());
                    if remaining > 0 {
                        rx_buf.extend_from_slice(&data[..data.len().min(remaining)]);
                    }

                    if let Some(code) = parse_http_status(&rx_buf) {
                        let ok = matches!(code, 200 | 301 | 302 | 303 | 307 | 308);
                        crate::log!(
                            "net-http-smoke: http {} {} => status={} ({})\n",
                            HOST,
                            PATH,
                            code,
                            if ok { "ok" } else { "unexpected" }
                        );

                        // Dump (bounded) response snippet to help debugging.
                        log_http_snippet(&rx_buf);

                        if let Some(h) = tcp_handle {
                            let _ = cmds.push(NetCommand::Close { handle: h });
                        }
                        if let Some(h) = udp_handle {
                            let _ = cmds.push(NetCommand::Close { handle: h });
                        }
                        return;
                    }
                }
                NetEvent::Closed { handle } => {
                    if tcp_handle == Some(handle) {
                        tcp_handle = None;
                        tcp_established = false;
                    }
                    if udp_handle == Some(handle) {
                        udp_handle = None;
                    }
                }
                NetEvent::Error { msg } => {
                    // `SendTcp` before ESTABLISHED will typically yield "tcp not ready".
                    // Keep errors visible but not spammy.
                    if (ticks % 20) == 0 {
                        crate::log!("net-http-smoke: error {}\n", msg);
                    }
                }
                NetEvent::TcpSent { handle, len } => {
                    if tcp_handle == Some(handle) {
                        if len > 0 {
                            http_sent = true;
                        }
                        crate::log!(
                            "net-http-smoke: tcp sent handle={} len={}\n",
                            handle.0,
                            len
                        );
                    }
                }
            }
        }

        // Kick DNS once UDP is ready.
        if !dns_sent {
            if let Some(handle) = udp_handle {
                let _ = cmds.push(NetCommand::SendUdp {
                    handle,
                    remote: NetEndpoint {
                        addr: SLIRP_DNS_IP,
                        port: 53,
                    },
                    data: dns_query(DNS_ID, HOST, 1),
                });
                dns_sent = true;
                crate::log!("net-http-smoke: nslookup {} (udp) via 10.0.2.3:53\n", HOST);
            }
        }

        // Once we have an A record, open TCP.
        if !tcp_connect_sent {
            if let Some(ip) = resolved_ip {
                let _ = cmds.push(NetCommand::OpenTcpConnect {
                    remote: NetEndpoint { addr: ip, port: 80 },
                });
                tcp_connect_sent = true;
                crate::log!(
                    "net-http-smoke: tcp connect {}.{}.{}.{}:80\n",
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                );

                // Build the request once and keep it around for retries.
                let mut req = Vec::new();
                req.extend_from_slice(b"GET ");
                req.extend_from_slice(PATH.as_bytes());
                req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
                req.extend_from_slice(HOST.as_bytes());
                req.extend_from_slice(
                    b"\r\nUser-Agent: TRUEOS net-http-smoke\r\nConnection: close\r\n\r\n",
                );
                http_req = Some(req);
            }
        }

        // Send HTTP request only once the TCP connection is established.
        // If the socket briefly reports "tcp not ready", we'll retry.
        if !http_sent && tcp_established {
            if let (Some(handle), Some(req)) = (tcp_handle, http_req.clone()) {
                if cmds.push(NetCommand::SendTcp { handle, data: req }).is_ok() {
                    crate::log!("net-http-smoke: http get {}{}\n", HOST, PATH);
                }
            }
        }

        ticks = ticks.wrapping_add(1);
        if (ticks % 20) == 0 {
            let (rx, tx, dropped) = net_debug_counters();
            let tcp_id = tcp_handle.map(|h| h.0).unwrap_or(0);
            let udp_id = udp_handle.map(|h| h.0).unwrap_or(0);
            let cur = (rx, tx, dropped, tcp_id, udp_id);
            if last_stats != Some(cur) {
                last_stats = Some(cur);
                crate::log!(
                    "net-http-smoke: stats rx={} tx={} dropped={} tcp_handle={} udp_handle={}\n",
                    rx,
                    tx,
                    dropped,
                    tcp_id,
                    udp_id
                );
            }
        }

        if ticks >= TIMEOUT_TICKS {
            crate::log!("net-http-smoke: timed out\n");

            // If we got any bytes, show them to aid debugging.
            if !rx_buf.is_empty() {
                log_http_snippet(&rx_buf);
            }

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
