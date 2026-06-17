extern crate alloc;

use alloc::{boxed::Box, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use v::vnet;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, TlsTimeouts, register_tls_app_queues};
use crate::r::net::{NetProfile, Queue};

const DNS_TIMEOUT_MS: u64 = 5_000;
const DOT_PORT: u16 = 853;
const DOH_PORT: u16 = 443;
const DNS_QTYPE_A: u16 = 1;
static DNS_QUERY_SEQ: AtomicU32 = AtomicU32::new(1);
static DNS_TLS_SEQ: AtomicU32 = AtomicU32::new(1);
static SECURE_DNS_POLICY_LOGGED: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DnsError {
    BadName,
    NoAnswer,
    NoNic,
    Runtime,
    Timeout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DnsConfig {
    timeout_ms: u64,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            timeout_ms: DNS_TIMEOUT_MS,
        }
    }
}

impl DnsConfig {
    pub const fn for_device(_device_index: usize) -> Self {
        Self {
            timeout_ms: DNS_TIMEOUT_MS,
        }
    }

    pub const fn for_profile(_profile: NetProfile) -> Self {
        Self {
            timeout_ms: DNS_TIMEOUT_MS,
        }
    }

    pub const fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SecureDnsTransport {
    Doh,
    Dot,
}

impl SecureDnsTransport {
    const fn label(self) -> &'static str {
        match self {
            Self::Doh => "doh",
            Self::Dot => "dot",
        }
    }
}

#[derive(Clone, Copy)]
struct DohEndpoint {
    addr: [u8; 4],
    tls_name: &'static str,
    path: &'static str,
}

#[derive(Clone, Copy)]
struct DotEndpoint {
    addr: [u8; 4],
    tls_name: &'static str,
}

const PUBLIC_DOH_ENDPOINTS: [DohEndpoint; 3] = [
    DohEndpoint {
        addr: [1, 1, 1, 1],
        tls_name: "cloudflare-dns.com",
        path: "/dns-query",
    },
    DohEndpoint {
        addr: [8, 8, 8, 8],
        tls_name: "dns.google",
        path: "/dns-query",
    },
    DohEndpoint {
        addr: [9, 9, 9, 9],
        tls_name: "dns.quad9.net",
        path: "/dns-query",
    },
];

const PUBLIC_DOT_ENDPOINTS: [DotEndpoint; 3] = [
    DotEndpoint {
        addr: [1, 1, 1, 1],
        tls_name: "cloudflare-dns.com",
    },
    DotEndpoint {
        addr: [8, 8, 8, 8],
        tls_name: "dns.google",
    },
    DotEndpoint {
        addr: [9, 9, 9, 9],
        tls_name: "dns.quad9.net",
    },
];

#[derive(Clone, Copy)]
enum SecureTlsReply {
    HttpBody,
    DotMessage,
}

pub async fn resolve_ipv4_primary(host: &str, cfg: DnsConfig) -> Result<[u8; 4], DnsError> {
    let device_index = NetProfile::default()
        .resolve_device_index()
        .ok_or(DnsError::NoNic)?;
    resolve_ipv4_for_device(device_index, host, cfg).await
}

pub async fn resolve_ipv4_for_profile(
    profile: NetProfile,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let device_index = profile.resolve_device_index().ok_or(DnsError::NoNic)?;
    resolve_ipv4_for_device(device_index, host, cfg).await
}

pub async fn resolve_ipv4_for_device(
    device_index: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() {
        return Err(DnsError::BadName);
    }
    if let Some(ip) = parse_ipv4_literal(host) {
        return Ok(ip);
    }

    if SECURE_DNS_POLICY_LOGGED
        .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        crate::log_info!(target: "net";
            "dns: secure resolver enabled transports=doh,dot classic-udp=disabled\n"
        );
    }

    let ready = crate::r::readiness::wait_for_timeout(
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TLS_SOCKET_SERVICE_READY,
        EmbassyDuration::from_millis(cfg.timeout_ms),
    )
    .await;
    if !ready {
        return Err(DnsError::Timeout);
    }

    crate::log_info!(target: "net";
        "dns: secure lookup begin host={} dev={} qtype=A timeout_ms={}\n",
        host,
        device_index,
        cfg.timeout_ms
    );

    match resolve_ipv4_doh_for_device(device_index, host, cfg).await {
        Ok(ip) => {
            log_dns_ip("dns: secure resolved", SecureDnsTransport::Doh, host, device_index, ip);
            return Ok(ip);
        }
        Err(err) => crate::log_info!(target: "net";
            "dns: secure transport failed transport=doh host={} dev={} err={:?}\n",
            host,
            device_index,
            err
        ),
    }

    match resolve_ipv4_dot_for_device(device_index, host, cfg).await {
        Ok(ip) => {
            log_dns_ip("dns: secure resolved", SecureDnsTransport::Dot, host, device_index, ip);
            Ok(ip)
        }
        Err(err) => {
            crate::log_info!(target: "net";
                "dns: secure transport failed transport=dot host={} dev={} err={:?}; classic udp discarded\n",
                host,
                device_index,
                err
            );
            Err(DnsError::NoAnswer)
        }
    }
}

fn log_dns_ip(
    prefix: &str,
    transport: SecureDnsTransport,
    host: &str,
    dev_idx: usize,
    ip: [u8; 4],
) {
    crate::log_info!(target: "net";
        "{} transport={} host={} dev={} ip={}.{}.{}.{}\n",
        prefix,
        transport.label(),
        host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );
}

fn parse_ipv4_literal(host: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut count = 0usize;
    for part in host.split('.') {
        if count >= out.len() || part.is_empty() {
            return None;
        }
        out[count] = part.parse::<u8>().ok()?;
        count += 1;
    }
    if count == out.len() { Some(out) } else { None }
}

fn next_query_id() -> u16 {
    let seq = DNS_QUERY_SEQ.fetch_add(1, Ordering::AcqRel) as u16;
    seq ^ ((Instant::now().as_millis() as u16).rotate_left(5))
}

fn build_dns_query(host: &str, qtype: u16) -> Result<(u16, Vec<u8>), DnsError> {
    let id = next_query_id();
    let mut out = Vec::with_capacity(host.len() + 18);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());

    let mut total = 0usize;
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 || !label.is_ascii() {
            return Err(DnsError::BadName);
        }
        total = total.saturating_add(label.len() + 1);
        if total > 254 {
            return Err(DnsError::BadName);
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&qtype.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    Ok((id, out))
}

fn skip_dns_name(buf: &[u8], mut offset: usize) -> Option<usize> {
    let mut jumps = 0usize;
    loop {
        let len = *buf.get(offset)?;
        if len & 0xC0 == 0xC0 {
            let _ = *buf.get(offset + 1)?;
            return Some(offset + 2);
        }
        offset += 1;
        if len == 0 {
            return Some(offset);
        }
        if len & 0xC0 != 0 || len > 63 {
            return None;
        }
        offset = offset.checked_add(len as usize)?;
        jumps += 1;
        if jumps > 128 || offset > buf.len() {
            return None;
        }
    }
}

fn parse_dns_a_response(buf: &[u8], expected_id: u16) -> Result<[u8; 4], DnsError> {
    if buf.len() < 12 {
        return Err(DnsError::NoAnswer);
    }
    if u16::from_be_bytes([buf[0], buf[1]]) != expected_id {
        return Err(DnsError::NoAnswer);
    }
    let flags = u16::from_be_bytes([buf[2], buf[3]]);
    if flags & 0x8000 == 0 || flags & 0x000f != 0 {
        return Err(DnsError::NoAnswer);
    }

    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut offset = 12usize;
    for _ in 0..qd {
        offset = skip_dns_name(buf, offset).ok_or(DnsError::NoAnswer)?;
        offset = offset.checked_add(4).ok_or(DnsError::NoAnswer)?;
        if offset > buf.len() {
            return Err(DnsError::NoAnswer);
        }
    }

    for _ in 0..an {
        offset = skip_dns_name(buf, offset).ok_or(DnsError::NoAnswer)?;
        if offset + 10 > buf.len() {
            return Err(DnsError::NoAnswer);
        }
        let ty = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let class = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]);
        let rdlen = u16::from_be_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlen > buf.len() {
            return Err(DnsError::NoAnswer);
        }
        if ty == DNS_QTYPE_A && class == 1 && rdlen == 4 {
            return Ok([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
        }
        offset += rdlen;
    }

    Err(DnsError::NoAnswer)
}

fn base64url_no_pad(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0usize;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | input[i + 2] as u32;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(n & 0x3f) as usize] as char);
        i += 3;
    }
    match input.len() - i {
        1 => {
            let n = (input[i] as u32) << 16;
            out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        }
        2 => {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
            out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        }
        _ => {}
    }
    out
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn header_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    for line in headers.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let key = &line[..colon];
        if key.len() == name.len()
            && key
                .iter()
                .zip(name.iter())
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return Some(trim_ascii(&line[colon + 1..]));
        }
    }
    None
}

fn header_value_has_token(value: &[u8], token: &[u8]) -> bool {
    value.split(|b| *b == b',' || *b == b';').any(|part| {
        let part = trim_ascii(part);
        part.len() == token.len()
            && part
                .iter()
                .zip(token.iter())
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|idx| idx + 4)
}

fn decode_http_chunked(body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    loop {
        let line_rel = body[offset..].windows(2).position(|w| w == b"\r\n")?;
        let line = &body[offset..offset + line_rel];
        let size_text = core::str::from_utf8(line.split(|b| *b == b';').next()?).ok()?;
        let size = usize::from_str_radix(size_text.trim(), 16).ok()?;
        offset = offset.checked_add(line_rel + 2)?;
        if size == 0 {
            return Some(out);
        }
        if offset.checked_add(size + 2)? > body.len() {
            return None;
        }
        out.extend_from_slice(&body[offset..offset + size]);
        offset += size + 2;
    }
}

fn tls_reply_complete(
    buf: &[u8],
    mode: SecureTlsReply,
    closed: bool,
) -> Option<Result<Vec<u8>, DnsError>> {
    match mode {
        SecureTlsReply::DotMessage => {
            if buf.len() < 2 {
                return None;
            }
            let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
            if buf.len() >= len + 2 {
                Some(Ok(buf[2..2 + len].to_vec()))
            } else {
                None
            }
        }
        SecureTlsReply::HttpBody => {
            let header_end = find_header_end(buf)?;
            let headers = &buf[..header_end];
            let body = &buf[header_end..];
            if !(headers.starts_with(b"HTTP/1.1 2") || headers.starts_with(b"HTTP/1.0 2")) {
                return Some(Err(DnsError::NoAnswer));
            }
            if let Some(te) = header_value(headers, b"Transfer-Encoding")
                && header_value_has_token(te, b"chunked")
            {
                if let Some(decoded) = decode_http_chunked(body) {
                    return Some(Ok(decoded));
                }
                return closed.then_some(Err(DnsError::NoAnswer));
            }
            if let Some(len_text) = header_value(headers, b"Content-Length")
                && let Ok(len) = core::str::from_utf8(len_text)
                    .unwrap_or("")
                    .parse::<usize>()
            {
                if body.len() >= len {
                    return Some(Ok(body[..len].to_vec()));
                }
                return None;
            }
            closed.then_some(Ok(body.to_vec()))
        }
    }
}

fn dns_tls_owner(prefix: &str, device_index: usize) -> &'static str {
    let seq = DNS_TLS_SEQ.fetch_add(1, Ordering::Relaxed);
    Box::leak(format!("{}-{}@{}", prefix, seq, device_index).into_boxed_str())
}

fn dns_tls_queues(owner: &'static str) -> (&'static Queue<TlsCommand>, &'static Queue<TlsEvent>) {
    let cmds_name = Box::leak(format!("{}-cmd", owner).into_boxed_str());
    let events_name = Box::leak(format!("{}-evt", owner).into_boxed_str());
    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(events_name, 1024);
    register_tls_app_queues(owner, cmds, events);
    (cmds, events)
}

async fn dns_tls_exchange_v4(
    prefix: &str,
    device_index: usize,
    addr: [u8; 4],
    port: u16,
    server_name: &'static str,
    payload: Vec<u8>,
    timeout_ms: u64,
    mode: SecureTlsReply,
) -> Result<Vec<u8>, DnsError> {
    let owner = dns_tls_owner(prefix, device_index);
    let (cmds, events) = dns_tls_queues(owner);
    let cfg = if port == DOH_PORT {
        TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"])
    } else {
        TlsClientConfig::new()
    };
    let timeouts = TlsTimeouts {
        connect_ms: (timeout_ms as u32).max(1000),
        tls_ms: (timeout_ms as u32).max(1000),
        idle_ms: (timeout_ms as u32).max(1000),
    };
    cmds.push(TlsCommand::OpenTcpConnect {
        remote: vnet::EndpointV4 { addr, port },
        server_name,
        cfg,
        roots: TlsRoots::mozilla(),
        timeouts,
    })
    .map_err(|_| DnsError::Runtime)?;

    let deadline = Instant::now() + EmbassyDuration::from_millis(timeout_ms);
    let mut handle = None;
    let mut sent = false;
    let mut rx = Vec::new();

    while Instant::now() < deadline {
        for ev in events.drain(256) {
            match ev {
                TlsEvent::Opened { handle: h } => handle = Some(h),
                TlsEvent::Connected { handle: h } => {
                    if handle.is_none() || Some(h) == handle {
                        handle = Some(h);
                        if !sent {
                            cmds.push(TlsCommand::Send {
                                handle: h,
                                data: payload.clone(),
                            })
                            .map_err(|_| DnsError::Runtime)?;
                            sent = true;
                        }
                    }
                }
                TlsEvent::Data { handle: h, data } if Some(h) == handle => {
                    rx.extend_from_slice(data.as_slice());
                    if let Some(done) = tls_reply_complete(rx.as_slice(), mode, false) {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                        return done;
                    }
                }
                TlsEvent::Closed { handle: h } if Some(h) == handle => {
                    if let Some(done) = tls_reply_complete(rx.as_slice(), mode, true) {
                        return done;
                    }
                    return Err(DnsError::NoAnswer);
                }
                TlsEvent::Error { msg } => {
                    crate::log_info!(target: "net";
                        "dns: secure tls-socket error owner={} msg={}\n",
                        owner,
                        msg
                    );
                    return Err(DnsError::Runtime);
                }
                TlsEvent::TlsError { err } => {
                    crate::log_info!(target: "net";
                        "dns: secure tls error owner={} err={:?}\n",
                        owner,
                        err
                    );
                    return Err(DnsError::Runtime);
                }
                _ => {}
            }
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    if let Some(handle) = handle {
        let _ = cmds.push(TlsCommand::Close { handle });
    }
    Err(DnsError::Timeout)
}

async fn resolve_ipv4_doh_for_device(
    device_index: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let timeout_ms = cfg.timeout_ms.max(1000);
    let (query_id, query) = build_dns_query(host, DNS_QTYPE_A)?;
    let encoded = base64url_no_pad(query.as_slice());

    for endpoint in PUBLIC_DOH_ENDPOINTS {
        let req = format!(
            "GET {}?dns={} HTTP/1.1\r\nHost: {}\r\nAccept: application/dns-message\r\nConnection: close\r\nUser-Agent: TRUEOS/secure-dns\r\n\r\n",
            endpoint.path,
            encoded,
            endpoint.tls_name
        )
        .into_bytes();
        match dns_tls_exchange_v4(
            "dns-doh",
            device_index,
            endpoint.addr,
            DOH_PORT,
            endpoint.tls_name,
            req,
            timeout_ms,
            SecureTlsReply::HttpBody,
        )
        .await
        {
            Ok(body) => match parse_dns_a_response(body.as_slice(), query_id) {
                Ok(ip) => return Ok(ip),
                Err(err) => crate::log_info!(target: "net";
                    "dns: doh response parse failed host={} dev={} server={} err={:?}\n",
                    host,
                    device_index,
                    endpoint.tls_name,
                    err
                ),
            },
            Err(err) => crate::log_info!(target: "net";
                "dns: doh endpoint failed host={} dev={} server={} err={:?}\n",
                host,
                device_index,
                endpoint.tls_name,
                err
            ),
        }
    }

    Err(DnsError::NoAnswer)
}

async fn resolve_ipv4_dot_for_device(
    device_index: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let timeout_ms = cfg.timeout_ms.max(1000);
    let (query_id, query) = build_dns_query(host, DNS_QTYPE_A)?;
    let mut payload = Vec::with_capacity(query.len() + 2);
    payload.extend_from_slice(&(query.len() as u16).to_be_bytes());
    payload.extend_from_slice(query.as_slice());

    for endpoint in PUBLIC_DOT_ENDPOINTS {
        match dns_tls_exchange_v4(
            "dns-dot",
            device_index,
            endpoint.addr,
            DOT_PORT,
            endpoint.tls_name,
            payload.clone(),
            timeout_ms,
            SecureTlsReply::DotMessage,
        )
        .await
        {
            Ok(body) => match parse_dns_a_response(body.as_slice(), query_id) {
                Ok(ip) => return Ok(ip),
                Err(err) => crate::log_info!(target: "net";
                    "dns: dot response parse failed host={} dev={} server={} err={:?}\n",
                    host,
                    device_index,
                    endpoint.tls_name,
                    err
                ),
            },
            Err(err) => crate::log_info!(target: "net";
                "dns: dot endpoint failed host={} dev={} server={} err={:?}\n",
                host,
                device_index,
                endpoint.tls_name,
                err
            ),
        }
    }

    Err(DnsError::NoAnswer)
}
