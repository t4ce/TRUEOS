extern crate alloc;
extern crate std;

use alloc::{boxed::Box, format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::Vec as HVec;
use spin::Mutex;
use v::vnet;

use crate::net::tls::{TlsClientConfig, TlsRoots};
use crate::net::tls_socket::{TlsCommand, TlsEvent, TlsTimeouts, register_tls_app_queues};
use crate::r::net::NetProfile;
use crate::r::net::Queue;

#[derive(Clone, Copy, Debug)]
pub enum DnsError {
    NoNic,
    BadName,
    Timeout,
    NoAnswer,
}

#[derive(Clone, Copy, Debug)]
pub struct DnsConfig {
    pub server_count: u8,
    pub timeout_ms: u64,
    pub resend_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecureDnsTransport {
    Doh,
    Dot,
}

impl SecureDnsTransport {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Doh => "doh",
            Self::Dot => "dot",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecureDnsPolicy {
    pub enabled: bool,
    pub warn_on_disagreement: bool,
    pub transports: [Option<SecureDnsTransport>; 3],
}

impl Default for SecureDnsPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            warn_on_disagreement: true,
            transports: [
                Some(SecureDnsTransport::Doh),
                Some(SecureDnsTransport::Dot),
                None,
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DnsIpv4Candidate {
    transport: SecureDnsTransport,
    ip: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DnsIpv6Candidate {
    transport: SecureDnsTransport,
    ip: [u8; 16],
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self::for_profile(NetProfile::default())
    }
}

impl DnsConfig {
    pub fn for_profile(profile: NetProfile) -> Self {
        if let Some(dev_idx) = profile.resolve_device_index() {
            Self::for_device(dev_idx)
        } else {
            Self::for_device(0)
        }
    }

    pub fn for_device(dev_idx: usize) -> Self {
        let (ra6, ra6_count) = crate::net::adapter::ra_dns6_snapshot_at(dev_idx)
            .unwrap_or_else(crate::net::adapter::primary_ra_dns6_snapshot);
        let (dhcp6, dhcp6_count) = crate::net::adapter::dhcp6_dns6_snapshot_at(dev_idx)
            .unwrap_or_else(crate::net::adapter::primary_dhcp6_dns6_snapshot);
        let (dhcp4, dhcp4_count) = crate::net::adapter::dhcp_dns_snapshot_at(dev_idx)
            .unwrap_or_else(crate::net::adapter::primary_dhcp_dns_snapshot);

        // Prefer network-provided DNS (RA RDNSS for v6, then DHCPv6, then DHCPv4), then fall back
        // to public resolvers. Keep both v6 and v4 fallbacks so we work on:
        // - v6-only networks (need v6 resolvers)
        // - v4-only networks (need v4 resolvers)
        // - dual-stack (either)
        let mut n: u8 = 0;

        for _ in 0..(ra6_count as usize).min(ra6.len()) {
            if (n as usize) >= 8 {
                break;
            }
            n = n.saturating_add(1);
        }
        for _ in 0..(dhcp6_count as usize).min(dhcp6.len()) {
            if (n as usize) >= 8 {
                break;
            }
            n = n.saturating_add(1);
        }
        for _ in 0..(dhcp4_count as usize).min(dhcp4.len()) {
            if (n as usize) >= 8 {
                break;
            }
            n = n.saturating_add(1);
        }
        for _ in PUBLIC_DNS_SERVERS_V6.iter() {
            if (n as usize) >= 8 {
                break;
            }
            n = n.saturating_add(1);
        }
        for _ in PUBLIC_DNS_SERVERS_V4.iter() {
            if (n as usize) >= 8 {
                break;
            }
            n = n.saturating_add(1);
        }

        Self {
            server_count: n,
            // Loader/CDN imports are sensitive to resolver jitter; use a less aggressive
            // default than 1.5s to avoid spurious NET_ERR_TIMEOUT_DNS.
            timeout_ms: 4000,
            resend_ms: 500,
        }
    }

    /// DNS config that uses IPv4 resolvers only.
    ///
    /// This is useful for IPv4-only workflows (like the boot netbench URL),
    /// where trying IPv6 resolvers first can introduce unnecessary delays or
    /// confusing logs on networks with partial/broken IPv6.
    pub fn for_device_v4_only(dev_idx: usize) -> Self {
        let (dhcp4, dhcp4_count) = crate::net::adapter::dhcp_dns_snapshot_at(dev_idx)
            .unwrap_or_else(crate::net::adapter::primary_dhcp_dns_snapshot);
        let router4 = crate::net::adapter::ipv4_router_snapshot_at(dev_idx)
            .flatten()
            .or_else(crate::net::adapter::primary_ipv4_router_snapshot);

        let mut n: u8 = 0;

        for _ in 0..(dhcp4_count as usize).min(dhcp4.len()) {
            if (n as usize) >= 8 {
                break;
            }
            n = n.saturating_add(1);
        }
        if let Some(router) = router4 {
            let duplicate = dhcp4[..(dhcp4_count as usize).min(dhcp4.len())]
                .iter()
                .any(|addr| *addr == router);
            if !duplicate && (n as usize) < 8 {
                n = n.saturating_add(1);
            }
        }
        for s in PUBLIC_DNS_SERVERS_V4.iter() {
            if (n as usize) >= 8 {
                break;
            }
            let duplicate = dhcp4[..(dhcp4_count as usize).min(dhcp4.len())]
                .iter()
                .any(|addr| addr == s)
                || router4 == Some(*s);
            if duplicate {
                continue;
            }
            n = n.saturating_add(1);
        }

        Self {
            server_count: n,
            timeout_ms: 4000,
            resend_ms: 500,
        }
    }
}

pub const PUBLIC_DNS_SERVERS: [[u8; 4]; 3] = [
    // Cloudflare
    [1, 1, 1, 1],
    // Google
    [8, 8, 8, 8],
    // Quad9
    [9, 9, 9, 9],
];

pub const PUBLIC_DNS_SERVERS_V4: [[u8; 4]; 3] = PUBLIC_DNS_SERVERS;

pub const PUBLIC_DNS_SERVERS_V6: [[u8; 16]; 3] = [
    // Cloudflare: 2606:4700:4700::1111
    [
        0x26, 0x06, 0x47, 0x00, 0x47, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11,
        0x11,
    ],
    // Google: 2001:4860:4860::8888
    [
        0x20, 0x01, 0x48, 0x60, 0x48, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x88,
        0x88,
    ],
    // Quad9: 2620:fe::fe
    [
        0x26, 0x20, 0x00, 0xfe, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xfe,
    ],
];

const DOT_PORT: u16 = 853;
const DOH_PORT: u16 = 443;
const DNS_QTYPE_A: u16 = 1;
const DNS_QTYPE_AAAA: u16 = 28;

const DNS_CACHE_CAP: usize = 8;
// Keep this short; it's mainly to avoid repeated lookups during module loading.
const DNS_CACHE_TTL_TICKS: u64 = 15 * embassy_time_driver::TICK_HZ;
const DNS_FS_CACHE_PATH: &str = "net_dns_v4.cache";
const DNS_FS_CACHE_MAX_ENTRIES: usize = 32;

#[derive(Clone, Debug)]
struct DnsCacheEntry {
    dev_idx: u8,
    host: heapless::String<96>,
    ip: [u8; 4],
    expires_at: u64,
}

static DNS_CACHE: Mutex<HVec<DnsCacheEntry, DNS_CACHE_CAP>> = Mutex::new(HVec::new());

static SECURE_DNS_STUB_LOGGED: AtomicU32 = AtomicU32::new(0);
static CLASSIC_DNS_DISABLED_LOGGED: AtomicU32 = AtomicU32::new(0);
static DNS_TLS_SEQ: AtomicU32 = AtomicU32::new(1);
static DNS_WIRE_QUERY_SEQ: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug)]
struct DotEndpoint {
    addr: [u8; 4],
    tls_name: &'static str,
}

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

fn log_dns_ip(prefix: &str, host: &str, dev_idx: usize, ip: [u8; 4]) {
    crate::log!(
        "{} host={} dev={} ip={}.{}.{}.{}\n",
        prefix,
        host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );
}

fn log_dns_transport_ip(
    prefix: &str,
    transport: SecureDnsTransport,
    host: &str,
    dev_idx: usize,
    ip: [u8; 4],
) {
    crate::log!(
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

fn log_dns_transport_ip6(
    prefix: &str,
    transport: SecureDnsTransport,
    host: &str,
    dev_idx: usize,
    ip: [u8; 16],
) {
    crate::log!(
        "{} transport={} host={} dev={} ip6={:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}\n",
        prefix,
        transport.label(),
        host,
        dev_idx,
        ip[0],
        ip[1],
        ip[2],
        ip[3],
        ip[4],
        ip[5],
        ip[6],
        ip[7],
        ip[8],
        ip[9],
        ip[10],
        ip[11],
        ip[12],
        ip[13],
        ip[14],
        ip[15]
    );
}

fn dns_cache_insert(dev_idx: usize, host_trimmed: &str, ip: [u8; 4]) {
    let Ok(hs) = heapless::String::<96>::try_from(host_trimmed) else {
        return;
    };
    let mut cache = DNS_CACHE.lock();
    cache.retain(|e| e.expires_at > embassy_time_driver::now());
    if let Some(e) = cache
        .iter_mut()
        .find(|e| e.dev_idx as usize == dev_idx && e.host.as_str() == hs.as_str())
    {
        e.ip = ip;
        e.expires_at = embassy_time_driver::now().saturating_add(DNS_CACHE_TTL_TICKS);
        return;
    }
    let dev = dev_idx.min(255) as u8;
    let _ = cache.push(DnsCacheEntry {
        dev_idx: dev,
        host: {
            let mut s = heapless::String::<96>::new();
            let _ = s.push_str(hs.as_str());
            s
        },
        ip,
        expires_at: embassy_time_driver::now().saturating_add(DNS_CACHE_TTL_TICKS),
    });
}

fn parse_ipv4_text(text: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = text.split('.');
    for octet in &mut out {
        let value = parts.next()?.trim().parse::<u8>().ok()?;
        *octet = value;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

fn parse_dns_fs_cache_line(line: &str) -> Option<(usize, &str, [u8; 4])> {
    let mut parts = line.splitn(3, '|');
    let dev_idx = parts.next()?.trim().parse::<usize>().ok()?;
    let host = parts.next()?.trim();
    let ip = parse_ipv4_text(parts.next()?.trim())?;
    if host.is_empty() {
        return None;
    }
    Some((dev_idx, host, ip))
}

async fn dns_fs_cache_lookup(dev_idx: usize, host_trimmed: &str) -> Option<[u8; 4]> {
    crate::log!("dns: fs-cache lookup begin host={} dev={}\n", host_trimmed, dev_idx);
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!(
            "dns: fs-cache lookup done host={} dev={} status=no-root\n",
            host_trimmed,
            dev_idx
        );
        return None;
    };
    let bytes = match crate::r::fs::trueosfs::file_out_if_index_ready_async(disk, DNS_FS_CACHE_PATH)
        .await
    {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            crate::log!(
                "dns: fs-cache lookup done host={} dev={} status=miss-or-index-not-ready\n",
                host_trimmed,
                dev_idx
            );
            return None;
        }
        Err(err) => {
            crate::log!(
                "dns: fs-cache lookup done host={} dev={} status=error err={:?}\n",
                host_trimmed,
                dev_idx,
                err
            );
            return None;
        }
    };
    let Ok(text) = core::str::from_utf8(bytes.as_slice()) else {
        crate::log!(
            "dns: fs-cache lookup done host={} dev={} status=bad-utf8 bytes={}\n",
            host_trimmed,
            dev_idx,
            bytes.len()
        );
        return None;
    };
    let mut found: Option<[u8; 4]> = None;
    for line in text.lines() {
        let Some((line_dev_idx, line_host, line_ip)) = parse_dns_fs_cache_line(line) else {
            continue;
        };
        if line_dev_idx == dev_idx && line_host == host_trimmed {
            found = Some(line_ip);
        }
    }
    crate::log!(
        "dns: fs-cache lookup done host={} dev={} hit={}\n",
        host_trimmed,
        dev_idx,
        found.is_some()
    );
    found
}

async fn dns_fs_cache_update(dev_idx: usize, host_trimmed: &str, ip: [u8; 4]) {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return;
    };

    let existing = crate::r::fs::trueosfs::file_out_if_index_ready_async(disk, DNS_FS_CACHE_PATH)
        .await
        .ok()
        .flatten();

    let mut lines: Vec<String> = Vec::new();
    if let Some(bytes) = existing
        && let Ok(text) = core::str::from_utf8(bytes.as_slice())
    {
        for line in text.lines() {
            let Some((line_dev_idx, line_host, line_ip)) = parse_dns_fs_cache_line(line) else {
                continue;
            };
            if line_dev_idx == dev_idx && line_host == host_trimmed {
                continue;
            }
            lines.push(format!(
                "{}|{}|{}.{}.{}.{}",
                line_dev_idx, line_host, line_ip[0], line_ip[1], line_ip[2], line_ip[3]
            ));
        }
    }

    lines.push(format!("{}|{}|{}.{}.{}.{}", dev_idx, host_trimmed, ip[0], ip[1], ip[2], ip[3]));
    if lines.len() > DNS_FS_CACHE_MAX_ENTRIES {
        let drop_n = lines.len().saturating_sub(DNS_FS_CACHE_MAX_ENTRIES);
        lines.drain(0..drop_n);
    }

    let mut body = String::new();
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            body.push('\n');
        }
        body.push_str(line.as_str());
    }
    let _ = crate::r::fs::trueosfs::file_in_async(disk, DNS_FS_CACHE_PATH, body.as_bytes()).await;
}

async fn resolve_ipv4_secure_transport(
    transport: SecureDnsTransport,
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    match transport {
        SecureDnsTransport::Doh => resolve_ipv4_doh_for_device(dev_idx, host_trimmed, cfg).await,
        SecureDnsTransport::Dot => resolve_ipv4_dot_for_device(dev_idx, host_trimmed, cfg).await,
    }
}

fn dns_tls_owner(prefix: &str, dev_idx: usize) -> &'static str {
    let seq = DNS_TLS_SEQ.fetch_add(1, Ordering::Relaxed);
    let selector = if let Some((bus, slot, func)) = crate::net::bdf_at(dev_idx) {
        format!("{:02x}:{:02x}.{}", bus, slot, func)
    } else if let Some((vid, pid)) = crate::net::pci_id_at(dev_idx) {
        format!("{:04x}:{:04x}", vid, pid)
    } else {
        format!("{}", dev_idx)
    };
    Box::leak(format!("{}-{}@{}", prefix, seq, selector).into_boxed_str())
}

fn dns_tls_queues(owner: &'static str) -> (&'static Queue<TlsCommand>, &'static Queue<TlsEvent>) {
    let cmds_name = Box::leak(format!("{}-tls-cmd", owner).into_boxed_str());
    let evts_name = Box::leak(format!("{}-tls-evt", owner).into_boxed_str());
    let cmds = Queue::new_leaked(cmds_name, 256);
    let events = Queue::new_leaked(evts_name, 1024);
    register_tls_app_queues(owner, cmds, events);
    (cmds, events)
}

fn build_dns_query(host_trimmed: &str, qtype: u16) -> Result<(u16, Vec<u8>), DnsError> {
    let id = DNS_WIRE_QUERY_SEQ.fetch_add(1, Ordering::Relaxed) as u16;
    let mut out = Vec::with_capacity(host_trimmed.len() + 18);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&0x0100u16.to_be_bytes()); // recursion desired
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());

    let mut total = 0usize;
    for label in host_trimmed.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
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
    out.extend_from_slice(&1u16.to_be_bytes()); // IN
    Ok((id, out))
}

fn skip_dns_name(buf: &[u8], mut pos: usize) -> Option<usize> {
    let mut jumps = 0usize;
    loop {
        let len = *buf.get(pos)?;
        if len & 0xC0 == 0xC0 {
            let _ = *buf.get(pos + 1)?;
            return Some(pos + 2);
        }
        if len == 0 {
            return Some(pos + 1);
        }
        if len & 0xC0 != 0 || len > 63 {
            return None;
        }
        pos = pos.checked_add(1 + len as usize)?;
        jumps += 1;
        if jumps > 128 || pos > buf.len() {
            return None;
        }
    }
}

fn parse_dns_response_addr<const N: usize>(
    buf: &[u8],
    expected_id: u16,
    qtype: u16,
) -> Result<[u8; N], DnsError> {
    if buf.len() < 12 {
        return Err(DnsError::NoAnswer);
    }
    let id = u16::from_be_bytes([buf[0], buf[1]]);
    if id != expected_id {
        return Err(DnsError::NoAnswer);
    }
    let flags = u16::from_be_bytes([buf[2], buf[3]]);
    if flags & 0x8000 == 0 || flags & 0x000f != 0 {
        return Err(DnsError::NoAnswer);
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut pos = 12usize;
    for _ in 0..qd {
        pos = skip_dns_name(buf, pos).ok_or(DnsError::NoAnswer)?;
        pos = pos.checked_add(4).ok_or(DnsError::NoAnswer)?;
        if pos > buf.len() {
            return Err(DnsError::NoAnswer);
        }
    }
    for _ in 0..an {
        pos = skip_dns_name(buf, pos).ok_or(DnsError::NoAnswer)?;
        if pos + 10 > buf.len() {
            return Err(DnsError::NoAnswer);
        }
        let typ = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let class = u16::from_be_bytes([buf[pos + 2], buf[pos + 3]]);
        let rdlen = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > buf.len() {
            return Err(DnsError::NoAnswer);
        }
        if typ == qtype && class == 1 && rdlen == N {
            let mut out = [0u8; N];
            out.copy_from_slice(&buf[pos..pos + rdlen]);
            return Ok(out);
        }
        pos += rdlen;
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

struct DohEndpoint {
    addr: [u8; 4],
    tls_name: &'static str,
    path: &'static str,
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

#[derive(Clone, Copy)]
enum SecureTlsReply {
    HttpBody,
    DotMessage,
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn header_value_usize(headers: &[u8], name: &[u8]) -> Option<usize> {
    for line in headers.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let key = &line[..colon];
        let value = &line[colon + 1..];
        if key.eq_ignore_ascii_case(name) {
            let s = core::str::from_utf8(value).ok()?.trim();
            return s.parse::<usize>().ok();
        }
    }
    None
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
                return Some(Ok(buf[2..2 + len].to_vec()));
            }
            None
        }
        SecureTlsReply::HttpBody => {
            let header_end = find_header_end(buf)?;
            let headers = &buf[..header_end];
            let body = &buf[header_end..];
            if !(headers.starts_with(b"HTTP/1.1 2") || headers.starts_with(b"HTTP/1.0 2")) {
                return Some(Err(DnsError::NoAnswer));
            }
            if super::http::header_contains_token(headers, b"Transfer-Encoding", b"chunked") {
                if let Some(decoded) = super::http::decode_http_chunked(body) {
                    return Some(Ok(decoded));
                }
                return closed.then_some(Err(DnsError::NoAnswer));
            }
            if let Some(len) = header_value_usize(headers, b"Content-Length") {
                if body.len() >= len {
                    return Some(Ok(body[..len].to_vec()));
                }
                return None;
            }
            if closed {
                return Some(Ok(body.to_vec()));
            }
            None
        }
    }
}

async fn dns_tls_exchange_v4(
    prefix: &str,
    dev_idx: usize,
    addr: [u8; 4],
    port: u16,
    server_name: &'static str,
    payload: Vec<u8>,
    timeout_ms: u64,
    mode: SecureTlsReply,
) -> Result<Vec<u8>, DnsError> {
    let owner = dns_tls_owner(prefix, dev_idx);
    let (cmds, events) = dns_tls_queues(owner);
    let cfg = if port == DOH_PORT {
        TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"])
    } else {
        TlsClientConfig::new()
    };
    let roots = TlsRoots::mozilla();
    let timeouts = TlsTimeouts {
        connect_ms: (timeout_ms as u32).max(1000),
        tls_ms: (timeout_ms as u32).max(1000),
        idle_ms: (timeout_ms as u32).max(1000),
    };
    cmds.push(TlsCommand::OpenTcpConnect {
        remote: vnet::EndpointV4 { addr, port },
        server_name,
        cfg,
        roots,
        timeouts,
    })
    .map_err(|_| DnsError::NoAnswer)?;

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
                        if handle.is_none() {
                            crate::log!(
                                "dns: secure tls recovered-open owner={} handle={}\n",
                                owner,
                                h.0
                            );
                        }
                        handle = Some(h);
                        if !sent {
                            let _ = cmds.push(TlsCommand::Send {
                                handle: h,
                                data: payload.clone(),
                            });
                            sent = true;
                        }
                    }
                }
                TlsEvent::Data { handle: h, data } if Some(h) == handle => {
                    rx.extend_from_slice(&data);
                    if let Some(done) = tls_reply_complete(&rx, mode, false) {
                        if let Some(h) = handle {
                            let _ = cmds.push(TlsCommand::Close { handle: h });
                        }
                        return done;
                    }
                }
                TlsEvent::Closed { handle: h } if Some(h) == handle => {
                    if let Some(done) = tls_reply_complete(&rx, mode, true) {
                        return done;
                    }
                    return Err(DnsError::NoAnswer);
                }
                TlsEvent::Error { msg } => {
                    crate::log!("dns: secure tls-socket error owner={} msg={}\n", owner, msg);
                    return Err(DnsError::NoAnswer);
                }
                TlsEvent::TlsError { err } => {
                    crate::log!("dns: secure tls error owner={} err={:?}\n", owner, err);
                    return Err(DnsError::NoAnswer);
                }
                _ => {}
            }
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    if let Some(h) = handle {
        let _ = cmds.push(TlsCommand::Close { handle: h });
    }
    Err(DnsError::Timeout)
}

async fn resolve_doh_addr<const N: usize>(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
    qtype: u16,
    qtype_label: &'static str,
) -> Result<[u8; N], DnsError> {
    let timeout_ms = cfg.timeout_ms.max(cfg.resend_ms).max(100);
    crate::log!(
        "dns: doh enter host={} dev={} qtype={} timeout_ms={} tls=trueos\n",
        host_trimmed,
        dev_idx,
        qtype_label,
        timeout_ms
    );

    let (query_id, query) = build_dns_query(host_trimmed, qtype)?;
    let encoded = base64url_no_pad(&query);
    for endpoint in PUBLIC_DOH_ENDPOINTS.iter() {
        let path = format!("{}?dns={}", endpoint.path, encoded);
        let req = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: application/dns-message\r\nConnection: close\r\nUser-Agent: TRUEOS/secure-dns\r\n\r\n",
            path,
            endpoint.tls_name
        )
        .into_bytes();
        match dns_tls_exchange_v4(
            "dns-doh",
            dev_idx,
            endpoint.addr,
            DOH_PORT,
            endpoint.tls_name,
            req,
            timeout_ms,
            SecureTlsReply::HttpBody,
        )
        .await
        {
            Ok(body) => match parse_dns_response_addr::<N>(&body, query_id, qtype) {
                Ok(ip) => return Ok(ip),
                Err(err) => {
                    crate::log!(
                        "dns: doh response parse failed host={} dev={} server={} qtype={} err={:?}\n",
                        host_trimmed,
                        dev_idx,
                        endpoint.tls_name,
                        qtype_label,
                        err
                    );
                }
            },
            Err(err) => {
                crate::log!(
                    "dns: doh endpoint failed host={} dev={} server={} qtype={} err={:?}\n",
                    host_trimmed,
                    dev_idx,
                    endpoint.tls_name,
                    qtype_label,
                    err
                );
            }
        }
    }
    Err(DnsError::NoAnswer)
}

async fn resolve_dot_addr<const N: usize>(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
    qtype: u16,
    qtype_label: &'static str,
) -> Result<[u8; N], DnsError> {
    let timeout_ms = cfg.timeout_ms.max(cfg.resend_ms).max(100);
    crate::log!(
        "dns: dot enter host={} dev={} qtype={} timeout_ms={} tls=trueos\n",
        host_trimmed,
        dev_idx,
        qtype_label,
        timeout_ms
    );

    let (query_id, query) = build_dns_query(host_trimmed, qtype)?;
    let mut payload = Vec::with_capacity(query.len() + 2);
    payload.extend_from_slice(&(query.len() as u16).to_be_bytes());
    payload.extend_from_slice(&query);
    for endpoint in PUBLIC_DOT_ENDPOINTS.iter() {
        match dns_tls_exchange_v4(
            "dns-dot",
            dev_idx,
            endpoint.addr,
            DOT_PORT,
            endpoint.tls_name,
            payload.clone(),
            timeout_ms,
            SecureTlsReply::DotMessage,
        )
        .await
        {
            Ok(body) => match parse_dns_response_addr::<N>(&body, query_id, qtype) {
                Ok(ip) => return Ok(ip),
                Err(err) => {
                    crate::log!(
                        "dns: dot response parse failed host={} dev={} server={} qtype={} err={:?}\n",
                        host_trimmed,
                        dev_idx,
                        endpoint.tls_name,
                        qtype_label,
                        err
                    );
                }
            },
            Err(err) => {
                crate::log!(
                    "dns: dot endpoint failed host={} dev={} server={} qtype={} err={:?}\n",
                    host_trimmed,
                    dev_idx,
                    endpoint.tls_name,
                    qtype_label,
                    err
                );
            }
        }
    }
    Err(DnsError::NoAnswer)
}

async fn resolve_ipv4_doh_for_device(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    resolve_doh_addr::<4>(dev_idx, host_trimmed, cfg, DNS_QTYPE_A, "A").await
}

async fn resolve_ipv6_doh_for_device(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    resolve_doh_addr::<16>(dev_idx, host_trimmed, cfg, DNS_QTYPE_AAAA, "AAAA").await
}

async fn resolve_ipv4_dot_for_device(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    resolve_dot_addr::<4>(dev_idx, host_trimmed, cfg, DNS_QTYPE_A, "A").await
}

async fn resolve_ipv6_dot_for_device(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    resolve_dot_addr::<16>(dev_idx, host_trimmed, cfg, DNS_QTYPE_AAAA, "AAAA").await
}

fn warn_secure_dns_disagreement(
    host_trimmed: &str,
    dev_idx: usize,
    first: DnsIpv4Candidate,
    other: DnsIpv4Candidate,
) {
    if first.ip == other.ip {
        return;
    }

    crate::log!(
        "dns: warning secure disagreement host={} dev={} first={} {}.{}.{}.{} other={} {}.{}.{}.{}\n",
        host_trimmed,
        dev_idx,
        first.transport.label(),
        first.ip[0],
        first.ip[1],
        first.ip[2],
        first.ip[3],
        other.transport.label(),
        other.ip[0],
        other.ip[1],
        other.ip[2],
        other.ip[3]
    );
}

fn record_secure_dns_result(
    first_healthy: &mut Option<DnsIpv4Candidate>,
    policy: SecureDnsPolicy,
    transport: SecureDnsTransport,
    result: Result<[u8; 4], DnsError>,
    host_trimmed: &str,
    dev_idx: usize,
) {
    match result {
        Ok(ip) => {
            let candidate = DnsIpv4Candidate { transport, ip };
            log_dns_transport_ip("dns: secure resolved", transport, host_trimmed, dev_idx, ip);

            if let Some(first) = *first_healthy {
                if policy.warn_on_disagreement {
                    warn_secure_dns_disagreement(host_trimmed, dev_idx, first, candidate);
                }
            } else {
                *first_healthy = Some(candidate);
            }
        }
        Err(DnsError::NoAnswer) => {
            crate::log!(
                "dns: secure transport no-answer transport={} host={} dev={} qtype=A\n",
                transport.label(),
                host_trimmed,
                dev_idx
            );
        }
        Err(err) => {
            crate::log!(
                "dns: secure transport failed transport={} host={} dev={} qtype=A err={:?}\n",
                transport.label(),
                host_trimmed,
                dev_idx,
                err
            );
        }
    }
}

async fn resolve_ipv4_secure_policy(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
    policy: SecureDnsPolicy,
) -> Result<[u8; 4], DnsError> {
    if !policy.enabled {
        return Err(DnsError::NoAnswer);
    }

    if SECURE_DNS_STUB_LOGGED
        .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        crate::log!("dns: secure transport policy wired; classic udp dns disabled\n");
    }

    let doh_enabled = policy
        .transports
        .iter()
        .flatten()
        .any(|transport| *transport == SecureDnsTransport::Doh);
    let dot_enabled = policy
        .transports
        .iter()
        .flatten()
        .any(|transport| *transport == SecureDnsTransport::Dot);

    let mut first_healthy: Option<DnsIpv4Candidate> = None;
    if doh_enabled && dot_enabled {
        let (doh_result, dot_result) = tokio::join!(
            resolve_ipv4_doh_for_device(dev_idx, host_trimmed, cfg),
            resolve_ipv4_dot_for_device(dev_idx, host_trimmed, cfg)
        );
        record_secure_dns_result(
            &mut first_healthy,
            policy,
            SecureDnsTransport::Doh,
            doh_result,
            host_trimmed,
            dev_idx,
        );
        record_secure_dns_result(
            &mut first_healthy,
            policy,
            SecureDnsTransport::Dot,
            dot_result,
            host_trimmed,
            dev_idx,
        );
    }

    for transport in policy.transports.iter().flatten().copied() {
        if (transport == SecureDnsTransport::Doh && doh_enabled && dot_enabled)
            || (transport == SecureDnsTransport::Dot && doh_enabled && dot_enabled)
        {
            continue;
        }
        let result = resolve_ipv4_secure_transport(transport, dev_idx, host_trimmed, cfg).await;
        record_secure_dns_result(
            &mut first_healthy,
            policy,
            transport,
            result,
            host_trimmed,
            dev_idx,
        );
    }

    first_healthy
        .map(|candidate| candidate.ip)
        .ok_or(DnsError::NoAnswer)
}

fn warn_secure_dns6_disagreement(
    host_trimmed: &str,
    dev_idx: usize,
    first: DnsIpv6Candidate,
    other: DnsIpv6Candidate,
) {
    if first.ip == other.ip {
        return;
    }

    crate::log!(
        "dns: warning secure ipv6 disagreement host={} dev={} first={} other={}\n",
        host_trimmed,
        dev_idx,
        first.transport.label(),
        other.transport.label()
    );
}

fn record_secure_dns6_result(
    first_healthy: &mut Option<DnsIpv6Candidate>,
    policy: SecureDnsPolicy,
    transport: SecureDnsTransport,
    result: Result<[u8; 16], DnsError>,
    host_trimmed: &str,
    dev_idx: usize,
) {
    match result {
        Ok(ip) => {
            let candidate = DnsIpv6Candidate { transport, ip };
            log_dns_transport_ip6("dns: secure resolved", transport, host_trimmed, dev_idx, ip);

            if let Some(first) = *first_healthy {
                if policy.warn_on_disagreement {
                    warn_secure_dns6_disagreement(host_trimmed, dev_idx, first, candidate);
                }
            } else {
                *first_healthy = Some(candidate);
            }
        }
        Err(DnsError::NoAnswer) => {
            crate::log!(
                "dns: secure transport no-answer transport={} host={} dev={} qtype=AAAA\n",
                transport.label(),
                host_trimmed,
                dev_idx
            );
        }
        Err(err) => {
            crate::log!(
                "dns: secure transport failed transport={} host={} dev={} qtype=AAAA err={:?}\n",
                transport.label(),
                host_trimmed,
                dev_idx,
                err
            );
        }
    }
}

async fn resolve_ipv6_secure_transport(
    transport: SecureDnsTransport,
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    match transport {
        SecureDnsTransport::Doh => resolve_ipv6_doh_for_device(dev_idx, host_trimmed, cfg).await,
        SecureDnsTransport::Dot => resolve_ipv6_dot_for_device(dev_idx, host_trimmed, cfg).await,
    }
}

async fn resolve_ipv6_secure_policy(
    dev_idx: usize,
    host_trimmed: &str,
    cfg: DnsConfig,
    policy: SecureDnsPolicy,
) -> Result<[u8; 16], DnsError> {
    if !policy.enabled {
        return Err(DnsError::NoAnswer);
    }

    let doh_enabled = policy
        .transports
        .iter()
        .flatten()
        .any(|transport| *transport == SecureDnsTransport::Doh);
    let dot_enabled = policy
        .transports
        .iter()
        .flatten()
        .any(|transport| *transport == SecureDnsTransport::Dot);

    let mut first_healthy: Option<DnsIpv6Candidate> = None;
    if doh_enabled && dot_enabled {
        let (doh_result, dot_result) = tokio::join!(
            resolve_ipv6_doh_for_device(dev_idx, host_trimmed, cfg),
            resolve_ipv6_dot_for_device(dev_idx, host_trimmed, cfg)
        );
        record_secure_dns6_result(
            &mut first_healthy,
            policy,
            SecureDnsTransport::Doh,
            doh_result,
            host_trimmed,
            dev_idx,
        );
        record_secure_dns6_result(
            &mut first_healthy,
            policy,
            SecureDnsTransport::Dot,
            dot_result,
            host_trimmed,
            dev_idx,
        );
    }

    for transport in policy.transports.iter().flatten().copied() {
        if (transport == SecureDnsTransport::Doh && doh_enabled && dot_enabled)
            || (transport == SecureDnsTransport::Dot && doh_enabled && dot_enabled)
        {
            continue;
        }
        let result = resolve_ipv6_secure_transport(transport, dev_idx, host_trimmed, cfg).await;
        record_secure_dns6_result(
            &mut first_healthy,
            policy,
            transport,
            result,
            host_trimmed,
            dev_idx,
        );
    }

    first_healthy
        .map(|candidate| candidate.ip)
        .ok_or(DnsError::NoAnswer)
}

pub async fn resolve_ipv4_for_device(
    dev_idx: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let host_trimmed = host.trim().trim_end_matches('.');
    if host_trimmed.is_empty() {
        return Err(DnsError::BadName);
    }
    crate::log!("dns: resolve begin host={} dev={} qtype=A\n", host_trimmed, dev_idx);

    {
        let now = embassy_time_driver::now();
        let mut cache = DNS_CACHE.lock();
        cache.retain(|e| e.expires_at > now);
        if let Some(e) = cache
            .iter()
            .find(|e| e.dev_idx as usize == dev_idx && e.host.as_str() == host_trimmed)
        {
            log_dns_ip("dns: cache hit", host_trimmed, dev_idx, e.ip);
            return Ok(e.ip);
        }
    }

    if let Some(ip) = dns_fs_cache_lookup(dev_idx, host_trimmed).await {
        dns_cache_insert(dev_idx, host_trimmed, ip);
        log_dns_ip("dns: fs-cache hit", host_trimmed, dev_idx, ip);
        return Ok(ip);
    }

    crate::log!(
        "dns: secure lookup begin host={} dev={} qtype=A servers={} timeout_ms={} resend_ms={}\n",
        host_trimmed,
        dev_idx,
        cfg.server_count,
        cfg.timeout_ms,
        cfg.resend_ms
    );
    if let Ok(ip) =
        resolve_ipv4_secure_policy(dev_idx, host_trimmed, cfg, SecureDnsPolicy::default()).await
    {
        dns_cache_insert(dev_idx, host_trimmed, ip);
        dns_fs_cache_update(dev_idx, host_trimmed, ip).await;
        return Ok(ip);
    }
    crate::log!("dns: secure lookup failed host={} dev={} qtype=A\n", host_trimmed, dev_idx);

    if CLASSIC_DNS_DISABLED_LOGGED
        .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        crate::log!(
            "dns: no secure dns possible host={} dev={} qtype=A; network lookup discarded\n",
            host_trimmed,
            dev_idx
        );
    }
    Err(DnsError::NoAnswer)
}

pub async fn resolve_ipv6_for_device(
    dev_idx: usize,
    host: &str,
    cfg: DnsConfig,
) -> Result<[u8; 16], DnsError> {
    let host_trimmed = host.trim().trim_end_matches('.');
    if host_trimmed.is_empty() {
        return Err(DnsError::BadName);
    }

    if let Ok(ip) =
        resolve_ipv6_secure_policy(dev_idx, host_trimmed, cfg, SecureDnsPolicy::default()).await
    {
        return Ok(ip);
    }

    if CLASSIC_DNS_DISABLED_LOGGED
        .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
        crate::log!(
            "dns: no secure dns possible host={} dev={} qtype=AAAA; network lookup discarded\n",
            host_trimmed,
            dev_idx
        );
    }
    Err(DnsError::NoAnswer)
}

#[inline]
pub async fn resolve_ipv4_primary(host: &str, cfg: DnsConfig) -> Result<[u8; 4], DnsError> {
    resolve_ipv4_with_profile(host, NetProfile::default(), cfg).await
}

#[inline]
pub async fn resolve_ipv4_with_profile(
    host: &str,
    profile: NetProfile,
    cfg: DnsConfig,
) -> Result<[u8; 4], DnsError> {
    let dev_idx = profile.resolve_device_index().ok_or(DnsError::NoNic)?;
    resolve_ipv4_for_device(dev_idx, host, cfg).await
}
